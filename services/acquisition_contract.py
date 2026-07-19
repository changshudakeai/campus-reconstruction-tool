"""Provider-neutral decoder for the frozen Controlled Acquisition v1 contract."""

import base64
import copy
import hashlib
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping

from jsonschema import Draft202012Validator


CONTRACT_VERSION = "1.0.0"
COMPLETE_STATUSES = frozenset({"complete", "complete-empty"})
KNOWN_STATUSES = COMPLETE_STATUSES | {"partial", "failed", "cancelled"}
KNOWN_CATEGORIES = frozenset(
    {"building", "circulation", "water", "vegetation", "sports"}
)
KNOWN_GEOMETRIES = frozenset(
    {"Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon"}
)


class ContractDecodeError(ValueError):
    pass


@dataclass(frozen=True)
class ProviderOutcome:
    provider: str
    category: str
    tile_id: str
    status: str
    pagination_exhausted: bool
    relation_members_complete: bool
    raw_count: int
    deduplicated_count: int

    @property
    def is_complete(self) -> bool:
        return (
            self.status in COMPLETE_STATUSES
            and self.pagination_exhausted
            and self.relation_members_complete
        )


@dataclass(frozen=True)
class SourceObservation:
    observation_id: str
    category: str
    geometry_type: str
    provider: str
    source_record_id: str
    licence_identifier: str
    coordinate_reference_system: str


@dataclass(frozen=True)
class AcquisitionFixture:
    contract_version: str
    bundle_id: str
    outcomes: tuple[ProviderOutcome, ...]
    observations: tuple[SourceObservation, ...]


