# ADR-0038：V2 不提供高德离线地图

## 状态

已接受（2026-07-30，产品负责人确认）。

## 背景

当前 Windows 桌面程序通过 WebView 使用高德 JS 地图。高德官方说明 JS API 只支持在线加载；官方离线地图能力面向 Android、iOS 等特定原生 SDK。现有 Web 端 Key 或 Web 服务 Key 不授予自行下载、抓取、存储或缓存高德地图内容的权限，高德开放平台服务协议也明确限制这些行为。

参考：

- https://lbs.amap.com/product/mapstyle/m
- https://lbs.amap.com/api/android-sdk/guide/create-map/offline-map/?type=spa
- https://lbs.amap.com/pages/terms/

## 决定

V2 当前版本继续使用在线高德地图完成校区搜索、地址显示和边界绘制，不提供高德离线地图下载，不抓取或缓存高德地图瓦片，也不把现有 Web 端 Key 或 Web 服务 Key 当作离线授权。

如果未来需要离线地图，单独研究基于 OSM 数据的合法本地离线地图包，并分别评估底图、离线搜索索引、地址解析、数据更新、安装包体积和数据许可。该研究不得改变当前 V2 的实施范围。
