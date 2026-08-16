# T19B-8 — 导出控制台（F9）+ .schem 产出 + 收尾基础设施

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

> **架构修订（2026-08-01）**：按 ADR-0037，S1 只能调用一次完整导出入口，壳不得实现封账业务或先调 F5、再调 F9。ADR-0039 明确 A1 只拥有采集流程，不得吸收导出；本单实施前必须另定独立导出应用流程模块。

**What to build**: 贯穿弹剧本的最后一步——导出确认（封账闸门）、非阻塞进度条、真实 .schem 文件 + manifest 产出；同时完成 T19B 系列全部收尾：气泡坐标定稿、dev-shortcut 验证、CODEOWNERS、public-api 快照补齐。注：原“导出完成”教程钩子已按 ADR-0028 作废，不在本单范围。

- **窗口契约**: Shell → 一次调用 F9 完整导出入口；F9 通过构造期注入的 `SealGate` 能力请求封账，并在自身边界后协调 B18 → B17 → B4
- **业务规则**: 封账铁律（ADR-0022）：确认后评审即封账不可再改；待定项如实报数、不拦截、不导出；失败可回滚重试
- **UI 决策法源**: ADR-0027（五步步骤条：导出占第五格；跳转规则 / 右上角四入口）+ ADR-0028（教程三泡：导出完成不再弹气泡，三处气泡坐标定稿改为 T19B-4/5/7 三处）；与本单描述冲突时以 ADR 为准

**Blocked by**: T19B-7（有评审终态才能封账导出）+ 独立导出应用流程模块的 ADR/模块目录登记

**Status**: blocked（等待独立导出应用流程模块决策；不得按旧壳编排方案实施）

## 🎯 验收标准

### 核心交付物：导出控制台
- [ ] 导出页 UI（`export_console.slint`）：
  - "导出地基"按钮 → F9 确认弹窗（类别汇总 + 封账后果 seal_notice + 待定报数 pending_notice）
  - 确认后触发 SealGate：F5 评审封账，非阻塞进度条（右上角，复用 F9 ProgressTracker）
  - 完成后显示导出清单（manifest：包含/缺失类别），失败走 B7 error 弹窗且可重试
- [ ] 真实产出验证：导出目录出现合法 .schem 文件 + manifest（B18 生成 → B17 用料表版本校验 → B4 落盘全链路真跑）
- [ ] 【换真门】把 `MockSealGate` 占位替换为非 S1 业务适配器实现的真 `SealGate`（Send+Sync，内部调用评审封账能力）；组合根只负责注入，封账链路端到端真跑，不得带着假门收工

### 教程收尾（债务④；债务③之三已按 ADR-0028 重排至 T19B-7）
- [ ] 三处气泡坐标从占位值调到实际坐标（T19B-4/5/7 三个钩子的气泡位置一并定稿，供负责人开发版审核）

### 基础设施收尾
- [ ] `cargo xtask dev-shortcut` 实测：桌面出现"校园复刻工具 - 开发版"快捷方式且双击能启动
- [ ] `.github/CODEOWNERS` 补充守卫 .lnk/.desktop 相关文件与 apps/desktop 关键配置
- [ ] desktop-shell public-api 快照入库（含 T19B-1 至 T19B-8 全部新增公开面）
- [ ] `.scratch` 各 T19B 工单与 T19-desktop-shell.md / STATUS.md 按真实状态更新收账

### 架构断言（CI 门禁必过）
- [ ] `cargo xtask arch` 通过：desktop-shell 不依赖 B12-B16；B18/B4 经 F9 适配层调用
- [ ] `cargo deny check bans` 无违规

## 📋 实施提示

- F9（T07）已实现封账确认 + 非阻塞进度 + 失败回滚；B18（T10）/B4（T09）/B17（T08）全链路已有测试夹具——本单要完成导出入口内部能力适配 + 端到端真跑，不是壳层业务组装
- 导出耗时操作不得冻结 UI：进度更新走 Slint invoke_from_event_loop
- ❌ 不要在壳里判断“待定项是否阻止导出”，也不要由壳先封账再导出；✅ F9 的确认视图已包含 pending_count，完整导出入口负责后续流程

✅ **收工自检清单**:
- [ ] `cargo check` 全 workspace 无报错
- [ ] 手动测试：小样本方案 → 导出 → 确认弹窗如实报数 → 进度条走完 → 拿到真实 .schem + manifest
- [ ] 破坏性测试：导出中断（如目标目录只读）→ 弹窗报错 → 重试成功
- [ ] `cargo xtask dev-shortcut` → 双击桌面快捷方式能启动
- [ ] 全套门禁：test / xtask tidy / arch / clippy -D warnings / fmt --check / machete / deny 四连
- [ ] git push 后 GitHub Actions conclusion 绿灯

---

## 负责人验收点（一句话）

导出的时候弹个框告诉你保留了多少栋楼、还有多少个待定，确认后右上角有进度条，走完后文件夹里躺着一个真的 .schem 文件；双击桌面图标就能打开软件。
