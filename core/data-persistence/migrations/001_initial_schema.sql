-- MCRebuild V2 数据库迁移脚本 v1 — 初始 schema
--
-- 依据 sqlite/schemas/v1.sql 草案（ADR-0002/0004）。
-- 版本号记录由迁移执行器统一写入 schema_migrations，本脚本不自行插入。

-- ============================================
-- 应用级全局设置（ADR-0004）
-- ============================================

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 当前支持范围：语言仅中文；游戏版本仅 26.1.2（ADR-0004）
INSERT INTO app_settings (key, value) VALUES
    ('language', 'zh-CN'),
    ('minecraft_version', '26.1.2'),
    ('first_run_completed', 'false');

-- ============================================
-- 校区（顶层容器）
-- ============================================

CREATE TABLE campuses (
    id         TEXT PRIMARY KEY,               -- UUID
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================
-- 方案（挂在校区之下；一个校区可有多个方案）
-- ============================================

CREATE TABLE plans (
    id         TEXT PRIMARY KEY,               -- UUID
    campus_id  TEXT NOT NULL REFERENCES campuses(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_plans_campus ON plans(campus_id);
