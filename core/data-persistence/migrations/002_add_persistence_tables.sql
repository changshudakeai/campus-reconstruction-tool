-- MCRebuild V2 数据库迁移脚本 v2 — 原始观测表 + 评审终态表 + 回收站
--
-- 依据：T11（data-persistence crate）+ ADR-0002（SQLite 存储）
-- 版本号记录由迁移执行器统一写入 schema_migrations，本脚本不自行插入。
-- 升级内容：
--   - 原始观测表 raw_observations（数据粮仓，永不删除）
--   - 评审终态表 review_decisions（封账时批量写入）
--   - 校园级回收站表 trash（保留 30 天框架）

-- ============================================
-- 原始观测表（数据粮仓）
-- ============================================
-- 
-- 采集爬回的原始信息永久存库不删；评审后、导出后都不删除。
-- 用于将来"精细建筑模式"的原料。

CREATE TABLE raw_observations (
    id             TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),  -- UUID v4
    plan_id        TEXT NOT NULL,                                          -- 方案 ID
    entity_type    TEXT NOT NULL CHECK(entity_type IN ('Building', 'Road', 'Water', 'Vegetation', 'Sports', 'Other')),  -- 六类别
    entity_id      TEXT NOT NULL,                                          -- 真实世界对象 ID（来自数据源）
    source_data    TEXT NOT NULL,                                          -- JSON 格式的原始标签 + 属性
    data_source_tag TEXT NOT NULL,                                         -- 数据来源标识（如 "gaode", "overpass"）
    digest         TEXT NOT NULL,                                          -- SHA256 内容指纹（用于增量刷新检测）
    created_at     TEXT NOT NULL DEFAULT (datetime('now')),                -- ISO8601 时间戳
    updated_at     TEXT NOT NULL DEFAULT (datetime('now'))                 -- 最后更新时间
);

CREATE INDEX idx_raw_observations_plan ON raw_observations(plan_id);
-- 同一方案内同一实体唯一：重复采集时按 digest 增量刷新（UPSERT），而非叠加重复行
CREATE UNIQUE INDEX idx_raw_observations_entity ON raw_observations(plan_id, entity_type, entity_id);
CREATE INDEX idx_raw_observations_digest ON raw_observations(digest);

-- ============================================
-- 评审终态表（review_decisions）
-- ============================================
--
-- 仅在导出封账时批量写入；评审期间零写库。
-- 主键组合：plan_id + entity_type + entity_id
-- plan_id 与原始观测表一样是弱引用：方案删除的清理策略由后续
-- 方案管理 API 统一处理（原始观测永不删）。

CREATE TABLE review_decisions (
    plan_id        TEXT NOT NULL,
    entity_type    TEXT NOT NULL CHECK(entity_type IN ('Building', 'Road', 'Water', 'Vegetation', 'Sports', 'Other')),
    entity_id      TEXT NOT NULL,
    review_state   TEXT NOT NULL CHECK(review_state IN ('pending', 'keep', 'remove')),  -- 三态
    reviewer_id    TEXT,                                                    -- 可选 reviewer ID（当前版本可填 "system" 或留空）
    updated_at     TEXT NOT NULL DEFAULT (datetime('now')),                -- 最后修改时间

    PRIMARY KEY (plan_id, entity_type, entity_id)
);

CREATE INDEX idx_review_decisions_state ON review_decisions(review_state);

-- ============================================
-- 校园级回收站（trash）
-- ============================================
--
-- 被删除的方案进回收站保留 30 天；支持恢复和确认永久删除。
-- 注意：实体（候选）不留回收站（剔除即永久），只管理方案级删除。
-- plan_id 不设外键级联：方案永久删除后回收站记录作为审计痕迹保留。

CREATE TABLE trash (
    id                     TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),  -- UUID v4
    campus_id              TEXT NOT NULL,                                         -- 校区 ID（被删除实体所属校区）
    plan_id                TEXT NOT NULL,                                         -- 方案 ID（审计引用，不级联）
    entity_type            TEXT NOT NULL,                                         -- 被删除实体类型（当前仅 'plan'，预留扩展）
    entity_id              TEXT NOT NULL,                                         -- 被删除实体 ID
    deleted_at             TEXT NOT NULL DEFAULT (datetime('now')),               -- 删除时间
    deleted_by             TEXT,                                                  -- 删除人（可选）
    restored_at            TEXT,                                                  -- NULL 表示未恢复
    permanently_deleted_at TEXT                                                   -- NULL 表示未永久删除
);

CREATE INDEX idx_trash_campus ON trash(campus_id);
CREATE INDEX idx_trash_deleted_at ON trash(deleted_at);
