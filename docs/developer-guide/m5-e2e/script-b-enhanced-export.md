# 端到端剧本 B：真实高德在线链路 → 候选采集 → 评审封账 → 增强导出

状态：M5 实施窗口准备（2026-08-06）；T31（2026-08-07）候选采集源切 OSM/Overpass。
真实在线链路为 M5 强制人工环节：需产品负责人提供已授权的 Web 端(JS API) 密钥
（地图）与 Web 服务 Key（regeo 补名，可留空），密钥只经设置页录入。

## 前置

- 高德 Web 端(JS API) 密钥 + securityJsCode，且已把应用 WebView 来源域名
  加入高德控制台白名单；密钥经设置页保存（代码不落盘明文日志）。
- 发布产物同剧本 A。

## 步骤与预期

| 步骤 | 操作 | 预期可见反馈 |
|------|------|--------------|
| 1 首次设置 | 同剧本 A 步骤 1-3 | 校区选择/方案列表 |
| 2 确认边界 | 打开方案 → 地图自动从 OSM 获取边界（T31：Nominatim→Overpass，来源标注 © OpenStreetMap contributors）→ 确认 | 步骤①完成 |
| 3 候选采集 | 进入采集页 → 点"采集"（OSM/Overpass：union building=* 面几何；缺名建筑 regeo 补名，需 Web 服务 Key） | 进度逐类别推进；完成后显示采集报告（原始/可评审/修复/隔离计数） |
| 4 评审保留 | 进入评审工作台 → 逐项保留/剔除 → 批量确认 | 六类分组展示；保留计数正确 |
| 5 封账 | 点"封账完成评审"并确认 | 评审终态写回；导出页显示增强摘要 |
| 6 增强导出 | 导出页 → 确认导出 | manifest 为 `exportKind == "enhanced"`，含保留候选生成内容 |

## 证据采集

```powershell
Get-ChildItem $out -Recurse | Select-Object Name, Length | Out-File e2e-b-files.txt
Get-Content (Join-Path $out '<plan_id>.foundation_manifest.json') -Encoding UTF8 | Out-File e2e-b-manifest.json
# 截图：地图加载成功（真实高德瓦片）、采集报告、评审台、导出完成页
```

## 剧本 B 验收点

- [ ] 地图真实加载高德在线瓦片并完成校区搜索/边界获取（截图）
- [ ] 采集报告计数真实（>0 原始观测；可评审/隔离与报告一致）
- [ ] 评审三态与封账生效；导出摘要保留/待定/剔除计数正确
- [ ] `.schem` + manifest：`exportKind == "enhanced"`、`keepByCategory` 与保留候选一致
- [ ] 负责人现场确认（截图 + 签名/日期）
