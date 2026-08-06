-- MCRebuild V2 数据库迁移脚本 v6 — 校区高德地点标识
--
-- 依据：ADR-0008（校区层持久化字段——高德地点标识、名称、地址、坐标锚点）
--       T30 D-3（重复点选同一真实学校只切换、不重复建，须按 POI 标识判重）
-- 版本号由迁移执行器统一写入 schema_migrations，本脚本不自行插入。
--
-- 功能：campuses 表新增 poi_id 列，记录校区来自哪个高德地点；
--       旧开发数据库（无 poi_id）按空串处理，不影响既有校区读取。

ALTER TABLE campuses ADD COLUMN poi_id TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_campuses_poi_id ON campuses(poi_id);