def _mapping(value: Any, field: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise ContractDecodeError(f"{field} must be an object")
    return value


def _text(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractDecodeError(f"{field} must be a non-empty string")
    return value


def decode_contract_fixture(value: Mapping[str, Any]) -> AcquisitionFixture:
    contract_version = _text(value.get("contract_version"), "contract_version")
    if contract_version != CONTRACT_VERSION:
        raise ContractDecodeError(
            f"unsupported contract {contract_version}; expected {CONTRACT_VERSION}"
        )
    bundle = _mapping(value.get("bundle"), "bundle")
    coverage = _mapping(value.get("coverage_report"), "coverage_report")

    outcomes = []
    for index, item in enumerate(coverage.get("outcomes", ())):
        outcome = _mapping(item, f"coverage_report.outcomes[{index}]")
        status = _text(outcome.get("status"), "status")
        category = _text(outcome.get("category"), "category")
        if status not in KNOWN_STATUSES:
            raise ContractDecodeError(f"unknown provider outcome status: {status}")
        if category not in KNOWN_CATEGORIES:
            raise ContractDecodeError(f"unknown Foundation category: {category}")
        outcomes.append(
            ProviderOutcome(
                provider=_text(outcome.get("provider"), "provider"),
                category=category,
                tile_id=_text(outcome.get("tile_id"), "tile_id"),
                status=status,
                pagination_exhausted=outcome.get("pagination_exhausted") is True,
                relation_members_complete=outcome.get("relation_members_complete")
                is True,
                raw_count=int(outcome.get("raw_count", 0)),
                deduplicated_count=int(outcome.get("deduplicated_count", 0)),
            )
        )

    observations = []
    for index, item in enumerate(value.get("observations", ())):
        observation = _mapping(item, f"observations[{index}]")
        geometry = _mapping(observation.get("geometry"), "geometry")
        lineage = _mapping(observation.get("lineage"), "lineage")
        licence = _mapping(observation.get("licence"), "licence")
        coordinates = _mapping(
            observation.get("coordinate_semantics"), "coordinate_semantics"
        )
        geometry_type = _text(geometry.get("type"), "geometry.type")
        if geometry_type not in KNOWN_GEOMETRIES:
            raise ContractDecodeError(f"unknown typed geometry: {geometry_type}")
        observations.append(
            SourceObservation(
                observation_id=_text(observation.get("id"), "observation.id"),
                category=_text(observation.get("category"), "observation.category"),
                geometry_type=geometry_type,
                provider=_text(lineage.get("provider"), "lineage.provider"),
                source_record_id=_text(
                    lineage.get("source_record_id"), "lineage.source_record_id"
                ),
                licence_identifier=_text(
                    licence.get("identifier"), "licence.identifier"
                ),
                coordinate_reference_system=_text(
                    coordinates.get("crs"), "coordinate_semantics.crs"
                ),
            )
        )

    return AcquisitionFixture(
        contract_version=contract_version,
        bundle_id=_text(bundle.get("id"), "bundle.id"),
        outcomes=tuple(outcomes),
        observations=tuple(observations),
    )


@dataclass(frozen=True)
class ServiceResponse:
    status: int
    body: bytes
    headers: Mapping[str, str]


class FixtureAcquisitionService:
    """Executable service-side v1 contract used only by tests and development."""

    _JOB_PATH = re.compile(
        r"^/v1/(?P<kind>boundary-jobs|acquisition-jobs)/(?P<job>[^/]+)"
        r"(?:/(?P<action>retry|cancel|manifest|chunks)(?:/(?P<chunk>[^/]+))?)?$"
    )

    def __init__(self, fixture_dir: Path):
        self._acquisition = json.loads(
            (fixture_dir / "canonical-acquisition.json").read_text(encoding="utf-8")
        )
        self._boundary = json.loads(
            (fixture_dir / "boundary-discovery-snapshot.json").read_text(
                encoding="utf-8"
            )
        )
        schema = json.loads(
            (fixture_dir.parent / "acquisition.schema.json").read_text(encoding="utf-8")
        )
        self._request_validators = {
            kind: Draft202012Validator(
                {
                    "$schema": schema["$schema"],
                    "$defs": schema["$defs"],
                    "$ref": f"#/$defs/{definition}",
                }
            )
            for kind, definition in (
                ("boundary-jobs", "boundaryRequest"),
                ("acquisition-jobs", "acquisitionRequest"),
            )
        }
        self._retry_validator = Draft202012Validator(
            {
                "$schema": schema["$schema"],
                "$defs": schema["$defs"],
                "$ref": "#/$defs/retryRequest",
            }
        )
        self._jobs: dict[str, dict[str, Any]] = {}
        self._request_identities: dict[tuple[str, str], tuple[str, str]] = {}
        self._next_job = 1

    def handle(
        self,
        method: str,
        path: str,
        body: bytes | None = None,
        *,
        cursor: str | None = None,
    ) -> ServiceResponse:
        if method == "GET" and path == "/v1/health":
            return self._json(200, {"status": "ok", "contract_version": CONTRACT_VERSION})
        if method == "GET" and path == "/v1/capabilities":
            return self._json(
                200,
                {
                    "contract_versions": [CONTRACT_VERSION],
                    "supported_bundles": [self._acquisition["bundle"]],
                    "refresh_bundle_precedence": [self._acquisition["bundle"]["id"]],
                    "limits": {
                        "area_square_metres": 100_000_000,
                        "boundary_vertices": 10_000,
                        "tiles": 10_000,
                        "observations": 1_000_000,
                        "result_bytes": 1_000_000_000,
                        "concurrent_jobs": 2,
                    },
                    "retention_days": 30,
                    "quota_remaining": 100,
                },
            )
        if method == "POST" and path in (
            "/v1/boundary-jobs",
            "/v1/acquisition-jobs",
        ):
            try:
                request = json.loads(body or b"")
            except (json.JSONDecodeError, UnicodeDecodeError):
                request = None
            kind = path.rsplit("/", 1)[-1]
            if not self._valid_create_request(kind, request):
                return self._failure(400, "invalid_request", path, False)
            if request["bundle_id"] != self._acquisition["bundle"]["id"]:
                return self._failure(409, "unsupported_bundle", request["bundle_id"], False)
            if kind == "boundary-jobs":
                requested_area = math.pi * request["campus_target"]["search_radius_m"] ** 2
                if requested_area > 100_000_000:
                    return self._failure(
                        413,
                        "area_limit_exceeded",
                        "campus_target.search_radius_m",
                        False,
                        explanation=(
                            f"The requested boundary search covers {requested_area:.0f} m², "
                            "above the 100000000 m² limit."
                        ),
                        suggested_action="Reduce the bounded search radius and submit a new request.",
                    )
            identity = request["request_identity"]
            canonical_content = {
                key: value for key, value in request.items() if key != "request_identity"
            }
            content_sha256 = hashlib.sha256(
                json.dumps(
                    canonical_content,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            ).hexdigest()
            if identity["content_sha256"] != content_sha256:
                return self._failure(
                    400,
                    "content_digest_mismatch",
                    "request_identity.content_sha256",
                    False,
                )
            identity_key = (kind, identity["idempotency_key"])
            previous = self._request_identities.get(identity_key)
            if previous is not None:
                previous_hash, previous_job_id = previous
                if previous_hash != content_sha256:
                    return self._failure(
                        409,
                        "idempotency_conflict",
                        identity["idempotency_key"],
                        False,
                    )
                return self._job(202, previous_job_id)
            job_id = f"fixture-{self._next_job}"
            self._next_job += 1
            fixture = self._fixture(kind)
            outcomes = copy.deepcopy(fixture["coverage_report"]["outcomes"])
            self._jobs[job_id] = {
                "kind": kind,
                "state": (
                    "partial"
                    if kind == "acquisition-jobs"
                    and any(
                        outcome["status"] not in COMPLETE_STATUSES
                        for outcome in outcomes
                    )
                    else "complete"
                ),
                "outcomes": outcomes,
            }
            self._request_identities[identity_key] = (
                content_sha256,
                job_id,
            )
            return self._job(202, job_id)

        match = self._JOB_PATH.fullmatch(path)
        if not match:
            return self._failure(404, "route_not_found", path, False)
        job_id = match.group("job")
        job = self._jobs.get(job_id)
        if job is None or job["kind"] != match.group("kind"):
            return self._failure(404, "job_not_found", job_id, False)
        action = match.group("action")
        if action is None and method == "GET":
            return self._job(200, job_id)
        if action == "retry" and method == "POST":
            try:
                retry = json.loads(body or b"")
            except (json.JSONDecodeError, UnicodeDecodeError):
                retry = None
            if retry is None or not self._retry_validator.is_valid(retry):
                return self._failure(400, "invalid_retry", job_id, False)
            known_scopes = {
                (
                    outcome["provider"],
                    outcome["category"],
                    outcome["tile_id"],
                )
                for outcome in job["outcomes"]
            }
            requested_scopes = {
                (scope["provider"], scope["category"], scope["tile_id"])
                for scope in retry["scopes"]
            }
            if not requested_scopes.issubset(known_scopes):
                return self._failure(
                    409, "unknown_retry_scope", job_id, False
                )
            job["state"] = (
                "partial"
                if any(
                    outcome["status"] not in COMPLETE_STATUSES
                    for outcome in job["outcomes"]
                )
                else "complete"
            )
            return self._job(202, job_id)
        if action == "cancel" and method == "POST":
            job["state"] = "cancelled"
            return self._job(202, job_id)
        fixture = self._fixture(job["kind"])
        if action == "manifest" and method == "GET":
            return self._json(200, self._full_manifest(fixture))
        if action == "chunks" and method == "GET":
            chunk = fixture["manifest"]["chunks"][0]
            if match.group("chunk") != chunk["id"] or cursor != chunk["stable_cursor"]:
                return self._failure(416, "invalid_cursor", job_id, True)
            compressed = base64.b64decode(
                fixture["transport_chunks"][chunk["id"]], validate=True
            )
            return ServiceResponse(
                status=200,
                body=compressed,
                headers={
                    "content-type": "application/gzip",
                    "x-stable-cursor": chunk["stable_cursor"],
                    "digest": f"sha-256={chunk['sha256']}",
                },
            )
        return self._failure(405, "method_not_allowed", path, False)

    def _fixture(self, kind: str) -> Mapping[str, Any]:
        return self._boundary if kind == "boundary-jobs" else self._acquisition

    def _valid_create_request(self, kind: str, value: Any) -> bool:
        validator = self._request_validators.get(kind)
        return validator is not None and validator.is_valid(value)

    @staticmethod
    def _canonical_records(fixture: Mapping[str, Any]) -> bytes:
        key = "candidates" if "candidates" in fixture else "observations"
        return b"".join(
            (
                json.dumps(
                    record,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                )
                + "\n"
            ).encode("utf-8")
            for record in fixture[key]
        )

    @staticmethod
    def _full_manifest(fixture: Mapping[str, Any]) -> Mapping[str, Any]:
        records = fixture.get("candidates") or fixture.get("observations") or ()
        return {
            "contract_version": CONTRACT_VERSION,
            "bundle": fixture["bundle"],
            "coverage_report": fixture["coverage_report"],
            "licences": [record["licence"] for record in records],
            "chunks": fixture["manifest"]["chunks"],
            "result_sha256": fixture["manifest"]["result_sha256"],
        }

    def _job(self, status: int, job_id: str) -> ServiceResponse:
        job = self._jobs[job_id]
        return self._json(
            status,
            {
                "job_id": job_id,
                "contract_version": CONTRACT_VERSION,
                "bundle_id": self._acquisition["bundle"]["id"],
                "state": job["state"],
                "outcomes": job["outcomes"],
            },
        )

    @staticmethod
    def _json(status: int, value: Mapping[str, Any]) -> ServiceResponse:
        return ServiceResponse(
            status=status,
            body=json.dumps(
                value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
            ).encode("utf-8"),
            headers={"content-type": "application/json"},
        )

    def _failure(
        self,
        status: int,
        code: str,
        scope: str,
        retryable: bool,
        *,
        explanation: str | None = None,
        suggested_action: str | None = None,
    ) -> ServiceResponse:
        return self._json(
            status,
            {
                "code": code,
                "scope": scope,
                "retryable": retryable,
                "explanation": explanation or f"The fixture service rejected {scope}.",
                "suggested_action": suggested_action
                or (
                    "Retry the same pinned scope."
                    if retryable
                    else "Check the request and diagnostics."
                ),
            },
        )
