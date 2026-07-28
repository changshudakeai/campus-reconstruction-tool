-- MCRebuild V2 数据库迁移脚本 v3 — 校区锚点列
--
-- 依据：T05 需求 + ADR-0004(全局设置) + ADR-0007(方案隔离)
-- 版本号由迁移执行器统一写入 schema_migrations，本脚本不自行插入。
--
-- 功能：为 campuses 表添加坐标锚点字段，解决高德地图选校区后无法记住位置的问题

ALTER TABLE campuses ADD COLUMN anchor_lng REAL NOT NULL DEFAULT 116.397;
ALTER TABLE campuses ADD COLUMN anchor_lat REAL NOT NULL DEFAULT 39.916;
