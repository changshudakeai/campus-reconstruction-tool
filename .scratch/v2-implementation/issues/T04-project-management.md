# T04 — F3 项目方案管理（校区/方案的增删改查 + 卡片三件套）

**What to build:** 老用户二次启动着陆于上次校区的方案列表页，顶部显示校区名 + "切换校区"入口；输入方案名即建立方案，卡片三件套展示名称/进度描述/最后修改时间，按最近修改倒序排列。

- **窗口契约**：缝 2（方案管理 ↔ 持久化）。F3 向 B2 要校区/方案的增删改查、回收站功能、"上次使用的校区"读写。
- **业务规则**：同一校区内方案名唯一；新建方案时一键复制本校区其他方案的边界；未画边界的方案显示"尚未确定范围"；删除进校园级回收站保留 30 天；回收站内立即永久删除需确认。
- **进度描述格式**："已完成 A → 下一步 B"。

**Blocked by:** T01, T02, T11（B2 数据持久化核心——CRUD 与回收站接口）, T03（文本外置）

**Status:** completed

- [x] data-persistence crate 立项并实现校区/方案表 CRUD + 迁移脚本（schema_migrations）
- [x] 回收站功能实现（进站/恢复/到期自动清理逻辑框架/确认后永久删除）
- [x] "上次使用的校区"读写 API（app_settings 表读取 + 写入）
- [x] F3 crate 立项并实现 ViewModel：列表查询、创建方案、改名、复制方案、删除
- [x] 同校区方案名冲突检测与错误处理
- [x] 卡片三件套字段映射（名称/进度/最后修改时间）+ 最近修改倒序
- [x] 复制方案功能实现（全量复制加"副本"后缀）
- [ ] 回收站 UI 占位（按钮存在即可，具体交互待定访谈）—— apps/desktop 尚未立项，无 UI 落点；由 UI 壳工单承接，F3 已提供 `list_trash`/`restore_plan`/`purge_plan_confirmed` 接口待接线
- [x] public-api 快照测试 + 初始快照入库

### 实施备注（2026-07-26）

- 新增 `core/project-management/`（F3 ViewModel 层，不写 SQL）；校区/方案 CRUD、
  回收站进出、app_settings 读写的 SQL 全部落在 B2 `data-persistence`新增的
  `projects.rs`（`CampusCrudApi`/`PlanCrudApi`/`AppSettingsApi` 三组 trait）。
- 删除方案 = 方案行保留 + trash 登记；`list_plans` 自动隐藏在站方案；永久删除
  时方案行清理、trash 行留审计痕迹；数据粮仓铁律不受影响。
- 进度描述：边界/采集状态数据尚未接入（T14/F4 范围），卡片当前如实显示
  "尚未确定范围"（ADR-0010）；`plan.card_progress` 模板已按"已完成 A → 下一步 B"入库。
- 顺带完成 T12 遗留 B6 国际化：tag-rules.json 的类别字段改为文本键
  `category_tkey`（collection.category_*），引擎/校验器/测试同步迁移。

---

## 负责人验收点（一句话）

打开软件能看到上次校区的方案列表，点"+"建一个方案后它出现在列表里，卡片上写着名字和"未完成"之类的状态。

