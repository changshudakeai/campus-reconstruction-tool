# 候选建筑与校区边界数据源调研（含坐标转换与命名）

> 调研日期：2026-08-07
> 调研人：独立验收窗口（验收职责，仅调研，不改代码）
> 范围：M5 收口遗留问题——真实校区边界自动获取、候选建筑轮廓来源、坐标转换、候选命名。
> 方法：一手官方文档 + 本机（上海网络）实测 Overpass/Nominatim/Overture 端点；代码事实引用当前 `main`。

## 一、执行摘要（给负责人）

1. **候选数据源正解是 OSM/Overpass，高德只当地图、身份和命名**。高德没有免费的通用建筑轮廓数据：POI 只有点，AOI 面数据属付费/商务产品且面向小区商圈等区域，不是建筑足迹。这与 ADR-0013/0004 的规划一致。
2. **“Overpass 不可用”的根因已被实测找到**：应用发的查询有三个硬伤——请求 URL 少了 `data=` 参数（服务器必然报错）、正则里的 `|` 在新版 Overpass（0.7.62.11，overpass-api.de）上被解析器拒绝、overpass-api.de 与 kumi 不给浏览器 CORS 头（WebView 里即便查询对了也会被 CORS 拦截）。端点本身在上海网络是可用的。
3. **OSM 数据真实存在且够用**：实测上海交大闵行校区 1km 范围有 277 个 `building=*` 面；Nominatim 可解析出“上海交通大学（徐汇/七宝校区）”等具名校区。建筑名称优先用 OSM 自带 `name` 标签，缺名时用高德逆地理编码补（个人配额 5000 次/日，一所学校几百栋建筑完全够用）。
4. **推荐实现**：候选采集与边界获取统一走 Overpass（Rust 侧直连，绕开 CORS；union 查询替代 `|` 正则；de → kumi → mail.ru 多端点回退）；Overture 作为离线补充包；坐标以 GCJ-02 为应用工作坐标系（与已存边界一致），OSM 的 WGS-84 数据在入口转换。
5. **必须合规**：OSM/Overture 建筑数据是 ODbL 许可，导出物与界面需要署名 © OpenStreetMap contributors 的义务，商用工具尤其要注意。

## 二、结论速览

| 用途 | 首选数据源 | 备选 | 关键理由 |
|---|---|---|---|
| 校区搜索与身份 | 高德 PlaceSearch（现状保留） | Nominatim | 高德中文地名全；身份锚点已入库 |
| 校区边界自动获取 | OSM Overpass（关系/面的 `out geom`） | Nominatim 解析校名→OSM 元素 ID → Overpass 按 ID 拉取 | OSM 有大学面数据；实测可行 |
| 候选建筑轮廓 | OSM Overpass `building=*`（WGS-84 面） | Overture buildings（Azure 可达） | 实测 277 面/交大闵行 1km；OSM 有名字与层高标签 |
| 候选命名 | OSM `name` 标签优先 | 高德 regeo 补名（关键建筑） | OSM 中国建筑名覆盖率低；regeo 个人 5000 次/日 |
| 地图显示/编辑 | 高德 JS API（现状保留） | 天地图 | 产品基线既定；GCJ-02 显示 |
| 坐标转换 | 高德 `AMap.convertFrom`（官方） | 开源 transform（批处理、精度约 1m） | 官方权威但 40 点/次；开源可离线批转 |

## 三、问题一：高德能提供什么数据？免费与收费边界

### 3.1 POI 只有点，没有建筑轮廓

