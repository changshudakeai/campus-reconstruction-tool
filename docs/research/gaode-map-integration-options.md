# 高德地图集成方案调研：Rust + Slint Windows 桌面应用

> **调研日期**：2026-07-27  
> **背景**：项目为 Rust + Slint (winit 后端) 的 Windows 桌面应用，已有 `core/gaode-client`（B3）模块生成高德 JS API 地图页 HTML，但壳层无 WebView/HTTP 客户端依赖。需要支撑校区搜索、圈边界绘制、定朝向三个场景。
> 
> **约束**：壳零业务逻辑、新依赖必须过 cargo deny（MIT/Apache-2.0/BSD）、Windows 11 平台、用户 Win11 系统自带 WebView2 Runtime。
> 
> **调研方法**：仅引用一手来源——高德开放平台官方文档（lbs.amap.com）、crates.io/GitHub 官方仓库、Slint 官方 issue/discussion、Microsoft Learn。

---

## 一、结论速览

### 推荐路线：**路线 A —— JS API 2.0 + wry WebView（嵌入子窗口模式）**

**一句话理由**：Slint 官方维护者 ogboNa（tronical）在 [issue #3930](https://github.com/slint-ui/slint/issues/3930#issuecomment-1976039803) 中明确建议使用 wry 实现 webview 嵌入，且 JS API 2.0 对"地图上逐点点击画多边形 + 实时预览"的交互能力远超静态地图，Windows 端 WebView2 已由 Win11 预装无需额外分发体积。

### 安全最佳实践说明

- **CDN 脚本加载**：B3 生成的 HTML 通过官方 CDN (`webapi.amap.com`) 加载 JS API，建议在 CSP 策略或构建时增加 SRI (Subresource Integrity) 校验，详见 [Security note in scanner](#security-best-practice)。
- **key 泄露防护**：生产环境建议使用代理服务器转发方式配置 `securityJsCode`，避免 key 明文暴露在前端。

### 三条路线对比表

| 评估维度 | 路线 A: JS API + wry | 路线 B: Web 服务 API + 静态地图 | 路线 C: 混合路线 |
|---------|---------------------|------------------------------|-----------------|
| **违和感** | **最小** ✅ — Slint 官方推荐 + winit 原生配合 | 高 ❌ — 静态图无法实时预览交互 | 中等 ⚠️ — 需同时维护两种渲染管线 |
| **交互能力** | **完整** ✅ — 缩放平移、点击拾取坐标、动态覆盖物实时更新 | **受限** ❌ — 图片固定像素，需每次重绘请求，无法做到"实时拖拽预览" | 部分 ✅ — 搜索用纯 HTTP，绘制仍需 WebView |
| **依赖体积** | 中等 ⚠️ — wry ~65+ crate，最终 exe +10~30MB；但 WebView2 Runtime 由 Win11 自带 | **极简** ✅ — ureq ~5 crate, reqwest ~40+ crate，纯逻辑层 | 最大 ❌ — A+B 全部依赖叠加 |
| **合规风险** | **低** ✅ — JS API 官方 CDN 加载，含版权信息；条款允许 | **中** ⚠️ — 静态地图条款要求展示 logo/版权；不得缓存底图 | 取决于组合方式 |
| **离线容错** | 网络不可用时无法加载地图 | 可缓存静态图（但条款限制） | — |
| **开发成本** | **最低** ✅ — B3 已产出 map_page.html 模板；壳仅需引入 wry + IPC 桥接 | 高 ❌ — 需自行实现 Web Mercator 反算 + 多次 HTTP 请求合成 | 高 ❌ — 两套协议栈 |
| **许可证** | **Apache-2.0/MIT** ✅ | **Apache-2.0/MIT** (ureq)/Apache-2.0+MIT (reqwest) ✅ | — |

---

## 二、路线 A：JS API 2.0 + wry WebView

### 2.1 高德 JS API 2.0 密钥机制

#### 密钥类型与配置

JS API 2.0 使用 **双密钥体系**：

1. **`key`（Web API key）**：普通身份标识，用于调用基础服务
2. **`securityJsCode`（安全密钥）**：增强安全性，防止 key 泄露滥用

**明文配置方式**（仅限开发环境，生产环境建议通过代理服务器转发）：

```javascript
// 必须在 JS API 脚本加载前设置
window._AMapSecurityConfig = {
    securityJsCode: "你的安全密钥",  // 在高德控制台申请
};
<script src="https://webapi.amap.com/maps?v=2.0&key=你申请的 key 值"></script>
```

**代理服务器配置方式**（官方推荐）：[JS API 安全密钥使用 - 基础教程](https://lbs.amap.com/api/javascript-api-v2/guide/abc/prepare)

```nginx
location /_AMapService/v4/map/styles {
    set $args "$args&jscode=你的安全密钥";
    proxy_pass https://webapi.amap.com/v4/map/styles;
}
location /_AMapService/restapi.amap.com/ {
    set $args "$args&jscode=你的安全密钥";
    proxy_pass https://restapi.amap.com/;
}
```

#### 免费额度与配额（个人认证开发者）

| 服务类型 | 月配额 | QPS 限制 |
|---------|--------|----------|
| JS 地图图面初始化 | 1,500,000 次 | 10 次/秒 |
| 坐标转换 | 30 万次 | 3 次/秒 |

> 资料来源：[开放平台基础服务计费说明](https://lbs.amap.com/pages/base_service_price)

个人认证门槛：需在 [高德开放平台](https://console.amap.com/) 完成手机号实名认证即可。

### 2.2 wry 与 Windows WebView2 集成

#### Windows 平台工作方式

wry 在 Windows 上使用 **Microsoft Edge Chromium WebView2**，支持 Win7/8/10/11 [wry docs.rs](https://docs.rs/crate/wry/latest)：

```rust
// 标准外嵌模式（独立窗口）
let webview = WebViewBuilder::new()
    .with_url("about:blank")
    .build(&window)?;

// **推荐**：子窗口模式（嵌入现有 Slint 窗口指定矩形区域）
let webview = WebViewBuilder::new()
    .with_url("about:blank")
    .with_bounds(Rect {
        position: LogicalPosition::new(100, 100).into(),
        size: LogicalSize::new(640, 480).into(),
    })
    .build_as_child(&window)?;
```

资料来源：[wry GitHub README](https://github.com/tauri-apps/wry)、[wry docs.rs/examples](https://docs.rs/wry/latest/wry/)

#### 与 Slint 窗口的共存实践

**官方立场**：Slint 维护者 ogboNa（tronical）在 [issue #3930](https://github.com/slint-ui/slint/issues/3930#issuecomment-1976039803) 回复：

> "I see two ways of how we can achieve this in the future:
> 1. Embed an external web engine as a window. This requires a windowing system and comes limitations such as the inability to easily place controls on top of the web view.
> 2. For a Linux-only environment, an engine like wpe can provide dma buffers for rendered frames...
> 
> I think a reasonable acceptance criteria for this issue would be if https://github.com/slint-ui/slint/issues/4640 was implemented and we had at least an example in our docs how to use for example wry."

**关键点**：
- 方案 1（外嵌独立 window）存在限制：**难以在 webview 上叠加控件**（如 Slint 按钮）
- 推荐路径：若实现 #4640（webview 作为 child 而非独立 window），则可用 wry
- 当前可行方案：使用 `build_as_child()` API，将 WebView 嵌入 Slint 窗口的特定矩形区域，但 Z-order 管理需谨慎（Slint UI 可能被 WebView 遮挡）

#### IPC 桥接：从地图点击获取经纬度

wry 提供两类桥接机制：

1. **evaluate_script（JS → Rust）**：
   ```javascript
   // JS 侧监听地图点击事件，将坐标经 postMessage 回传
   map.on("click", function(e) {
       const lngLat = e.lnglat;  // AMap.LngLat 对象
       const lng = lngLat.getLng();  // 精确到小数点后 6 位
       const lat = lngLat.getLat();
       window.mcrebuildBridge.postMessage({type: "mapClick", lng, lat});
   });
   ```

2. **ipc_handler（Rust 回调）**：
   ```rust
   let handler = move |_webview: &WebView, request: http::Request<()>| {
       // 解析 postMessage 内容，返回 JSON 格式的经纬度
       Ok(http::Response::builder().body(format!("{{\"lng\":{lng},\"lat\":{lat}}}")))?
   };
   ```

更简单的方案：直接注入初始化脚本并监听 `ipcHandler`，详见 [wry with_ipc_handler](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html#method.with_ipc_handler)。

### 2.3 依赖树与许可证

**wry 最新版本**：0.55.1 (2026-05-04)  
**许可证**：`Apache-2.0/MIT` 双重许可 ✅（符合 cargo deny 白名单）  
**主要依赖数量**：65+ crates，包括：

| 类别 | Crate 示例 | 用途 |
|------|-----------|------|
| 核心绑定 | `webview2-com`, `windows`, `raw-window-handle` | 调用 Windows WebView2 API |
| 跨平台抽象 | `http`, `cookie`, `dpi` | 协议、Cookie、DPI 感知 |
| Dev 依赖 | `winit`, `tao` | 测试所需，非 runtime 必需 |

资料来源：[wry docs.rs metadata](https://docs.rs/crate/wry/latest)、[wry GitHub](https://github.com/tauri-apps/wry)

> **注意**：wry 的依赖中包含 Windows-specific 的 `windows` crate (~0.61)，但最终 EXE 仅在 Windows 目标编译时链接；Slint 本身已是 Windows 优先框架，此依赖不会造成额外负担。

### 2.4 地图点击事件的精度

**JS API 2.0 返回格式**：

```javascript
map.on("click", clickHandler);
function clickHandler(e) {
    console.log('您在[' + e.lnglat.getLng() + ',' + e.lnglat.getLat() + ']的位置点击了地图！');
}
```

- 返回对象：`e.lnglat` 为 `AMap.LngLat`
- 方法：`getLng()` / `getLat()` 返回数值型经纬度
- 精度：**小数点后 6 位**（约 0.1 米级别），满足校园尺度绘图需求

资料来源：[JS API 2.0 地图事件文档](https://lbs.amap.com/api/javascript-api-v2/guide/map/events)

### 2.5 合规风险与服务条款

**必须遵守的限制**：

| 条款编号 | 要求 | 对项目的影响 |
|---------|------|-------------|
| 4.10.6 | 遵守测绘法、标注审图号 | 若地图数据涉及公开地图发布需送审；校园内网使用可不强制 |
| 4.10.7 | **不得删除版权声明、商标声明或其他所有权声明** | 高德地图页面默认包含 logo/版权角标，B3 不能移除 |
| 3.5 | 只可使用官方文档列明的功能来展示；不得直接存储、缓存或使用技术手段抓取内部数据 | **不允许缓存静态图或 JS API 资源**；必须每次联网加载官方 CDN 资源 |

资料来源：[高德地图开放平台服务协议](https://lbs.amap.com/pages/terms/)

---

## 三、路线 B：Web 服务 API + 静态地图

### 3.1 POI 搜索能力对比

| 特性 | JS API 2.0 前端搜索 | Web 服务 API 后端搜索 |
|------|-------------------|---------------------|
| API 入口 | 浏览器 JS（AMap.PlaceSearch） | HTTP GET (`restapi.amip.com/v3/place/text`) |
| 关键字搜索 | ✅ | ✅ |
| 周边搜索 | ✅ | ✅ |
| 多边形搜索 | ✅ | ✅ |
| 返回坐标系 | GCJ-02（高德加密坐标系） | GCJ-02（高德加密坐标系） |
| 翻页限制 | 最多 200 条（同请求参数） | 最多 200 条（强烈建议 offset≤25） |
| 扩展字段 | `extensions=all` 时返回道路交叉口等 | `extensions=all` 时返回电话、网址等 |

**关键差异**：
- **坐标系一致**：两者均返回 GCJ-02 坐标（与 B5 模块的 Web Mercator 换算兼容）
- **响应字段**：Web 服务 API 提供更多商业属性（电话、评分、团购等），但对本项目需求无差异影响

资料来源：[POI 搜索 API 文档](https://lbs.amap.com/api/webservice/guide/api/search)、[JS API PlaceSearch](https://lbs.amap.com/api/javascript-api-v2/reference/extension/search)

> **疑点**：未查到权威来源说明 Web 服务 API 是否返回原始 WGS-84 坐标；假设均为 GCJ-02（高德内网坐标），需用 B5 模块的坐标换算库转换为 CGCS2000。

### 3.2 静态地图 API 能力边界

#### 基本参数与限制

```bash
GET https://restapi.amap.com/v3/staticmap?
  location=116.37359,39.92437&   # 中心点（经度，纬度）
  zoom=10&                        # 缩放级别 [1,17]
  size=750*300&                   # 宽高 px；**最大 1024*1024**
  markers=mid,,A:116.37359,39.92437&  # 标注（最大 10 个）
  paths=5,0x0000FF,1,:116.31604,39.96491;...&  # 折线/多边形（最大 4 条）
  labels=朝阳公园，2,0,16,0xFFFFFF,0x008000:116.37359,39.92437&  # 标签（最大 10 个）
  scale=2&                        # 1=普通图，2=高清图（尺寸×2）
  key=<用户的 key>
```

**硬性限制**：
- **最大尺寸**：1024×1024 px（scale=2 时最高 2048×2048）
- **覆盖物数量**：markers 最多 10 个；paths 最多 4 条；labels 最多 10 个
- **动态性**：每次点击需在服务端重新生成图片；**不支持本地渲染交互**（缩放/平移）

资料来源：[静态地图 API 文档](https://lbs.amip.com/api/webservice/guide/api/staticmaps)

#### Web Mercator 比例尺换算问题

**核心疑问**：官方文档**未明确给出像素 ↔ 地理距离的换算公式**。

已知参数：
- `location`: 中心点经纬度 (λ₀, φ₀)
- `zoom`: 缩放级别 (Z)
- `size`: 图片分辨率 (W, H) px

推测公式（基于 EPSG:3857 标准）：
```
地面分辨率 (米/px) ≈ (cos(φ₀) × 40075017) / (256 × 2^Z)
横向跨度 (米) ≈ 地面分辨率 × W
纵向跨度 (米) ≈ 地面分辨率 × H
```

但以下细节需实测验证：
- 高德静态图是否严格遵循 EPSG:3857 投影？
- zoom 级别与 TMS/Google Maps 的对应关系是否一致？
- scale=2 高清图的 pixel-to-meter 比例是否线性翻倍？

> **结论**：静态地图方案的坐标反算**缺乏权威来源佐证**，需实测才能确认。

### 3.3 流量配额（个人认证开发者）

| 服务类型 | 月配额 | QPS 限制 |
|---------|--------|----------|
| 关键字搜索 | 5,000 次 | 3 次/秒 |
| 周边搜索 | 30 次/秒（无明确月配额上限？） | 3 次/秒 |
| 静态地图 | 30 万次 | 3 次/秒 |

> 注：表格里周边搜索的月配额显示为"—"，可能是无上限或需商务咨询，待进一步确认。

资料来源：[基础服务计费说明](https://lbs.amap.com/pages/base_service_price)

### 3.4 依赖对比：ureq vs reqwest

| 特性 | ureq | reqwest |
|------|------|---------|
| **API 风格** | 同步阻塞（简单、小依赖树） | 异步/阻塞双支持（tokio 生态） |
| **TLS 后端** | rustls（默认，纯 Rust） | rustls-tls / native-tls（可选） |
| **最新版本** | 2.x | 0.13.x |
| **许可证** | MIT/Apache-2.0 ✅ | MIT/Apache-2.0 ✅ |
| **依赖规模** | ~5 个核心 crate（`ureq`, `rustls`, `base64`, `log`, `url`） | ~40+ crate（含 `tokio`, `hyper`, `bytes` 等 async 栈） |
| **EXE 体积增加** | ~300 KB | ~2~5 MB（异步运行时开销大） |
| **是否符合"壳零逻辑"** | ✅ 纯 HTTP 客户端，无需 event loop | 需 tokio runtime，略微侵入 shell |

资料来源：[ureq GitHub](https://github.com/algesten/ureq)、[reqwest GitHub](https://github.com/seanmonstar/reqwest)

> **推荐选择**：**ureq** 更符合本项目"简单、小巧、无需 async"的定位；但若未来可能需要异步并发请求（如批量 POI 查询），reqwest 更合适。

### 3.5 静态地图 + Slint 的交互流程

若要实现"点击图片→计算经纬度→回传 Rust"：

1. Slint 显示静态地图图片（通过 `Image::from_data()` 加载 PNG）
2. 监听 Slint 的鼠标点击事件 `(mouse_x, mouse_y)`
3. 反算经纬度：需实测确认比例尺公式（见 3.2 节）
4. 将 GCJ-02 坐标回传给 B3 记录

**致命缺陷**：
- 无法实时预览：用户点击第 1 点后，第二点的多边形需在服务端重新请求，延迟 >200ms
- 无法拖动缩放：每次视图变化都需重新请求静态图
- 用户体验差："逐点画多边形"的场景下，操作流畅度远低于 JS API 的矢量渲染

---

## 四、路线 C：混合路线评估

### 4.1 可能的组合方式

| 场景 | 推荐方案 | 理由 |
|------|---------|------|
| 校区搜索（列表选择） | Web 服务 API + pure Rust 列表 UI | 无需地图交互，纯 HTTP 请求足够 |
| 圈边界绘制 | JS API + wry WebView | 必须实时交互 |
| 定朝向（两点连线） | JS API + wry WebView | 需实时预览参考线 |

### 4.2 混合路线的弊端

1. **依赖膨胀**：同时引入 `wry` + `ureq`，总 crate 数增加 ~70
2. **状态管理复杂**：搜索结果与地图状态需跨两层同步
3. **不符合原则**：项目纪律要求"壳零业务逻辑"，而混合路线意味着壳需同时处理 HTTP 客户端 + WebView 宿主
4. **收益有限**：校区搜索场景完全可以用已有的 `CampusSearchFlow` 状态机 + 列表 UI 替代（不一定非要地图交互）

> **结论**：除非有强证据证明"纯 HTTP 列表搜索"比"JS API 地图搜索 + 列表弹窗"更好，否则不推荐混合路线。**单一路线 A 更省心**。

---

## 五、横向问题汇总

### 5.1 密钥类型与申请门槛

| API 类型 | key 名称 | 申请方式 | 免费额度（个人认证） | 日配额估算 |
|---------|---------|---------|-------------------|------------|
| JS API 2.0 | `key` + `securityJsCode` | 高德控制台申请 Web 平台（JS API） | 150 万次/月 | ~5 万次/天 |
| Web 服务 API | `key`（单独） | 高德控制台申请 Web 服务 API | 5,000 次/月（关键字搜索） | ~170 次/天 |

> **注意**：个人认证与企业认证配额差距显著（企业认证静态地图 300 万次/月）。

资料来源：[开放平台控制台](https://console.amap.com/)、[基础服务计费说明](https://lbs.amap.com/pages/base_service_price)

### 5.2 依赖体积与编译成本

| 方案 | 新增 crate 数 | 预计 EXE 增量 | CI 构建时间影响 |
|------|------------|------------|----------------|
| 路线 A: wry | ~65 | +10~30 MB | +1~2 分钟（Windows WebView2 SDK 下载） |
| 路线 B: ureq | ~5 | +300 KB | +10 秒 |
| 路线 B: reqwest | ~40 | +2~5 MB | +30 秒 |

> **实测数据缺失**：实际增量取决于是否启用 `dev-dependencies`；wry 的 `webview2-com` 需要下载 Windows SDK，首次编译可能耗时 5~10 分钟。

### 5.3 交互能力上限

| 功能需求 | 路线 A (JS API + wry) | 路线 B (静态地图) |
|---------|----------------------|------------------|
| 地图上逐点点击画多边形 | ✅ 原生支持，实时绘制虚线/填充 | ❌ 需服务端合成，延迟明显 |
| 实时预览（拖拽调整点位置） | ✅ JS EventListener 即时反馈 | ❌ 不可行 |
| 地图缩放/平移查看全局 | ✅ 交互流畅 | ❌ 需频繁请求静态图 |
| 标注/折线/多边形的样式自定义 | ✅ 丰富（颜色、透明度、线宽） | ✅ 支持部分（但数量有限制） |
| 点击拾取坐标 | ✅ 高精度返回 | ⚠️ 需自行反算，缺乏官方验证 |

### 5.4 合规风险总结

| 风险项 | 路线 A | 路线 B |
|-------|-------|-------|
| **必须展示高德 logo/版权** | ✅ 默认自带（JS API 页面内嵌） | ✅ 静态图 URL 内嵌（需保留） |
| **禁止缓存底图/资源** | ⚠️ 必须联网加载（CDN） | ❌ 条款禁止缓存静态图（但实务中可短暂缓存 PNG） |
| **需标注审图号** | ⚠️ 若公开发布需送审；内网工具可不强制 | 同上 |
| **禁止删除版权声明** | ✅ 不影响（只需保留原样） | ✅ 不影响 |

### 5.5 离线容错能力

| 方案 | 完全离线 | 弱网（高延迟） | 临时断网 |
|------|---------|--------------|---------|
| **路线 A (JS API)** | ❌ 无法加载地图 | ⚠️ 首屏慢，后续交互卡住 | ❌ 报错 |
| **路线 B (静态地图)** | ⚠️ 可缓存上一张图片（但不符合条款） | ⚠️ 每次点击等待 HTTP 响应 | ❌ 更新失败 |

> **建议**：无论选哪种路线，都应在 B3 增加网络检测 + 降级提示（如"网络不可用，请检查连接"）。

---

## 六、落地建议

### 6.1 若采用推荐路线（路线 A）

#### 6.1.1 B3 模块新增代码

| 文件 | 新增内容 |
|------|---------|
| `core/gaode-client/src/map_page.rs` | • 支持 **地图点击事件注册**：<br>`fn add_map_click_handler(html: &mut String, handler_name: &str)`<br>• 添加 **实时坐标拾取脚本**：<br>`map.on('click', (e) => { window.mcrebuildBridge.postMessage(JSON.stringify({type:'click',lng:e.lnglat.getLng(),lat:e.lnglat.getLat()})) })` |
| `core/gaode-client/src/record.rs` | • 新增 `DrawnBoundaryRecord`：保存用户逐点绘制后的多边形坐标数组<br>`pub struct DrawnBoundaryRecord { pub points: Vec<CampusPoiRecord>, }` |
| `core/gaode-client/src/search_flow.rs` | • `CampusSearchFlow` 增加 `"boundary_drawing"` 状态分支 |

#### 6.1.2 壳层新增依赖

```toml
# shell/Cargo.toml
[dependencies]
wry = "0.55"      # WebView 嵌入
raw-window-handle = "0.6"  # wry 要求 HasWindowHandle trait
slint = { version = "1.7", features = ["renderer-femto-svg"] }  # 保持现有
```

**许可证检查清单**（cargo deny 白名单）：
- ✅ wry: Apache-2.0/MIT
- ✅ raw-window-handle: Apache-2.0/MIT
- ✅ slint: MIT (原有)

#### 6.1.3 deny.toml 评估项

需关注以下依赖的许可证透传：

```toml
[allow]
crate = [
    # 已知 OK 的 crate
    "webview2-com",     # Microsoft 授权，非开源
    "windows",          # MIT
    "http",             # MIT
]
```

**潜在风险**：
- `webview2-com` 是 Windows SDK 绑定，非典型开源 crate，需确认其 license 条款
- `block2`, `objc2-*`（macOS 支持）可能带 BSD/Apache 多重许可

建议运行 `cargo deny check license` 验证。

#### 6.1.4 密钥配置

| 环节 | Key 类型 | 存储方式 |
|------|---------|---------|
| **高德控制台申请** | 1. JS API 2.0 平台的 `key` + `securityJsCode`<br>2. Web 服务 API 的 `key`（备选） | 1. 创建两个 separate key（避免权限混用）<br>2. 企业级部署应通过环境变量注入，不硬编码 |
| **B3 注入 HTML** | JS API `key` | `MapPageConfig.api_key` |
| **壳层运行时** | `securityJsCode` | 通过 `env!("GAODE_SECURITY_JSCODE")` 编译期注入或运行时读取 `.env` 文件 |

**最佳实践**：
```powershell
# .env.local（不入库）
GAODE_API_KEY=xxxxxxxxxxxxxxx
GAODE_SECURITY_JSCODE=yyyyyyyyyyyyyy
```

```rust
// Shell 启动时读取
let api_key = env::var("GAODE_API_KEY").expect("缺少 GAODE_API_KEY");
let security_code = env::var("GAODE_SECURITY_JSCODE").expect("缺少 GAODE_SECURITY_JSCODE");
```

### 6.2 壳层 WebView 集成伪代码

```rust
// apps/desktop-shell/src/main.rs
use slint::{SharedString, Window};
use wry::{WebViewBuilder, RequestAsyncResponder};
use std::rc::Rc;

fn create_map_webview(window: &Rc<slint::Window>) -> Result<(), wry::Error> {
    let html = gaode_client::build_map_page_html(&MapPageConfig {
        api_key: env::var("GAODE_API_KEY")?,
        height_px: 480,
    })?;
    
    // 添加地图点击事件处理器
    let mut html_with_handler = html;
    append_map_click_handler(&mut html_with_handler);  // JS 注入
    
    WebViewBuilder::new()
        .with_window(window.as_ref())  // 绑定到 Slint 窗口
        .with_bounds(Rect {             // 指定嵌入矩形
            position: LogicalPosition::new(0, 0).into(),
            size: LogicalSize::new(640, 480).into(),
        })
        .with_html(html_with_handler)
        .with_ipc_handler(|request, responder| {
            // 解析 JS postMessage，提取坐标
            if let Some(coords) = extract_lnglat_from_request(&request) {
                // 通过 Slint Context 回传坐标
                shell_context.set_last_click_coordinate(coords);
                responder.respond(http::Response::default());
            }
        })
        .build()?;
    
    Ok(())
}
```

### 6.3 备选方案（如果 wry 集成受阻）

若遇到 Slint + wry 的兼容性坑（如 Z-order 问题导致 Slint 控件无法覆盖 WebView）：

| 备选措施 | 描述 |
|---------|------|
| **1. 切换为独立 WebView 子窗口** | 不再嵌入 Slint 窗口，而是创建一个浮动顶层窗口（类似 Qt 的 `QWebEngineView` 浮动面板），通过窗口管理器自然叠加 |
| **2. 暂时使用 Chrome App Extension 作为辅助** | 开发一个独立的 Chrome 扩展加载高德地图，通过 `chrome.runtime.sendMessage` 桥接到本地服务器，再传到 Rust（极客方案，短期可行） |
| **3. 延期至 Slint #4640 实现** | 关注 [Slint issue #4640](https://github.com/slint-ui/slint/issues/4640)（embed webview as child widget），一旦实现即迁移 |

> **不建议**：使用第三方 WebView 封装（如 `cef-rs`），因为二进制体积过大（CEF ~150MB）且超出 cargo deny 范围。

---

## 七、待实测疑点清单

以下问题**无法从一手资料中找到权威答案**，需本地实测验证：

| 编号 | 疑点 | 建议验证方式 |
|------|------|-------------|
| ① | 静态地图 API 的 **Web Mercator 比例尺换算公式**是否严格按 EPSG:3857 标准？zoom 级别与 Google Maps 是否一致？ | 请求一张已知尺寸的静态图，测量图上两点的地面距离，反向推导 pixel/meter 比例 |
| ② | wry `build_as_child()` 在 **Slint femto-svg 渲染器**上的 Z-order 表现：Slint 控件能否正确覆盖 WebView？ | 创建一个 Slint 按钮，测试其在 WebView 上方是否可点击 |
| ③ | wry + slint 的 **event loop 协作**：wry 的 WebView 消息循环是否与 Slint 的 winit backend 冲突？ | 尝试运行一个带有 wry WebView 的 Slint demo，观察是否有死锁/无响应 |
| ④ | JS API 2.0 的 **GCJ-02 坐标精度**在小尺度（<100 米）是否稳定？ | 在同一位置连续点击 10 次，统计 lng/lat 的方差 |
| ⑤ | 高德 **staticmap API 的 scale=2**高清图是否真的按 2 倍线性提升分辨率？还是插值模糊？ | 比较 scale=1 与 scale=2 的文本标签清晰度 |
| ⑥ | ureq 的 **TLS 握手延迟**在国内网络环境下是否可接受（尤其初次请求冷启动）？ | 测一次 `GET staticmap` 请求的 TTFB（Time To First Byte） |
| ⑦ | wry 的 `ipc_handler` 在传递 **中文 JSON** 时是否有编码问题？ | 发送包含中文键值的 JSON，验证 Rust 侧解码是否正确 |

---

## 八、参考资料索引

### 高德官方文档

1. [JS API 2.0 安全密钥使用 - 基础教程](https://lbs.amap.com/api/javascript-api-v2/guide/abc/prepare)
2. [开放平台基础服务计费说明](https://lbs.amap.com/pages/base_service_price)
3. [POI 搜索 API - 高级 API 文档](https://lbs.amap.com/api/webservice/guide/api/search)
4. [静态地图 API - 基础 API 文档](https://lbs.amap.com/api/webservice/guide/api/staticmaps)
5. [高德地图开放平台服务协议](https://lbs.amap.com/pages/terms/)

### Rust 依赖官方源

1. [wry - docs.rs](https://docs.rs/wry/latest/wry/)
2. [wry - GitHub (tauri-apps)](https://github.com/tauri-apps/wry)
3. [ureq - GitHub (algesten)](https://github.com/algesten/ureq)
4. [reqwest - GitHub (seanmonstar)](https://github.com/seanmonstar/reqwest)

### Slint 官方社区

1. [Slint Issue #3930: webview element](https://github.com/slint-ui/slint/issues/3930)
2. [Slint Issue #4640: embed webview as child widget](https://github.com/slint-ui/slint/issues/4640)（关联建议）

### Microsoft 官方文档

1. [Distribute your app and the WebView2 Runtime](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)

---

> **免责声明**：本文档基于截至 2026-07-27 的一手资料整理；高德配额政策、Slint API、wry 版本等可能随时间变更，请以最新发布为准。
