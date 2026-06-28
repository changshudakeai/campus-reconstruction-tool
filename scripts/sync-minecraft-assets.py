#!/usr/bin/env python3
"""Sync Minecraft Java block ids and representative textures from Mojang's official client jar."""
from __future__ import annotations
import hashlib, json, sys, tempfile, urllib.request, zipfile
from pathlib import Path

VERSION = sys.argv[1] if len(sys.argv) > 1 else "26.2"
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "public" / "minecraft" / VERSION
MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"

def read_json(url: str):
    with urllib.request.urlopen(url) as response:
        return json.load(response)

def first_model(state):
    if "variants" in state and state["variants"]:
        value = next(iter(state["variants"].values()))
        if isinstance(value, list): value = value[0] if value else {}
        return value.get("model") if isinstance(value, dict) else None
    for part in state.get("multipart", []):
        value = part.get("apply", {})
        if isinstance(value, list): value = value[0] if value else {}
        if isinstance(value, dict) and value.get("model"): return value["model"]
    return None

def asset_path(kind: str, identifier: str, suffix: str):
    namespace, sep, path = identifier.partition(":")
    if not sep: namespace, path = "minecraft", namespace
    return f"assets/{namespace}/{kind}/{path}{suffix}"

def load_model(jar, model_id, cache):
    if not model_id: return {}
    if model_id in cache: return cache[model_id]
    try: own = json.loads(jar.read(asset_path("models", model_id, ".json")))
    except KeyError: own = {}
    merged = {}
    parent = own.get("parent")
    if parent and parent != model_id: merged.update(load_model(jar, parent, cache))
    merged.update(own.get("textures", {}))
    cache[model_id] = merged
    return merged

def resolve_texture(textures):
    priorities = ("all", "top", "side", "end", "particle", "texture", "cross", "plant")
    value = next((textures[k] for k in priorities if k in textures), None)
    if value is None: value = next(iter(textures.values()), None)
    seen = set()
    while isinstance(value, str) and value.startswith("#") and value not in seen:
        seen.add(value); value = textures.get(value[1:])
    return value if isinstance(value, str) and not value.startswith("#") else None

def main():
    manifest = read_json(MANIFEST)
    entry = next((item for item in manifest["versions"] if item["id"] == VERSION), None)
    if not entry: raise SystemExit(f"Minecraft {VERSION} not found in Mojang manifest")
    version = read_json(entry["url"])
    client = version["downloads"]["client"]
    jar_path = Path(tempfile.gettempdir()) / f"minecraft-{VERSION}-client.jar"
    if not jar_path.exists() or jar_path.stat().st_size != client["size"]:
        urllib.request.urlretrieve(client["url"], jar_path)
    if hashlib.sha1(jar_path.read_bytes()).hexdigest() != client["sha1"]:
        raise SystemExit("Client jar SHA-1 mismatch")
    icons = OUT / "block-icons"; icons.mkdir(parents=True, exist_ok=True)
    catalog = []
    with zipfile.ZipFile(jar_path) as jar:
        states = sorted(name for name in jar.namelist() if name.startswith("assets/minecraft/blockstates/") and name.endswith(".json"))
        cache = {}
        for state_path in states:
            block = Path(state_path).stem
            state = json.loads(jar.read(state_path))
            texture = resolve_texture(load_model(jar, first_model(state), cache))
            icon_url = None
            if texture:
                texture_path = asset_path("textures", texture, ".png")
                try:
                    icon_file = icons / f"{block}.png"
                    icon_file.write_bytes(jar.read(texture_path))
                    icon_url = f"/minecraft/{VERSION}/block-icons/{block}.png"
                except KeyError: pass
            catalog.append({"id": f"minecraft:{block}", "label": block.replace("_", " "), "icon": icon_url})
    payload = {"version": VERSION, "source": client["url"], "sha1": client["sha1"], "count": len(catalog), "blocks": catalog}
    (OUT / "blocks.json").write_text(json.dumps(payload, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    print(f"Synced {len(catalog)} blocks; {sum(1 for item in catalog if item['icon'])} icons -> {OUT}")

if __name__ == "__main__": main()