- 高德 POI 搜索（PlaceSearch/searchNearBy）返回的是**点要素**（名称、坐标、类型），没有建筑多边形。官方文档的 POI 搜索教程只演示名称/位置/联系方式等字段，无面几何（[高德官方：搜索地点](https://lbs.amap.com/api/javascript-api-v2/tutorails/search-poi)；[输入提示与 POI 搜索](https://developer.amap.com/api/javascript-api-v2/guide/services/autocomplete)）。
- 高德提供“AOI 边界查询”类接口（第三方 API 目录可检索到，[Apifox 镜像：AOI边界查询](http://apifox.com/apidoc/docs-site/759159/api-14639804)，非官方）；但官方对搜索类接口做过配额合并下调（[高德官方：搜索接口配额调整](https://lbs.amap.com/news/search-interface-adjustment2/)），且 AOI 面向住宅区/商圈等区域而非通用建筑足迹。**结论：高德不提供免费的通用建筑轮廓，商业数据需商务购买**（[高德官方：服务升级与定价](https://lbs.amap.com/upgrade#price)，付费档位 5 万/年起）。

### 3.2 官方配额（Web 服务，个人/企业）

| 能力 | 个人日配额 | 企业日配额 |
|---|---|---|
| 逆地理编码 | 5000 次 | 3000000 次 |
| 地理编码 | 5000 次 | 3000000 次 |
| 坐标转换 | 5000 次 | 3000000 次 |
| 关键字/周边/多边形/ID 搜索 | 100 次 | 1000 次 |

来源：[高德官方 FAQ：企业/个人认证开发者区别](https://developer.amap.com/faq/account/certification/39670)。注意搜索类配额很低；regeo 配额高，足够做建筑补名。JS API（Web 端 Key）的调用量官方未公布固定日配额（未证实；第三方文章称“不限次数”，[51cto 文章](https://blog.51cto.com/u_16099238/14794013)，非官方，仅供参考）。

## 四、问题二：OSM/Overpass 实测现状（本机 2026-08-07，上海网络）

### 4.1 端点可达性与 CORS 实测

| 端点 | /api/status | 数据查询 | CORS 头 |
|---|---|---|---|
| overpass-api.de | OK 200，931ms | 可用但负载高，实测多次 504/超时 | 无 ACAO |
| overpass.kumi.systems | OK 200，9816ms（慢） | 一次成功(891ms)，一次 504 | 无 ACAO |
| maps.mail.ru（俄罗斯镜像） | OK 200，1932ms | 多次成功，个别复杂查询 504 | `Access-Control-Allow-Origin: *` |
| overpass.nchc.org.tw（台湾） | 连接失败 | - | - |
| overpass.osm.jp（日本） | TLS 握手失败 | - | - |
| overpass.openstreetmap.ru | 超时 | - | - |

实测结论：**端点从上海网络可达**（de 最快），但 de/kumi 无 CORS 头、负载下不稳定；mail.ru 有 CORS 且较稳定。**应用在 WebView 里用 fetch 打 de/kumi，即使查询正确也会被 CORS 拦截；从 Rust 侧直连则无此问题。**

### 4.2 应用当前查询的三个硬伤（已定位，代码事实）

当前边界查询在 `core/gaode-client/src/boundary_edit_map_page.rs`：

```js
return '[out:json][timeout:45];' +
  '(way["amenity"~"university|college|school"](' + ... + ');' +
  'relation["amenity"~"university|college|school"](' + ... + '););out geom;';
// :411 处 fetch: 'https://overpass-api.de/api/interpreter?' + encodeURIComponent(query)
```

1. **缺 `data=` 参数**：URL 是 `interpreter?<编码后的查询>`，服务器把编码串当参数名，实测返回 `parse error: Unknown type "%"`（HTTP 200 的错误页）。正确写法是 `interpreter?data=<编码后的查询>`。
2. **`|` 正则被新版解析器拒绝**：overpass-api.de（0.7.62.11）实测 `way["amenity"~"university|college|school"]` 报 `parse error: ',' or ']' expected - '|' found`；而 mail.ru（0.7.62.4）接受同一写法。**版本相关，不能依赖**；替代写法用 union（`(way["amenity"="university"];way["amenity"="college"];way["amenity"="school"];)`）或转义。
3. **de/kumi 无 CORS 头**：WebView 内 fetch 会被浏览器同源策略拦截；只有 mail.ru 返回 `Access-Control-Allow-Origin: *`。

这就是 T30 真实验收中“OSM 边界查询失败、退回手动圈画”的直接原因——不是 OSM 没数据，是请求根本没到服务器正确执行。

### 4.3 OSM 数据实测（上海交大闵行 1km 范围）

- `building=*`：**277 个面**（mail.ru 计数）。
- `landuse=education` / `amenity=university` 关系：该 bbox 内未见明确校区面（实测 relation amenity=university 为空，way 仅 2 个）；徐汇/七宝校区通过 Nominatim 可解析到 `amenity=university` 的 way（[Nominatim 实测](https://nominatim.openstreetmap.org/search?q=Shanghai+Jiao+Tong+University&format=json)）。**结论：校区边界不能只靠“1km 内 amenity 正则”，需要按校名解析（Nominatim）→ 元素 ID → 拉取，再回退 landuse=education / 手动圈画。**
- 带 `name` 的建筑：覆盖稀疏（需再实测），印证“OSM 命名优先 + regeo 补名”的混合策略。

### 4.4 Nominatim（OSM 官方地理编码）

- 实测可达（200，约 1.7s），可返回具名校区的 `osm_type/osm_id` 与 boundingbox（上海交大徐汇 way 144183801、七宝 way 538416377）。
- 无 CORS 头（实测响应头无 ACAO）→ 适合 Rust 侧调用。
- 使用政策：**每个应用最多 1 次/秒**，需带有效 User-Agent（[OSMF 官方政策](https://operations.osmfoundation.org/policies/nominatim/)）。只用于“校名→校区元素”的一次性解析，完全够用。

## 五、问题三：Overture Maps

- **覆盖**：官方称建筑主题全球覆盖，数据源为 OSM（最高优先级）+ Esri 社区图 + 国家级数据 + 机器学习足迹（Microsoft、Google Open Buildings、以及一个**东亚国家 ML 数据集**——2025-01-22 发布说明称新增 2.285 亿东亚建筑，[官方发布说明](https://docs.overturemaps.org/blog/2025/01/22/release-notes/)；中国覆盖由此数据与 OSM 构成）。
- **许可**：建筑主题**整体是 ODbL**（因含 OSM 数据），不是 CDLA（[官方 Buildings 指南](https://docs.overturemaps.org/guides/buildings/)）。
- **访问**：月更 GeoParquet，公开在 AWS S3 与 Azure Blob；**无公开实时查询 API**，需要下载分区文件后用 DuckDB 等本地查询（[官方指南](https://docs.overturemaps.org/guides/buildings/)、[获取数据](https://docs.overturemaps.org/getting-data/)）。实测 Azure 端点从上海可达（HEAD 200）；S3 目录路径返回 404（该路径不提供目录列举，不代表数据不可用）。
- **适用性**：适合做“离线补充包”（下载区域切片、本地查询、补 OSM 缺漏），不适合当实时小查询接口。建筑主题**不带名称**（名称在 Places 主题），用于命名仍需 OSM/regeo。

## 六、问题四：其他可用数据源对比

| 数据源 | 几何 | 中国覆盖 | 许可 | 访问方式 | 实用性评估 |
|---|---|---|---|---|---|
| OSM/Overpass | 点线面 + 标签 | 大学/主要建筑较好，实测交大 277 面 | ODbL | 公共 API，无需 Key | 首选：实时、带 name/levels |
| Overture Buildings | 面 + 高度 | 有（OSM+东亚 ML） | ODbL（整体） | S3/Azure 月更 GeoParquet | 覆盖广但重、无名称 |
| Microsoft GlobalMLBuildingFootprints | 面 | 全球 130 国；中国覆盖未证实（Maxar 影像受限） | ODbL | 下载 | 大而全，需自行切片 |
| Google Open Buildings | 面 | **不含中国大陆**（非洲/南亚/东南亚/拉美/加勒比） | 开放许可（官方站点标注 CC BY；部分渠道标注 ODbL） | 下载 | 中国不可用 |
| 高德 AOI | 面（区域级） | 好 | 商业付费 | 商务采购 | 非建筑级、付费 |
| 天地图 | 底图/地名/矢量图层 | 好 | 免费注册+开发许可；商用需授权 | JS API/数据服务 | 只当地图备选，无公开建筑面 API |
| 百度地图 AOI | 面（区域级） | 好 | 商业付费 | 商务采购 | 同上，且坐标系 GCJ 系 |

来源：OSM Overpass（[OSM Wiki](https://wiki.openstreetmap.org/wiki/Overpass_API)）、Overture（[Buildings 指南](https://docs.overturemaps.org/guides/buildings/)）、Microsoft（[GlobalMLBuildingFootprints README](https://github.com/microsoft/GlobalMLBuildingFootprints)）、Google Open Buildings（[官方站点](https://sites.research.google/gr/open-buildings/)）、高德（[配额调整](https://lbs.amap.com/news/search-interface-adjustment2/)、[定价页](https://lbs.amap.com/upgrade#price)）、天地图（[JS API 指南](http://lbs.tianditu.gov.cn/api/js4.0/guide.html)、[开发许可说明](http://lbs.tianditu.gov.cn/authorization/authorization.html)）。

## 七、问题五：坐标转换

### 7.1 现状

- 仓库已有决策（ADR-0029 配套技术笔记 [gcj02-conversion-practice.md](./gcj02-conversion-practice.md)）：**JS 侧用高德官方 `AMap.convertFrom(lnglat,'gps')` 把 OSM 的 WGS-84 转 GCJ-02**，单次最多 40 点、分批；**应用工作坐标系是 GCJ-02**（边界已按 GCJ-02 入库，避免二次转换误差）。
- 高德官方不提供 GCJ-02 → WGS-84 的反向转换接口（`convertFrom` 只支持 gps/baidu/mapbar → GCJ）。

### 7.2 建议

1. **保持 GCJ-02 为应用工作坐标系**（与已入库边界一致，B5 投影/包含判定/导出换算都在同一坐标系，避免混用）。
2. **采集侧**：Overpass 返回 WGS-84 多边形 → 在入口转换为 GCJ-02 后落库（建筑面点数多，可用 JS `convertFrom` 分批，或 Rust 侧开源 transform 批转，精度约 1 米，校区尺度误差可忽略；开源实现非官方，属常规做法，注意测绘合规说明）。
3. **保留原始 WGS-84 字段**（如 `source_crs`/原始几何），便于将来换显示底图或与 OSM 增量对比。
4. 导出到 Minecraft 的平面换算继续用等距圆柱近似（现状），不引入投影库。

## 八、问题六：候选命名

- **OSM `name` 标签优先**：建筑面自带中文/英文名，零成本、与几何同源（实测中国建筑 name 覆盖率低，作为第一来源而非唯一来源）。
- **高德逆地理编码（regeo）补名**：按建筑中心坐标反查最近 POI/地址取名。配额：个人 5000 次/日、企业 300 万/日（[官方 FAQ](https://developer.amap.com/faq/account/certification/39670)）。一所校园 200–500 栋建筑，**个人配额完全够用且免费**；超量才按商用计费。
- 实现注意：regeo 需要 **Web 服务 Key**（与现有 JS API Key 不同）；若不想新增 Key，可用 JS API 的 `AMap.Geocoder.getAddress` 在 WebView 内逐个反查（[官方文档](https://lbs.amap.com/api/javascript-api-v2/guide/services/geocoder)），但批量效率低。**建议：只对无 name 的关键建筑（教学楼/图书馆/宿舍等）补名，结果缓存入库，不重复调用。**
- 候选名称直接参与评审列表展示与导出 manifest，命名质量影响用户体验，但**不阻塞导出**（无名建筑可显示“未命名建筑 #id”）。

## 九、推荐实现方案（效果最好 + 实现工作量合理）

### 9.1 主方案：Overpass 直连（Rust 侧）+ Nominatim 校名解析 + OSM 建筑面 + regeo 补名

1. **校区边界**：候选校名 → Nominatim（1 次/秒限制）→ 得到 OSM 元素 ID → Overpass 按 ID 拉 `out geom`；失败回退 Overpass 的 landuse=education/大学面查询；再失败才手动圈画（维持 ADR-0029 顺序）。
2. **候选采集**：Overpass 查校区边界内的 `way["building"]`（**union 写法，避免 `|` 正则**），返回面几何 + `name`/`building:levels` 标签 → WGS-84 → GCJ-02 转换 → 落库为面候选。端点按 de → kumi → mail.ru 顺序回退，Rust 侧 reqwest 直连（**无 CORS 问题**），每端点超时 10–15s，带重试。
3. **命名**：OSM `name` 优先；无名的关键建筑用 regeo（Web 服务 Key）补名并缓存。
4. **Overture**：作为可选“离线补充包”——把校区所在 10°×10° 分区的 buildings parquet 预下载（Azure 可达），本地 DuckDB 查询补 OSM 缺漏；第一版可不做。
5. **高德职责**：只保留地图显示、校区搜索身份、`convertFrom` 坐标转换、regeo 命名。

### 9.2 备选方案

- **A（零改造 WebView JS）**：只修三处硬伤（补 `data=`、`|` 换 union、端点换成 mail.ru 或 Rust 桥），改动最小但端点不稳定、CORS 依赖第三方，不推荐作为最终形态。
- **B（全部 Overture）**：覆盖最好但下载/切片/本地查询工程量大，命名还要另找来源，适合后续版本做“精细模式”。

## 十、风险与合规

1. **ODbL 署名与共享义务**：OSM 数据（含经 Overture 合并的）要求署名 © OpenStreetMap contributors；导出物若内嵌 OSM 建筑数据，发布需遵守 ODbL（共享一致、保留署名）。详见 [ODbL](https://opendatacommons.org/licenses/odbl/1-0/)。
2. **公共 Overpass 限流**：官方建议日常约 1 万查询/日、1GB 下载（[官方手册](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html)）；多用户部署应自建实例。本工具单用户低频使用没问题。
3. **Nominatim 政策**：1 次/秒，必须带 User-Agent（[政策原文](https://operations.osmfoundation.org/policies/nominatim/)）。
4. **高德 ToS 与配额**：搜索类个人配额低（100 次/日）；regeo 5000 次/日；商用需企业认证。AOI/面数据需商务购买。
5. **天地图商用授权**：商用场景需单独申请授权（[开发许可说明](http://lbs.tianditu.gov.cn/authorization/authorization.html)）。
6. **GCJ-02 转换合规**：官方转换在 JS 侧；开源转换属常见实践，精度约 1m，正式商用前建议法务评估。

## 十一、引用清单

### 高德官方

- [搜索地点（PlaceSearch）](https://lbs.amap.com/api/javascript-api-v2/tutorails/search-poi)
- [输入提示与 POI 搜索](https://developer.amap.com/api/javascript-api-v2/guide/services/autocomplete)
- [地理编码/逆地理编码（JS API）](https://lbs.amap.com/api/javascript-api-v2/guide/services/geocoder)
- [坐标转换 convertFrom](https://lbs.amap.com/api/javascript-api-v2/guide/transform/convertfrom)
- [个人/企业认证配额](https://developer.amap.com/faq/account/certification/39670)
- [搜索接口配额调整](https://lbs.amap.com/news/search-interface-adjustment2/)
- [服务升级与定价](https://lbs.amap.com/upgrade#price)

### OSM/Overpass/Nominatim 官方

- [Overpass API（OSM Wiki）](https://wiki.openstreetmap.org/wiki/Overpass_API)
- [Overpass 公共资源与限流（官方手册）](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html)
- [Nominatim 使用政策（OSMF）](https://operations.osmfoundation.org/policies/nominatim/)
- [ODbL 许可](https://opendatacommons.org/licenses/odbl/1-0/)

### Overture / 其他

- [Overture Buildings 指南](https://docs.overturemaps.org/guides/buildings/)
- [Overture 获取数据](https://docs.overturemaps.org/getting-data/)
- [Overture 2025-01-22 发布说明（东亚 ML 建筑）](https://docs.overturemaps.org/blog/2025/01/22/release-notes/)
- [Microsoft GlobalMLBuildingFootprints](https://github.com/microsoft/GlobalMLBuildingFootprints)
- [Google Open Buildings（官方站点）](https://sites.research.google/gr/open-buildings/)
- [天地图 JS API 指南](http://lbs.tianditu.gov.cn/api/js4.0/guide.html)
- [天地图开发许可说明](http://lbs.tianditu.gov.cn/authorization/authorization.html)

## 十二、附录：本机实测记录（2026-08-07，上海网络）

| 测试 | 结果 |
|---|---|
| overpass-api.de/status | 200，931ms |
| overpass.kumi.systems/status | 200，9816ms |
| maps.mail.ru/.../status | 200，1932ms |
| overpass-api.de `building=*` count（交大闵行 bbox） | 首次超时 25s；`?data=` 写法 200 |
| kumi `building=*` count | 首次 200（891ms，277 面），二次 504 |
| mail.ru `building=*` count | 200（19.6s，277 面）；复杂查询偶发 504 |
| de/kumi CORS | 无 `Access-Control-Allow-Origin` |
| mail.ru CORS | `Access-Control-Allow-Origin: *` |
| 应用原 URL（缺 data=） | 200 错误页 `parse error: Unknown type "%"` |
| de `|` 正则（0.7.62.11） | 400 `parse error: ',' or ']' expected - '|' found` |
| mail.ru `|` 正则（0.7.62.4） | 200 正常 |
| de union 查询 | 200 正常（计数查询）；`out body geom` 时 504（负载高） |
| Nominatim 上海交大 | 200，约 1.7s，返回徐汇 way 144183801 / 七宝 way 538416377；无 CORS 头 |
| Overture Azure（目录 HEAD） | 200 |
| Overture S3（目录 HEAD） | 404（不提供目录列举） |

### 代码事实索引（当前 main）

- 边界查询生成：`core/gaode-client/src/boundary_edit_map_page.rs:437-447`（`|` 正则 + `out geom`）
- 查询 fetch URL（缺 data=）：同文件 `:411`、`:421`
- Overpass 适配器（存在、未接线）：`core/data-acquisition/src/source.rs:119-150`（`OverpassDataSource`，经 BridgeTransport 解析 out geom JSON，保留 node/way 面几何）
- 生产候选采集仍走高德：`apps/desktop/src/runtime.rs:550-562`（`GaodeDataSource::new` 注入 WebView JS 桥）

> 未证实项：高德 JS API 的公开日配额（官方未公布固定数字）；Microsoft 建筑足迹对中国大陆的覆盖完整性（README 仅称 130 国）；Overture S3 的国内可达性（仅测目录路径，未测具体文件）；天地图是否有公开的建筑级面数据 API（官方文档未查到）。
