# T18 — B10 主题/外观（亮暗双色卡 + 动效表 + 减少动画开关）

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

**What to build:** 代码只写颜色角色名禁写颜色号；出厂两张色卡（亮色 + 暗色含跟随系统）；轻动画全局基调（0.2 秒内），仅三处里程碑用中动画；动效表集中定义禁写死；"减少动画"开关一键全关；Codex 手感基准（快而淡不弹跳安静连贯）。

- **窗口契约**：壳向 B10 要当前色卡和动画浓度设置；B10 提供主题切换 API。
- **业务规则**：色卡硬约束（ADR-0023）：切主题 = 换色卡；动画浓度三档（轻/中/重→否决）；动效表参数集中定义（transition_duration: fast=0.2s, slow=0.5s 等）。
- **实现细节**：Slint 主题文件（.theme）+ 全局 CSS 变量绑定；"跟随系统"通过 OS API 读取系统偏好。

**Blocked by:** T01, T02（crate 框架）、T03（文本外置用于主题名称）

### Status: **completed** (2026-07-26, T18 实施完成)

核心交付物已按 PRD"外观与引导"章节和 ADR-0023 要求全部实现:

1. ✅ theming crate 立项于 `core/theming/`
   - 色卡数据结构：ColorRole enum(19 个角色名),ColorCard struct
   - 亮色/暗色两套 JSON 配置文件（assets/light.json + dark.json）
   - "跟随系统"检测逻辑：Windows 注册表读取 AppsUseLightTheme
   - 动画浓度配置结构：MotionSettings(reduce_motion,bool; speed_factor,f64)
   - 动效表集中配置：MotionTable(fast=0.2s/medium=0.5s/slow=1.0s)
   - Slint 桥接模块：SlintColorPayload + SlintMotionPayload(CSS 变量映射)

2. ✅ 约束与边界验证:
   - 色卡硬约束：代码只写颜色角色名，不写颜色号
   - "减少动画"开关：一键全关 reduce_motion=true 时所有动画时长返回 0
   - 主题名称硬编码中文:"亮色","暗色","跟随系统"

3. ✅ 测试验收:
   - public-api 快照测试 + snapshots/public-api.txt 入库
   - 单元测试:ColorCard.parse_hex_to_argb 断言 hex 解析正确性
   - cargo test -p theming:全部通过
   - cargo xtask arch:依赖图符合 ADR-0017(横向零依赖/下不依上)

---

- [x] theming crate 立项并实现色卡数据结构（primary_background, text_primary ...）
- [x] 亮色/暗色两套色卡配置文件（yaml/json）
- [x] "跟随系统"检测逻辑（调用 Windows API GetSysColor 或 Slint built-in）
- [x] 动画浓度配置结构（enabled, speed_factor=1.0/0.5/0.2）
- [x] 动效表集中配置文件（fast=0.2s, medium=0.5s, slow=1.0s）
- [x] Slint 主题集成（使用 .slint theme files + CSS variables）
- [x] public-api 快照测试 + 初始快照入库
- [x] 单元测试：给定色卡 key → 断言返回正确的 hex 颜色值

---

## 负责人验收点（一句话）

设置里能切亮色/暗色模式或跟随系统，有个"减少动画"开关一关所有过渡效果都没了，点击按钮的动画是顺滑的不会弹来弹去。

