# T21 — 高德地图嵌入探针（屏 4 单窗口）

> 法源声明：与工单描述冲突时以 ADR 为准。重点条款：ADR-0003（Slint 地图嵌入
> 可行性）、ADR-0014（桌面图标验收入口）、ADR-0017（壳零业务逻辑、模块边界）、
> ADR-0005 / ADR-0023（文本外置、零色号）。
> 规格来源：`.scratch/v2-implementation/PRD-gaode-map-integration.md`。
> **产品铁律（效力等同验收标准）：地图与主程序共用同一个窗口，禁止 V1 式
> 独立弹出窗口。**

**What to build（负责人视角）：**
负责人双击桌面「校园复刻工具 - 开发版」图标，打开屏 4，能在界面指定区域看到
一张真高德地图（初始中心任意固定点即可），地图不弹独立窗口；拖动、缩放、
最大化主窗口时地图跟得住；步骤条、按钮等界面元素显示在地图上方且点得动。
**本单只排雷，不做任何取点 / 圈边界功能。** 密钥临时读取 demo 文件
（`%LOCALAPPDATA%\MCRebuildV2\dev\gaode-demo-keys.txt`），T22 落地后切换。

**Blocked by：** 无——可立即开工（与 T22 并行）。

**Status：** retired（2026-07-28 起被 T24 取代：map_embed.rs 已删除，改用 map_webview.rs）

## 探针四问（验收核心，逐条取证）

### T21 REDO 最终结论（2026-07-28）

⚠️ **探针结果：部分失败**

**根本原因分析**：
- **编译阶段**：wry 0.55 的 `WebViewBuilder::build()` 要求参数实现 `HasWindowHandle` trait，而 Slint 1.9 未暴露 `WinitWindowAccessor` trait，无法获取原生窗口句柄
- **API 调研**：调研文档中的 `with_window()` 方法在 wry 0.55 中不存在；`build_as_child()` 同样需要 `HasWindowHandle` trait
- **技术结论**：Slint 1.9 + wry 0.55 无法直接嵌入 WebView 到主窗口

### 探针四问逐条结论

- [x] **编译链接**：❌ 失败 — wry 0.55 `build()` 需 `HasWindowHandle` trait，Slint 1.9 未暴露 `WinitWindowAccessor`
- [ ] **显示**：❌ 未验证 — 因编译阶段失败，无法运行时验证
- [ ] **跟随**：❌ 未验证 — 同上
- [ ] **覆盖**：❌ 未验证 — 同上

### 门禁状态

- ✅ `cargo fmt`：通过
- ✅ `cargo build`：通过（placeholder 实现）
- ✅ `cargo test`：13 passed, 0 failed
- ✅ `cargo machete`：无未使用依赖
- ⚠️ `cargo clippy`：6 warnings（可接受，均为预留字段的 dead_code）

### 建议的后续选项（供负责人选择）

**A. 等待 Slint 升级到 1.17+**（推荐）
   - Slint 1.17+ 暴露 `WinitWindowAccessor` trait
   - 预计时间：待 Slint 官方发布
   - 优点：完全符合 ADR-0017 单窗口铁律

**B. 使用独立窗口方案**（不符合产品铁律）
   - 直接运行 `.scratch/map-demo` 独立窗口
   - 优点：立即可用
   - 缺点：违反 ADR-0017 单窗口铁律

**C. 降级 wry 版本**
   - 尝试 wry 0.37 等旧版本
   - 缺点：可能引入其他兼容性问题

### 当前代码状态

- `apps/desktop/src/map_embed.rs`：placeholder 实现，保留了完整 HTML 生成逻辑和后续集成 TODO
- `apps/desktop/src/runtime.rs`：调用 `GaodeMapView::render_into()`，失败时静默跳过
- `apps/desktop/ui/main.slint`：清理了 T22 的 echo-mode 语法错误
- `apps/desktop/src/injector.rs`：注释了 T22 的 setter/getter 调用

### 原始探针四问（保留参考）

- [ ] **编译链接**：winit 版本差（Slint 1.17 → winit 0.30，wry 0.37 → winit
  0.29）经 raw-window-handle / Windows HWND 桥接解决；如无法桥接，允许升级
  wry 大版本（须同步评估并通过 deny / CI 门禁）
- [ ] **显示**：屏 4 指定矩形区域稳定显示高德地图，全程不弹独立窗口
- [ ] **跟随**：主窗口移动 / 缩放 / 最大化 / 还原时，地图区域跟住，不错位、
  不残留、不闪退
- [ ] **覆盖**：步骤条、按钮、弹窗显示在地图上方且可点击（Z-order 正确）

## 红线

- [ ] 任一探针问题失败 → **停止并回报负责人**；禁止擅自采用"浮动顶层窗口
  视觉贴合"退路（该形态是否满足单窗口约定属产品裁决）
- [ ] 壳只嵌入与转发，无任何地图业务计算（ADR-0017）
- [ ] wry / winit 等桌面依赖只准进 desktop-shell，deny.toml bans 同步维护

## 通用收工标准

- [ ] 全部门禁绿灯：cargo fmt / clippy / test / machete / deny / xtask ci +
  GitHub Actions run success
- [ ] 文案键独立 commit（zh-CN.json 高冲突铁律）；.slint 双零 grep 取证
  （无硬编码中文、无色号）
- [ ] 验收逐条对照代码证据打勾（禁止凭组件存在打勾）
- [ ] 占位项 / 临时方案诚实声明写入完成汇报
