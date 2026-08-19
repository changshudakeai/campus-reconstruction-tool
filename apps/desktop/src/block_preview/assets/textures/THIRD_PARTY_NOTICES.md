# Third-party notices — preview textures

T52 第五步 3D 方块预览使用社区纹理包 **Pixel Perfection Legacy** 的方块贴图
（仅 16×16 基础纹理，用于渲染图集 `atlas.png`）。

## 来源与版本

- 项目：Pixel Perfection Legacy
- Modrinth：<https://modrinth.com/resourcepack/pixel-perfection-legacy>
- 版本文件：`Pixel Perfection 26.1-84.1.zip`（Modrinth version `ry4VI4eZ`）
- 提取来源：`assets/minecraft/textures/block/*.png`

## 许可证

Modrinth 项目元数据（2026-08-19 查询）：

```json
{
  "title": "Pixel Perfection Legacy",
  "slug": "pixel-perfection-legacy",
  "license": {
    "id": "CC-BY-4.0",
    "name": "Creative Commons Attribution 4.0 International",
    "url": "https://creativecommons.org/licenses/by-sa/4.0/"
  }
}
```

注意：Modrinth 的 `license.id` 为 **CC-BY-4.0**，但其 `license.url` 链接指向
CC BY-SA 4.0 页面。项目包内 `pack.txt` 未附许可证文本，仅注明原包作者
XSSheep 与续作整理者 Nova_Wostra。原版 Pixel Perfection 在社区资料中通常以
CC BY-SA 4.0 发布。为谨慎起见，本文件同时保留两份署名与上述链接歧义；
使用方应自行确认最终合规口径。

## 署名与作者声明（pack.txt 原文）

```text
XSSheep's Pixel Perfection.
Pure pixely goodness.
(https://www.minecraftforum.net/members/XSSheep)

Edited by Nova_Wostra to continiue use for 1.13 - 25.1
(https://www.minecraftforum.net/members/Nova_Wostra)

Using textures (for 1.14 and 1.13) and code from:
freejusticehere (https://www.minecraftforum.net/members/freejusticehere)

Inspiration for Emissive Netherplants from Skaliber and his Caelesti Resourcepack

Contribution by HexaBlu (Jigsaw blocks, spectator_widgets, stats_icons,
checkbox, toast, achivements, Ancient Debriss)

Emissive shader code by shock_micro on discord
NEW emissive shader code by i_am_merp on discord or https://linktr.ee/i_am_merp
```

包内还包含 Minecraft 官方 `credits.txt`（游戏制作人员名单）与 Classic Mobs /
Old Paintings 等附加文件；本项目未使用上述附加内容。

## 使用范围

本工具只从纹理包提取下列方块贴图，缩小/保持为 16×16，打包成
`atlas.png`（512×512，nearest 采样）供 WebView 内 Three.js 渲染：

- 建筑：stone_bricks、bricks、glass/glass_pane、oak_planks、dark_oak_planks、
  dark_oak_door、smooth_sandstone、stone、stone_slab、smooth_stone
- 道路：smooth_stone
- 水面：water_still
- 植被：oak_log（顶/侧）、oak_leaves、grass_block（顶/侧）、dirt
- 体育：red_concrete、white_concrete、light_gray_concrete
- 其他：rail
- 兜底：stone、spruce_planks、spruce_door

原版资源包不随本仓库分发（未提交 34MB zip）；贴图提取与图集生成方式见
`tools/build_texture_atlas.py`。
