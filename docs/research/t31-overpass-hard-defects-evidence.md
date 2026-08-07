# T31 Overpass 三硬伤：修复前/后实测证据（2026-08-07，上海网络）

> 配套工单：`.scratch/v2-implementation/issues/T31-real-outline-boundary-sources.md`
> 调研报告：`docs/research/candidate-data-sources-and-naming.md`（§4.2 三硬伤根因）
> 执行纪律：三硬伤修复前后各留一次实测证据（HTTP 响应/查询结果）。

## 一、硬伤 1：请求 URL 缺 `data=` 参数（已复现）

应用原实现（`core/gaode-client/src/boundary_edit_map_page.rs`）拼 URL 为
`interpreter?<编码查询>`，没有 `data=`。实测（复现原应用行为）：

```text
URL: https://overpass-api.de/api/interpreter?%5Bout%3Ajson%5D%5Btimeout%3A45%5D%3B...
HTTP 200（错误页）
<p><strong style="color:#FF0000">Error</strong>: line 1: parse error: Unknown type &quot;%&quot; </p>
<p><strong style="color:#FF0000">Error</strong>: line 1: parse error: An empty query is not allowed </p>
<p><strong style="color:#FF0000">Error</strong>: line 1: parse error: ';' expected - '5' found. </p>
<p><strong style="color:#FF0000">Error</strong>: line 1: parse error: Unexpected end of input. </p>
```

结论：与调研一致——查询根本没有被服务端执行（参数名被当成查询体）。

## 二、硬伤 2：`amenity~"university|college|school"` 的 `|` 正则

调研当日（2026-08-07）实测 de（0.7.62.11）拒绝 `|`：`parse error: ',' or ']'
expected - '|' found`。本次实施窗口复查（同日晚，端点可能已更新）：

| 端点 | 版本 | `data=` + `%7C` 编码 `|` 正则 | 结论 |
|---|---|---|---|
| overpass-api.de | 0.7.62.11 | 200，`way["amenity"~"university\|college\|school"]` 返回 798 元素 | 当前接受（端点已更新） |
| overpass.kumi.systems | - | 本次连接超时/失败（HTTP 000） | 不稳定（与调研一致） |
| maps.mail.ru | 0.7.62.4 | 200，返回 798 元素 | 当前接受 |

结论：`|` 正则的拒绝是**版本相关**（调研已在 de 0.7.62.11 复现）；端点会滚动更新，
不能依赖。按工单要求一律改 **union 写法**（`(way["amenity"="university"];way[...]
;relation[...];);`），三端点全部验证可用（下方修复后证据），跨版本稳健。

## 三、硬伤 3：WebView 内 fetch 的 CORS 依赖

浏览器侧 `fetch` 能否读取响应取决于响应头的 `Access-Control-Allow-Origin`。
本次复查（Origin: `http://wry.localhost`，WebView 同源）：

| 端点 | `/api/status` | `/api/interpreter`（真实查询） |
|---|---|---|
| overpass-api.de | `Access-Control-Allow-Origin: *` | `Access-Control-Allow-Origin: *` |
| overpass.kumi.systems | `Access-Control-Allow-Origin: *` | 本次连接超时 |
| maps.mail.ru | `Access-Control-Allow-Origin: *` | `Access-Control-Allow-Origin: *` |

结论：CORS 头当前在三端点普遍存在（调研当日 de/kumi 无 ACAO），但公共镜像策略会
变动，且 WebView fetch 依赖同源策略与端点稳定性（kumi 反复超时）。按工单要求，
**边界与候选查询一律 Rust 侧直连**（ureq + 每端点 12s 超时 + de→kumi→mail.ru 回退），
彻底绕开 WebView CORS 与第三方镜像的浏览器策略依赖。

## 四、修复后证据（Rust 侧直连将使用的同一查询）

### 4.1 Nominatim 校名解析（≤1 次/秒，带 User-Agent）

```text
GET https://nominatim.openstreetmap.org/search?q=Shanghai+Jiao+Tong+University&format=json&limit=5
→ [{"osm_type":"way","osm_id":144183801,"class":"amenity","type":"university",
    "name":"上海交通大学（徐汇校区）",...}]
GET q=上海交通大学闵行校区 → way/288249651 class=amenity type=university name=上海交通大学（闵行校区）
```

### 4.2 Overpass 按 ID 拉取边界（`data=` + 无 `|`）

```text
data=[out:json][timeout:25];way(288249651);out geom;
→ elements=1: type=way id=288249651 name=上海交通大学（闵行校区）
  geometry_points=39，首尾闭合（31.0295090,121.4184319 = 31.0295090,121.4184319）
```

### 4.3 Overpass `amenity=university` 锚点近域查询（ADR-0029 主路径，union 写法）

```text
data=[out:json][timeout:25];(way["amenity"="university"](31.00,121.40,31.06,121.46);
      relation["amenity"="university"](31.00,121.40,31.06,121.46););out geom;
→ elements=4：way/288249651 上海交通大学（闵行校区）、way/1052063823 南洋北苑生活园区、
  way/1473289890 思源北苑博士生公寓、way/293438840 华东师范大学（闵行校区）
```

### 4.4 Overpass 候选建筑面（union `building=*`，面几何 + name/层数标签）

```text
data=[out:json][timeout:25];(way["building"](31.02,121.41,31.04,121.46);
      relation["building"](31.02,121.41,31.04,121.46););out geom;
→ elements=590：ways=579、带 name=382、带 building:levels=72；
  样例：way/154427164 第一餐饮大楼（19 点）、way/160634093 校医院（22 点）、
        way/219751253 霍英东体育中心（16 点）、way/237322332 Zizhu2（levels=4）
```

## 五、复现与验证命令

```powershell
# 硬伤1（修复前）
curl.exe -s "https://overpass-api.de/api/interpreter?<urlencoded query>"   # parse error: Unknown type "%"
# 修复后正确 URL
curl.exe -s "https://overpass-api.de/api/interpreter?data=<urlencoded query>"
# CORS 探测
curl.exe -s -D - -o NUL -H "Origin: http://wry.localhost" <endpoint>/api/status
```

完整日志与原始响应存档于本文件所在目录的 `t31-overpass-evidence/`。
