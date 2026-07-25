-- MCRebuild V2 数据库设计（草案，随决策讨论逐步扩充）
--
-- 依据：
--   ADR-0002 项目数据存储采用 SQLite
--   ADR-0004 语言与 Minecraft 版本为应用级全局设置
--   产品负责人确认的层级：应用 → 校区 → 方案
--
-- 约定：只收录已确认决策对应的表。工作流数据（地基、建筑、评审等）
-- 归属校区层还是方案层尚未定案，相应表结构待对应 ADR 落定后再加入。

PRAGMA foreign_keys = ON;

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

-- ============================================
-- 迁移版本记录
-- ============================================

CREATE TABLE schema_migrations (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL DEFAULT (datetime('now')),
    description TEXT
);

INSERT INTO schema_migrations (version, description)
VALUES (1, '初始草案：全局设置、校区、方案');

-- 待后续决策补充（勿在决策前擅自添加）：
--   * 校区级共享知识（建筑名称等，见既往决策"建筑名称是校园级共享知识"）
--   * 方案内工作流数据（边界、地块、建筑、评审）
--   * 回收站（v1.x 已有"校园级回收站保留 30 天"决策，V2 是否沿用待确认）
--   * v1.x JSON Schema2 迁移工具的中间表
