"""Small local GeoJSON bridge for Overture Maps building data.

It reads only the Parquet row groups intersecting the requested bounding box.
No complete Overture release or Minecraft world is downloaded.
"""

from __future__ import annotations

import io
import http.client
import hashlib
import json
import math
import os
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

import pyarrow.parquet as parquet
from shapely import from_wkb
from shapely.geometry import mapping

HOST = os.getenv("OVERTURE_BRIDGE_HOST", "127.0.0.1")
PORT = int(os.getenv("OVERTURE_BRIDGE_PORT", "8765"))
CACHE_ROOT = Path(os.getenv("OVERTURE_CACHE_DIR", Path(__file__).parents[1] / ".cache" / "overture"))
USER_AGENT = "Campus-Reconstruction-Tool/0.1"
STAC_ROOT = "https://stac.overturemaps.org"
MAX_LIMIT = 200
MAX_BBOX_SPAN = 0.02
QUERY_LOCK = threading.Lock()


class BridgeError(RuntimeError):
    pass


class HttpRangeReader(io.RawIOBase):
    """Seekable HTTP reader used by PyArrow to fetch Parquet byte ranges."""

    def __init__(self, url: str):
        self.url = url
        self.position = 0
        parsed = urllib.parse.urlsplit(url)
        self.path = urllib.parse.urlunsplit(("", "", parsed.path, parsed.query, ""))
        self.connection = http.client.HTTPSConnection(parsed.hostname, parsed.port or 443, timeout=60)
        self.connection.request("HEAD", self.path, headers={"User-Agent": USER_AGENT})
        response = self.connection.getresponse()
        response.read()
        if response.status >= 400:
            raise BridgeError(f"Overture partition HEAD returned HTTP {response.status}")
        self.size = int(response.headers["Content-Length"])

    def readable(self) -> bool:
        return True

    def seekable(self) -> bool:
        return True

    def tell(self) -> int:
        return self.position

    def seek(self, offset: int, whence: int = io.SEEK_SET) -> int:
        if whence == io.SEEK_SET:
            next_position = offset
        elif whence == io.SEEK_CUR:
            next_position = self.position + offset
        elif whence == io.SEEK_END:
            next_position = self.size + offset
        else:
            raise ValueError(f"Unsupported seek mode: {whence}")
        self.position = max(0, min(self.size, next_position))
        return self.position

    def read(self, size: int = -1) -> bytes:
        if self.position >= self.size:
            return b""
        if size is None or size < 0:
            size = self.size - self.position
        end = min(self.size - 1, self.position + size - 1)
        headers = {"Range": f"bytes={self.position}-{end}", "User-Agent": USER_AGENT}
        try:
            self.connection.request("GET", self.path, headers=headers)
            response = self.connection.getresponse()
            data = response.read()
        except (OSError, http.client.HTTPException):
            parsed = urllib.parse.urlsplit(self.url)
            self.connection.close()
            self.connection = http.client.HTTPSConnection(parsed.hostname, parsed.port or 443, timeout=60)
            self.connection.request("GET", self.path, headers=headers)
            response = self.connection.getresponse()
            data = response.read()
        if response.status not in (200, 206):
            raise BridgeError(f"Overture partition range returned HTTP {response.status}")
        self.position += len(data)
        return data

    def close(self) -> None:
        if hasattr(self, "connection"):
            self.connection.close()
        super().close()


def _download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".tmp")
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response, temporary.open("wb") as output:
        output.write(response.read())
    temporary.replace(destination)


