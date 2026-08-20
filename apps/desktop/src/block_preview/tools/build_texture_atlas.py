#!/usr/bin/env python3
"""Build the preview texture atlas from the Pixel Perfection Legacy pack.

T52 预览纹理资产生成工具（开发期一次性/可复现工具，不参与构建）：

1. 从本机解包后的 Pixel Perfection Legacy 目录读取 16x16 方块贴图；
2. 按“方块 ID + 面”映射裁剪成一张 512x512 的图集 PNG（无 mipmap，
   nearest 采样，像素中心 UV，避免出血）；
3. 输出 texture_map.json（方块 ID -> {top,bottom,side} -> [col,row]）。

运行：
    python apps/desktop/src/block_preview/tools/build_texture_atlas.py ^
        C:/path/to/unpacked/pp/assets/minecraft/textures/block ^
        apps/desktop/src/block_preview/assets/textures

源素材来自 Pixel Perfection Legacy（CC BY 4.0 / 原包 CC BY-SA 4.0），
第三方声明见 assets/textures/THIRD_PARTY_NOTICES.md。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from PIL import Image


# 方块 ID -> 各面使用的源贴图文件（不含扩展名）。
# "all" 表示六个面同图；显式 top/bottom/side 分别指定。
BLOCK_FACES: dict[str, dict[str, str]] = {
    "minecraft:stone_bricks": {"all": "stone_bricks"},
    "minecraft:bricks": {"all": "bricks"},
    "minecraft:glass_pane": {
        "top": "glass_pane_top",
        "bottom": "glass_pane_top",
        "side": "glass",
    },
    "minecraft:glass": {"all": "glass"},
    "minecraft:oak_planks": {"all": "oak_planks"},
    "minecraft:dark_oak_slab": {"all": "dark_oak_planks"},
    "minecraft:dark_oak_door": {
        "top": "dark_oak_door_top",
        "bottom": "dark_oak_door_top",
        "side": "dark_oak_door_bottom",
    },
    "minecraft:smooth_sandstone": {"all": "smooth_sandstone"},
    "minecraft:smooth_stone": {"all": "smooth_stone"},
    "minecraft:stone": {"all": "stone"},
    "minecraft:stone_slab": {
        "top": "stone_slab_top",
        "bottom": "stone_slab_top",
        "side": "stone_slab_side",
    },
    "minecraft:spruce_planks": {"all": "spruce_planks"},
    "minecraft:spruce_door": {
        "top": "spruce_door_top",
        "bottom": "spruce_door_top",
        "side": "spruce_door_bottom",
    },
    "minecraft:water": {"all": "water_still"},
    "minecraft:oak_log": {
        "top": "oak_log_top",
        "bottom": "oak_log_top",
        "side": "oak_log",
    },
    "minecraft:oak_fence": {
        "top": "oak_fence_top",
        "bottom": "oak_fence_top",
        "side": "oak_fence",
    },
    "minecraft:oak_leaves": {"all": "oak_leaves"},
    "minecraft:grass_block": {
        "top": "grass_block_top",
        "bottom": "dirt",
        "side": "grass_block_side",
    },
    "minecraft:dirt": {"all": "dirt"},
    "minecraft:red_concrete": {"all": "red_concrete"},
    "minecraft:white_concrete": {"all": "white_concrete"},
    "minecraft:light_gray_concrete": {"all": "light_gray_concrete"},
    "minecraft:rail": {"all": "rail"},
}

TILE = 16
GRID = 32  # 32x32 个 16x16 图块 -> 512x512 图集


def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)
    source_dir = Path(sys.argv[1])
    out_dir = Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)

    atlas = Image.new("RGBA", (GRID * TILE, GRID * TILE), (0, 0, 0, 0))
    texture_map: dict[str, dict[str, list[int]]] = {}
    slot = 0
    missing: list[str] = []

    for block_id, faces in BLOCK_FACES.items():
        entries = faces if "all" not in faces else {"top": "all", "bottom": "all", "side": "all"}
        face_map: dict[str, list[int]] = {}
        for face, source in entries.items():
            if source == "all":
                source_name = faces["all"]
            else:
                source_name = source
            source_path = source_dir / f"{source_name}.png"
            if not source_path.exists():
                missing.append(f"{block_id}/{face}: {source_name}.png")
                continue
            with Image.open(source_path) as tile:
                tile = tile.convert("RGBA").resize((TILE, TILE), Image.NEAREST)
            col = slot % GRID
            row = slot // GRID
            atlas.paste(tile, (col * TILE, row * TILE))
            face_map[face] = [col, row]
            slot += 1
        if face_map:
            texture_map[block_id] = face_map

    atlas_path = out_dir / "atlas.png"
    atlas.save(atlas_path)
    map_path = out_dir / "texture_map.json"
    map_path.write_text(
        json.dumps(texture_map, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"atlas: {atlas_path} ({atlas.size[0]}x{atlas.size[1]}, {slot} tiles)")
    print(f"map:   {map_path} ({len(texture_map)} blocks)")
    if missing:
        print("missing:", "\n".join(missing), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
