# 端到端剧本 A：首次设置 → 校区 → 方案 → 边界确认 → 基础导出

状态：M5 实施窗口准备（2026-08-06）。自动化等价证据：`cargo test -p
desktop-shell --test s1_08_boundary_export_flow`（同一 UI 回调链路写出真实
`.schem` + `.foundation_manifest.json`）；本剧本用于产品负责人现场人工验收。

## 前置

- 发布产物：`dist/MCRebuild-V2.0.0-portable.zip`（解压后运行
  `campus-rebuild.exe`），或桌面快捷方式"校园复刻工具 - 开发版"。
- 干净验收环境：删除 `%LOCALAPPDATA%\MCRebuildV2\dev\campus-rebuild.db`
  与导出目录，保证从首次设置开始。

## 步骤与预期

| 步骤 | 操作 | 预期可见反馈 |
|------|------|--------------|
| 1 首次设置 | 首开向导：语言 zh-CN、游戏版本 26.1.2、勾选已知悉后继续 | 进入校区选择页 |
| 2 选择校区 | 校区搜索框输入真实校区名（如"上海交通大学"）→ 回车搜索 → 点选结果 | 校区出现在"最近使用的校区"；进入方案列表 |
| 3 创建方案 | 点"新建方案"，输入方案名确认 | 方案卡片出现（进度：未确认边界） |
| 4 确认边界 | 打开方案 → 地图页自动从 OSM 获取边界（T31：Rust 侧直连；无数据/失败才人工圈画）→ 如需可拖动调整 → 点"确认边界" | 步骤①完成，导出入口可用 |
| 5 基础导出 | 直接进入导出页 → 确认导出（不设朝向、不采集） | 提示"导出完成"；输出位置出现两个文件 |

## 证据采集

```powershell
# 记录实际文件、尺寸与 manifest 内容
$out = '<导出目录>'
Get-ChildItem $out -Recurse | Select-Object Name, Length | Out-File e2e-a-files.txt
Get-Content (Join-Path $out '<plan_id>.foundation_manifest.json') -Encoding UTF8 | Out-File e2e-a-manifest.json
# 截图：导出完成提示页
```

## 剧本 A 验收点

- [ ] `.schem` 文件存在且尺寸 > 0
- [ ] `*.foundation_manifest.json` 存在且 `exportKind == "base"`、候选事实为空
- [ ] 未设置朝向时 manifest 朝向来源为地图正北（MapNorth）
- [ ] 负责人现场确认（截图 + 签名/日期）