def resolve_release(requested: str | None = None) -> str:
    if requested and requested.lower() != "latest":
        if not all(part.isdigit() for part in requested.split("-")[0].split(".")):
            raise BridgeError("Invalid Overture release id")
        return requested
    latest_path = CACHE_ROOT / "latest.txt"
    if latest_path.exists() and time.time() - latest_path.stat().st_mtime < 86_400:
        return latest_path.read_text(encoding="utf-8").strip()
    request = urllib.request.Request(f"{STAC_ROOT}/catalog.json", headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            catalog = json.load(response)
    except (urllib.error.URLError, TimeoutError):
        if latest_path.exists():
            return latest_path.read_text(encoding="utf-8").strip()
        raise
    release_ids = [link.get("href", "").rstrip("/").split("/")[-2]
                   for link in catalog.get("links", []) if link.get("rel") == "child"]
    release_ids = [release for release in release_ids if release and release[0].isdigit()]
    if not release_ids:
        raise BridgeError("The Overture STAC catalog did not report a release")
    release = max(release_ids)
    latest_path.parent.mkdir(parents=True, exist_ok=True)
    latest_path.write_text(release, encoding="utf-8")
    return release


def parse_bbox(params: dict[str, list[str]]) -> tuple[float, float, float, float]:
    if params.get("bbox"):
        values = [float(value) for value in params["bbox"][0].split(",")]
        if len(values) != 4:
            raise BridgeError("bbox must contain west,south,east,north")
        west, south, east, north = values
    else:
        longitude = float(params.get("lng", [""])[0])
        latitude = float(params.get("lat", [""])[0])
        radius = min(250.0, max(1.0, float(params.get("radius_m", ["120"])[0])))
        latitude_delta = radius / 111_320.0
        longitude_delta = radius / (111_320.0 * max(0.1, math.cos(math.radians(latitude))))
        west, south, east, north = (
            longitude - longitude_delta, latitude - latitude_delta,
            longitude + longitude_delta, latitude + latitude_delta,
        )
    if not (-180 <= west < east <= 180 and -90 <= south < north <= 90):
        raise BridgeError("bbox coordinates are invalid")
    if east - west > MAX_BBOX_SPAN or north - south > MAX_BBOX_SPAN:
        raise BridgeError("bbox is too large for the local building service")
    return west, south, east, north


def _overlaps(a: dict[str, float], bbox: tuple[float, float, float, float]) -> bool:
    west, south, east, north = bbox
    return a["xmin"] <= east and a["xmax"] >= west and a["ymin"] <= north and a["ymax"] >= south


def partition_urls(release: str, bbox: tuple[float, float, float, float]) -> list[str]:
    catalog_path = CACHE_ROOT / release / "collections.parquet"
    if not catalog_path.exists():
        _download(f"{STAC_ROOT}/{release}/collections.parquet", catalog_path)
    table = parquet.read_table(catalog_path, columns=["collection", "bbox", "assets"])
    urls: list[str] = []
    for row in table.to_pylist():
        if row["collection"] != "building" or not _overlaps(row["bbox"], bbox):
            continue
        assets = row.get("assets") or {}
        asset = assets.get("azure") or assets.get("aws")
        if asset and asset.get("href"):
            urls.append(asset["href"])
    if not urls:
        raise BridgeError("No Overture building partition intersects this bbox")
    return urls


def matching_row_groups(dataset: parquet.ParquetFile, bbox: tuple[float, float, float, float]) -> list[int]:
    west, south, east, north = bbox
    paths = {dataset.metadata.row_group(0).column(index).path_in_schema: index
             for index in range(dataset.metadata.row_group(0).num_columns)}
    required = ("bbox.xmin", "bbox.xmax", "bbox.ymin", "bbox.ymax")
    if not all(name in paths for name in required):
        return list(range(dataset.num_row_groups))
    groups: list[int] = []
    for group_index in range(dataset.num_row_groups):
        group = dataset.metadata.row_group(group_index)
        stats = {name: group.column(paths[name]).statistics for name in required}
        if not all(value and value.has_min_max for value in stats.values()):
            groups.append(group_index)
            continue
        if (stats["bbox.xmin"].min <= east and stats["bbox.xmax"].max >= west
                and stats["bbox.ymin"].min <= north and stats["bbox.ymax"].max >= south):
            groups.append(group_index)
    return groups


def _row_group_index(dataset: parquet.ParquetFile) -> list[dict[str, float | int]]:
    paths = {dataset.metadata.row_group(0).column(index).path_in_schema: index
             for index in range(dataset.metadata.row_group(0).num_columns)}
    result: list[dict[str, float | int]] = []
    for group_index in range(dataset.num_row_groups):
        group = dataset.metadata.row_group(group_index)
        values: dict[str, float | int] = {"group": group_index}
        for name in ("bbox.xmin", "bbox.xmax", "bbox.ymin", "bbox.ymax"):
            statistics = group.column(paths[name]).statistics
            values[name] = float(statistics.min if name.endswith(("xmin", "ymin")) else statistics.max)
        result.append(values)
    return result


def _groups_from_index(index: list[dict[str, float | int]], bbox: tuple[float, float, float, float]) -> list[int]:
    west, south, east, north = bbox
    return [int(item["group"]) for item in index
            if item["bbox.xmin"] <= east and item["bbox.xmax"] >= west
            and item["bbox.ymin"] <= north and item["bbox.ymax"] >= south]


def _primary_name(names: Any) -> str | None:
    if not isinstance(names, dict):
        return None
    primary = names.get("primary")
    if isinstance(primary, str):
        return primary
    if isinstance(primary, dict):
        return primary.get("value") or next((value for value in primary.values() if isinstance(value, str)), None)
    return None


def row_to_feature(row: dict[str, Any]) -> dict[str, Any] | None:
    if not row.get("geometry"):
        return None
    geometry = from_wkb(row["geometry"])
    if geometry.is_empty or geometry.geom_type not in ("Polygon", "MultiPolygon"):
        return None
    properties = {
        "name": _primary_name(row.get("names")),
        "building": row.get("class") or row.get("subtype") or "yes",
        "height": row.get("height"),
        "min_height": row.get("min_height"),
        "building:levels": row.get("num_floors"),
        "roof:shape": row.get("roof_shape"),
        "roof:material": row.get("roof_material"),
        "roof:height": row.get("roof_height"),
        "building:material": row.get("facade_material"),
        "building:colour": row.get("facade_color"),
        "has_parts": row.get("has_parts"),
        "source": "overture_maps",
    }
    return {
        "type": "Feature",
        "id": row.get("id"),
        "geometry": mapping(geometry),
        "properties": {key: value for key, value in properties.items() if value is not None},
    }


def _building_distance_score(
        building_bbox: dict[str, float],
        query_bbox: tuple[float, float, float, float]) -> float:
    west, south, east, north = query_bbox
    query_lng = (west + east) / 2
    query_lat = (south + north) / 2
    building_lng = (building_bbox["xmin"] + building_bbox["xmax"]) / 2
    building_lat = (building_bbox["ymin"] + building_bbox["ymax"]) / 2
    lng_scale = math.cos(math.radians(query_lat))
    return ((building_lng - query_lng) * lng_scale) ** 2 + (building_lat - query_lat) ** 2


def query_buildings(bbox: tuple[float, float, float, float], limit: int, release: str) -> list[dict[str, Any]]:
    ranked_features: list[tuple[float, dict[str, Any]]] = []
    columns = ["id", "height", "min_height", "num_floors", "subtype", "class",
               "roof_shape", "roof_height", "geometry", "has_parts", "bbox"]
    for url in partition_urls(release, bbox):
        partition_key = hashlib.sha256(url.encode("utf-8")).hexdigest()[:20]
        partition_cache = CACHE_ROOT / release / "row-groups" / partition_key
        index_path = partition_cache / "index.json"
        dataset = None
        if index_path.exists():
            index = json.loads(index_path.read_text(encoding="utf-8"))
            groups = _groups_from_index(index, bbox)
        else:
            dataset = parquet.ParquetFile(HttpRangeReader(url))
            index = _row_group_index(dataset)
            groups = _groups_from_index(index, bbox)
            partition_cache.mkdir(parents=True, exist_ok=True)
            index_path.write_text(json.dumps(index, separators=(",", ":")), encoding="utf-8")
        if not groups:
            continue
        for group in groups:
            group_path = partition_cache / f"{group}.parquet"
            if group_path.exists():
                table = parquet.read_table(group_path)
            else:
                if dataset is None:
                    dataset = parquet.ParquetFile(HttpRangeReader(url))
                table = dataset.read_row_group(group, columns=columns)
                temporary = group_path.with_suffix(".tmp.parquet")
                parquet.write_table(table, temporary, compression="zstd")
                temporary.replace(group_path)
            for row in table.to_pylist():
                if not _overlaps(row["bbox"], bbox):
                    continue
                feature = row_to_feature(row)
                if feature:
                    ranked_features.append((
                        _building_distance_score(row["bbox"], bbox),
                        feature
                    ))
    ranked_features.sort(key=lambda item: item[0])
    return [feature for _, feature in ranked_features[:limit]]


class Handler(BaseHTTPRequestHandler):
    server_version = "OvertureLocalBridge/0.1"

    def _json(self, status: int, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_GET(self) -> None:  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path == "/health":
            self._json(200, {"status": "ok", "service": "overture-local-bridge"})
            return
        if parsed.path != "/overture/buildings":
            self._json(404, {"error": "Not found"})
            return
        started = time.monotonic()
        try:
            params = urllib.parse.parse_qs(parsed.query)
            bbox = parse_bbox(params)
            limit = min(MAX_LIMIT, max(1, int(params.get("limit", ["50"])[0])))
            release = resolve_release(params.get("release", [os.getenv("OVERTURE_RELEASE_ID", "latest")])[0])
            with QUERY_LOCK:
                features = query_buildings(bbox, limit, release)
            self._json(200, {
                "type": "FeatureCollection",
                "features": features,
                "metadata": {"releaseId": release, "bbox": bbox, "limit": limit,
                             "elapsedMs": round((time.monotonic() - started) * 1000)},
            })
        except (BridgeError, ValueError) as error:
            self._json(400, {"error": str(error)})
        except (urllib.error.URLError, TimeoutError) as error:
            self._json(504, {"error": f"Overture upstream request failed: {error}"})
        except Exception as error:  # keep the desktop app recoverable
            self._json(500, {"error": f"Overture bridge failed: {error}"})

    def log_message(self, fmt: str, *args: Any) -> None:
        try:
            print(f"[{self.log_date_time_string()}] {fmt % args}", flush=True)
        except (BrokenPipeError, OSError):
            pass


def main() -> None:
    CACHE_ROOT.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer((HOST, PORT), Handler)
    print(f"Overture local bridge listening on http://{HOST}:{PORT}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
