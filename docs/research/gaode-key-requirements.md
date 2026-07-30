# V2 地图与采集凭证需求核验

> **调研日期**：2026-07-29
> **调研范围**：校区搜索、嵌入式地图、边界编辑、朝向设置，以及 OSM、Overture 数据采集。
> **资料范围**：产品目标以已接受 ADR 为法源；外部服务事实只使用高德、
> OpenStreetMap/Overpass 和 Overture Maps 官方资料；实现现状只使用本项目源码。
>
> **重要纠正**：初稿曾把当前代码中的高德 POI 采集误当成正式产品目标，因而错误地
> 把高德 Web服务 Key 列为完整 V2 的必填项。ADR-0013 已明确首发采集数据源是 OSM、
> Overture；以下结论已改为以 ADR 为准。
>
> **实现证据说明**：本文引用的 2026-07-29 未提交实现现已原样封存在 WIP 提交 `37d7b04`；这些路径用于说明当时的偏差，不代表当前 `main` 已采用该实现。

## 一、最终产品结论

### 1.1 正式 V2 只需填写两个高德字段

为完成 ADR 已确认的 V2 首发功能，设置页只需：

1. **高德 Web端（JS API）Key**；
2. 与该 Key 配套的 **安全密钥 `securityJsCode`**。

这是一组不可拆分的凭证，用于高德校区搜索、地图显示、坐标转换、边界编辑和地图上
确定朝向。2021 年 12 月 2 日以后创建的 JS API Key 必须与安全密钥配合使用
（[高德官方：JS API 准备](https://lbs.amap.com/api/javascript-api-v2/prerequisites)；
[高德官方：安全密钥使用](https://lbs.amap.com/api/javascript-api-v2/guide/abc/jscode)）。

### 1.2 高德 Web服务 Key 不是正式 V2 必填项

[ADR-0013](../adr/0013-pluggable-data-sources-with-recommendation.md) 明确规定 V2 首发
内置 **OSM、Overture** 两个采集适配器，没有把高德列为采集数据源。
[ADR-0029](../adr/0029-boundary-from-osm-with-manual-adjustment.md) 进一步规定：方案边界
通过 OSM Overpass 自动获取，高德承担地图显示、坐标转换和边界编辑。

OSM/Overpass 的本项目所需查询不要求 API Key；Overture 官方公开数据也不要求
API Key 或云账号。因此，完成正式 ADR 功能不需要高德 Web服务 Key。

只有未来另行作出产品决策，把高德加入正式采集数据源并继续直连
`restapi.amap.com` 时，才需要单独申请高德 Web服务 Key。

### 1.3 为什么过去只填 Web端 Key 也能做多种地理功能

高德同时提供两条通道：

- JS API 插件，如 `AMap.PlaceSearch`、`AMap.Geocoder`、`AMap.Geolocation`，使用
  JS API Key + `securityJsCode`；
- HTTP Web服务，如 `restapi.amap.com` 下的 POI、地理编码接口，使用另一种
  “Web服务”平台 Key。

所以 Web端 JS API 凭证组本身就能通过插件完成搜索、地理/逆地理编码和定位，并不
需要同时填写 Web服务 Key。只有程序直接请求 Web服务接口时才需要后者。

## 二、三类外部服务的官方凭证要求

### 2.1 高德 JS API：需要 Key + 配套安全密钥

高德控制台创建 JS API Key 时，服务平台选择“Web端（JS API）”；创建后取得 Key 和
与其配套的安全密钥。`securityJsCode` 不是 Web服务 Key，而是 JS API 鉴权的一部分，
必须在加载 JS API 脚本之前设置
（[高德官方：JS API 准备](https://lbs.amap.com/api/javascript-api-v2/prerequisites)；
[高德官方：安全密钥使用](https://lbs.amap.com/api/javascript-api-v2/guide/abc/jscode)）。

| 正式 V2 功能 | 通道 | 用户必须填写的凭证 |
|---|---|---|
| 搜索并选定校区 | `AMap.PlaceSearch` | JS API Key + `securityJsCode` |
| 显示嵌入式地图 | 高德 JS API 2.0 | JS API Key + `securityJsCode` |
| OSM 坐标转高德坐标 | `AMap.convertFrom` | JS API Key + `securityJsCode` |
| 显示、圈画、调整边界 | 高德地图、`PolygonEditor` | JS API Key + `securityJsCode` |
| 地图上确定朝向 | 高德地图与覆盖物 | JS API Key + `securityJsCode` |

官方功能资料：

- [搜索地点 `AMap.PlaceSearch`](https://lbs.amap.com/api/javascript-api-v2/tutorails/search-poi)
- [坐标转换 `AMap.convertFrom`](https://lbs.amap.com/api/javascript-api-v2/guide/transform/convertfrom)
- [多边形编辑 `PolygonEditor`](https://lbs.amap.com/api/maps-javascript-api/reference/tool/polygoneditor)

### 2.2 OSM/Overpass：本项目查询无需 Key 或账号

Overpass 提供公开查询端点，例如：

```text
https://overpass-api.de/api/interpreter
```

OSM 官方 Wiki 列出的主公共实例端点不包含 Key；同一列表中，需要 Key 的商业实例会
明确在路径中写出 `YOUR_API_KEY`
（[OpenStreetMap Wiki：Overpass API 公共实例](https://wiki.openstreetmap.org/wiki/Overpass_API)）。

OSM 的鉴权说明进一步明确：不涉及用户个人元数据的请求继续无需 API Key；只有请求
贡献者归属、用户 ID、changeset 等个人元数据时才涉及 Key 机制
（[OpenStreetMap Wiki：Overpass API/GDPR](https://wiki.openstreetmap.org/wiki/Overpass_API/GDPR)）。

本项目查询学校 way/relation 的标签和几何边界，不请求贡献者个人元数据。因此：

- 不需要用户填写 OSM Key；
- 不需要用户登录 OSM 账号；
- 不应在设置页增加 OSM 凭证字段。

不需要 Key 不等于没有运行限制。Overpass 公共实例按 IP 限流，高负载时可能返回
HTTP 429 或 504；官方建议一般使用量控制在约每天 10,000 次查询和 1 GB 下载量以内，
更大规模应考虑自建实例
（[Overpass 官方手册：公共资源与配额](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html)）。
这属于可靠性和容量设计，不是用户凭证要求。

### 2.3 Overture：官方公开数据无需 Key 或云账号

Overture 将版本化 GeoParquet 数据公开在 Amazon S3、Microsoft Azure Blob Storage 和
官方 STAC 目录。官方 AWS 下载命令明确使用 `--no-sign-request`，即不使用 AWS 签名
和账号凭证；Azure 则直接提供公开 URL
（[Overture 官方：访问数据目录](https://docs.overturemaps.org/getting-data/cloud-sources/)）。

官方 Quickstart 提供直接读取 S3 和公开 STAC JSON 的示例，没有 API Key、登录或云
账号步骤（[Overture 官方：Quickstart](https://docs.overturemaps.org/getting-data/)）。
Places 数据文档也明确称数据在 S3 和 Azure 上“freely available”
（[Overture 官方：Places 数据访问](https://docs.overturemaps.org/guides/places/)）。

因此，V2 使用 Overture 官方公开数据源时：

- 不需要 Overture API Key；
- 不需要 AWS 或 Azure 账号；
- 不应在设置页增加 Overture 凭证字段。

BigQuery、Databricks、Snowflake 等可选镜像可能有各自账号或计费要求，但它们不是读取
Overture 官方公开 S3/Azure 数据的必要条件。

## 三、ADR 正式目标与当前代码必须分开

### 3.1 ADR 正式产品目标

ADR-0013 要求首发实现 OSM、Overture 两个独立适配器，用户至少选择一个，也可以
多选；应用还要按当前区域的完整程度、精细程度和数据时点排序推荐。

ADR-0029 要求以 OSM Overpass 自动获取边界，再由高德 JS API 转换、显示和编辑。
所以正式产品的凭证表是：

| 正式能力 | 数据来源 | 用户凭证 |
|---|---|---|
| 校区搜索、地图、边界编辑、朝向 | 高德 JS API | JS API Key + `securityJsCode` |
| 自动获取校区边界 | OSM Overpass | 无 |
| 首发采集源 OSM | OSM/Overpass | 无 |
| 首发采集源 Overture | 公开 S3/Azure/STAC | 无 |
| 高德 Web服务 POI 采集 | 不在当前 ADR 首发范围 | 不要求 |

### 3.2 当前代码现状不是产品法源

边界获取已经符合 ADR-0029：页面无 Key 请求 Overpass，取得 OSM 边界，再调用
`AMap.convertFrom` 并在高德地图上编辑；失败则回退人工圈画
（[boundary_edit_map_page.rs](../../core/gaode-client/src/boundary_edit_map_page.rs)）。

但正式采集实现偏离 ADR-0013：

1. `core/data-acquisition` 只有 `GaodeDataSource`，没有 OSM、Overture 适配器；
2. 桌面端开始采集时固定创建 `GaodeDataSource`；
3. `runCollection` 直接请求 `https://restapi.amap.com/v3/place/polygon`；
4. 设置页因此新增并传递了高德 Web服务 Key；
5. 用户还不能在 OSM、Overture 中至少勾选一个并查看排序推荐。

源码证据：

- [data-acquisition/source.rs](../../core/data-acquisition/src/source.rs)
- [data-acquisition/lib.rs](../../core/data-acquisition/src/lib.rs)
- [desktop/injector.rs](../../apps/desktop/src/injector.rs)
- `apps/desktop/src/collection.rs`（见 WIP 提交 `37d7b04`）
- [boundary_edit_map_page.rs](../../core/gaode-client/src/boundary_edit_map_page.rs)

这是一项需要修复的实现偏差。应实现 OSM、Overture 适配器和数据源选择流程，并把正式
采集迁回 ADR-0013 指定的数据源；不能通过要求用户填写第三个 Key 来固化偏差。

## 四、对设置页规则的建议

### 4.1 只把两个 JS API 字段设为必填

| 字段 | 保存要求 | 缺失提示 |
|---|---:|---|
| 高德 Web端（JS API）Key | 必填 | 无法使用校区搜索和地图功能 |
| 高德 JS API 安全密钥（securityJsCode） | 必填 | 必须与上面的 JS API Key 配套 |

任意一项为空时，点击“保存”应不写入新配置，并明确指出缺少哪个字段。

高德 Web服务 Key 不属于正式必填项。产品负责人决定暂时保留该字段，并明确标注
“开发人员使用”；普通用户可留空，且不得阻止用户保存正式所需的两项凭证。该字段
不能被描述为 ADR-0013 正式 OSM/Overture 采集流程的必要配置。

### 4.2 清除按钮应按凭证组清除

产品负责人决定提供“清除全部高德密钥”按钮。点击后先弹出确认窗口，明确说明清除后
地图功能将暂时不可用，直到重新填写；用户确认后一次清除 Web端（JS API）Key、
`securityJsCode` 和标注为“开发人员使用”的 Web服务 Key，同时清空页面输入内容与
本地已保存值，取消则保持原配置不变。清除立即生效，并短暂提示“高德密钥已清除”。

### 4.3 连通性测试只验证高德 JS 凭证组

“测试高德地图”应实际验证 JS API Key + `securityJsCode` 能否加载地图并完成一次轻量
调用。

产品负责人决定“保存密钥”与“测试连通性”分开：保存只检查必填项和明显格式问题，
不依赖当前网络；测试按钮才进行真实联网验证。断网时允许先保存已填写内容，避免网络
故障导致用户重新输入。

同一个“测试连通性”按钮始终验证 Web端 Key + `securityJsCode`；若标注为“开发人员
使用”的 Web服务 Key 已填写，则同时验证该通道。结果按“地图服务”和“开发用
Web服务”分开显示，互不掩盖；Web服务 Key 未填写时显示“未配置”，不算失败。

反馈按影响分级：地图服务失败时弹出确认窗口说明原因；地图服务成功时短暂提示
“地图服务连接成功”；开发用 Web服务失败时在结果区标红并给出短暂提示，但不弹
阻断窗口；所有已执行测试均成功时短暂提示“连通性测试成功”。

OSM 和 Overture 不需要 Key，但采集模块可以另做“数据源可用性检查”：

- Overpass：发送小范围轻量查询，区分成功、限流、超时和服务不可用；
- Overture：读取 STAC 目录并尝试读取一个小范围数据片段。

这些属于网络和服务可用性检查，不是凭证校验。

## 五、当前实现缺口与修复方向

以下是源码事实，不是新的产品决策：

1. **首发数据源实现错位**：F4 当前只有 `GaodeDataSource`，而 ADR-0013 要求首发内置
   OSM、Overture 两个适配器。
2. **采集入口固定走高德**：桌面端固定创建 `GaodeDataSource`，没有 ADR 要求的至少
   选择一个数据源、多选和推荐排序流程。
3. **Web服务 Key 是偏离实现带来的临时字段**：`runCollection` 直连高德
   `/v3/place/polygon`，所以当前代码缺少该 Key 时会失败；这不等于正式产品需要它。
4. **旧 Web服务 Key 无法通过清空输入框删除**：保存逻辑只在第三个字段非空时写入，
   清空后保存会留下数据库旧值。
5. **现有“连通性测试”不是真正的网络测试**：`test_gaode_connection` 只检查前两个
   字符串的长度和字符集，不发送请求；两个字段都为空时甚至返回成功
   （[当前实现](../../core/global-settings/src/settings.rs)）。
6. **运行入口检查不完整**：部分路径只检查 JS API Key 是否存在，没有同时确认
   `securityJsCode`，可能在页面生成阶段才失败。

修复顺序建议：

1. 以 ADR-0013 实现 OSM、Overture 适配器及用户选择/推荐流程；
2. 将 F4 正式采集入口从 `GaodeDataSource` 迁回 OSM、Overture；
3. 保留高德 Web服务 Key 作为标注“开发人员使用”的可选字段，但从正式采集链路和必填校验中移除；
4. 设置页把 JS API Key + `securityJsCode` 作为正式必填项；开发用 Web服务 Key 仅在已填写时单独测试，其失败不阻断普通功能；
5. OSM、Overture 分别做无凭证的数据源可用性检查。

## 六、最终回答

以 ADR-0013 和 ADR-0029 为准，V2 完整首发功能需要用户申请：

1. 一个服务平台为“Web端（JS API）”的高德 Key；
2. 复制与它配套的 `securityJsCode`。

不需要用户申请：

- 高德 Web服务 Key；
- OSM/Overpass Key 或 OSM 账号；
- Overture Key、AWS 账号或 Azure 账号。

高德 Web服务 Key 之所以出现在当前程序，是因为采集实现临时改走了高德
`place/polygon`，这与 ADR-0013 的 OSM、Overture 首发数据源决定不一致。正确下一步
是修复采集实现，而不是把第三个 Key 设为必填。

## 七、资料索引

### 产品法源

1. [ADR-0013：首发内置 OSM、Overture](../adr/0013-pluggable-data-sources-with-recommendation.md)
2. [ADR-0029：OSM 边界、高德显示与编辑](../adr/0029-boundary-from-osm-with-manual-adjustment.md)

### 高德官方

1. [JS API 2.0：准备和创建 Key](https://lbs.amap.com/api/javascript-api-v2/prerequisites)
2. [JS API 2.0：安全密钥使用](https://lbs.amap.com/api/javascript-api-v2/guide/abc/jscode)
3. [JS API 2.0：搜索地点](https://lbs.amap.com/api/javascript-api-v2/tutorails/search-poi)
4. [JS API 2.0：地理/逆地理编码](https://lbs.amap.com/api/javascript-api-v2/guide/services/geocoder)
5. [JS API 2.0：定位](https://lbs.amap.com/api/javascript-api-v2/guide/services/geolocation)
6. [JS API 2.0：坐标转换](https://lbs.amap.com/api/javascript-api-v2/guide/transform/convertfrom)
7. [Web服务 API：状态码与 Key 平台不匹配](https://lbs.amap.com/api/web-service/tools/info)

### OpenStreetMap / Overpass 官方

1. [OpenStreetMap Wiki：Overpass API 与公共实例](https://wiki.openstreetmap.org/wiki/Overpass_API)
2. [OpenStreetMap Wiki：Overpass API/GDPR 与 Key 边界](https://wiki.openstreetmap.org/wiki/Overpass_API/GDPR)
3. [Overpass 官方手册：公共资源、限流与容量](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html)

### Overture Maps 官方

1. [Overture 官方：Quickstart](https://docs.overturemaps.org/getting-data/)
2. [Overture 官方：公开 S3/Azure/STAC 数据](https://docs.overturemaps.org/getting-data/cloud-sources/)
3. [Overture 官方：Places 数据访问](https://docs.overturemaps.org/guides/places/)

> 外部服务的访问方式、容量和政策可能变化。实施前应再次核对官方文档；但任何外部
> 变化都不能绕过 ADR 流程，直接改变正式产品的数据源范围。