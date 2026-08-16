# ⚠️ T19B-5 已拆分为 5A + 5B（请勿实施本单）

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

**状态变更**: ready → split

---

## 📂 新工单位置（严格串行执行）

| # | 新工单文件 | 一句话交付 | 状态 |
|---|-----------|-----------|------|
| 5A | [`T19B-5A-debt-and-infra.md`](./T19B-5A-debt-and-infra.md) | 色卡基建 + 通用对话框 + ADR-0010/0018 违规修复 | ready-for-agent |
| 5B | [`T19B-5B-plan-workspace.md`](./T19B-5B-plan-workspace.md) | 单击打开 + 五格步骤条 + 圈边界 + 定朝向 + stepper_intro 钩子 | blocked（等 5A 绿灯） |

---

## ℹ️ 历史参考（不再维护，法源见上方新工单表头 UI 决策法源行）

旧版原始需求保留于此便于追溯，但**验收标准以新工单逐条对照的 ADR 原文为准**。

**What to build**: 方案建好后的第一个实质步骤——在地图上圈画校区边界（多边形），然后设定朝向（必选步骤），完成后方案卡片进度推进到"下一步：采集"。

- **窗口契约**: 缝 7（地图服务，Shell ↔ F 模块 ↔ B5/B3 中转）；Shell 展示边界绘制 ViewModel，几何计算全部在 B5
- **业务规则**: 朝向是必选步骤（决策记忆）；边界属于方案而非校区（project_environment 决策）；修改朝向触发重算提示（collection.orientation_recalc_notice 文案已在库）
- **UI 决策法源**: ADR-0027（五步步骤条：圈边界与定朝向各占一格，本单实施两个步骤页；回跳改动弹窗确认、前跳上锁）+ ADR-0028（教程三泡，本单新增 stepper_intro 钩子）；与本单描述冲突时以 ADR 为准

**Blocked by**: T19B-4（方案列表页可跳转进入本页）

**Status**: ready-for-agent

## 🎯 验收标准

### 核心交付物
- [ ] 边界绘制页 UI（`boundary_edit.slint`）：
  - 地图视图区域（经功能模块中转调用 B3 高德地图能力，Shell 不直接依赖 B3/B12-B16）
  - 多边形圈画交互：点击加点、可撤销上一点、闭合完成
  - "确认边界"按钮（几何合法性校验走 B5，非法时经 B7 弹窗报 error.invalid_geometry）
- [ ] 朝向设定 UI：
  - 两点定朝向交互（或方位角选择器，以 B5 已有 API 为准）
  - 朝向为必选：未设定朝向不能进入采集步骤
  - 已有生成数据时修改朝向 → 弹出重算影响提示（复用 collection.orientation_recalc_notice）
- [ ] 完成后写回方案状态，方案卡片进度文案更新为“已完成：边界朝向 → 下一步：采集”

### 教程钩子（债务③-2，按 ADR-0028 重排至本单）
- [ ] F2 “stepper_intro”钩子：首次进入方案内、步骤条首次亮相时调用 F2 钩子接口，弹“顶上五格就是全部流程”气泡（坐标占位，定稿归 T19B-8）；注意 F2 提示点枚举需先按 ADR-0028 调整（拆 collection_completed/export_completed，加 stepper_intro/review_intro）

### 文案与国际化
- [ ] 新增文本键统一走 l10n.t()，补充 zh-CN.json（boundary.* 前缀），.slint 零硬编码中文
- [ ] 复用已有键：collection.boundary_step / collection.orientation_step / error.invalid_geometry

### 架构断言（CI 门禁必过）
- [ ] `cargo xtask arch` 通过：shell 不依赖 B12-B16，地图能力经功能模块中转
- [ ] `cargo deny check bans` 无违规

## 📋 实施提示

- B5 foundation-mode（T14）已有坐标系转换与几何校验，Shell 只做展示与事件传递
- 边界数据持久化走既有方案存储接口（F3/B2），Shell 不写 SQL
- ❌ 不要在 Shell 里做多边形闭合判断；✅ B5 提供 `is_valid_polygon()` 类接口

✅ **收工自检清单**:
- [ ] `cargo check` 全 workspace 无报错
- [ ] 手动测试：新建方案 → 圈边界 → 设朝向 → 返回列表看到进度推进
- [ ] 破坏性测试：画自相交多边形 → 弹窗报错而非崩溃
- [ ] 全套门禁：test / xtask tidy / arch / clippy -D warnings / fmt --check / machete / deny 四连
- [ ] git push 后 GitHub Actions conclusion 绿灯

---

## 负责人验收点（一句话）

在地图上点几个点圈出校园范围，设好朝向后回到方案列表，卡片上显示"下一步：采集"。
