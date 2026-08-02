-- ADR-0040：候选投影必须自带 F5 所需的展示属性。
-- 重建表以让新增列保持 NOT NULL 且没有占位默认值。
ALTER TABLE candidate_projections RENAME TO candidate_projections_without_display;
DROP INDEX idx_candidate_projections_batch_eligibility;

-- 记录哪些行确实由本次 v6 迁移生成展示属性。后续修复只能依赖该审计事实，
-- 不得通过展示内容反推来源，以免覆盖 v6 后由新 API 写入的合法数据。
CREATE TABLE candidate_display_backfill_audit (
    collection_batch_id TEXT NOT NULL,
    candidate_id TEXT NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
    repaired_at TEXT,
    PRIMARY KEY(collection_batch_id, candidate_id)
);

INSERT INTO candidate_display_backfill_audit (collection_batch_id, candidate_id)
SELECT collection_batch_id, candidate_id
FROM candidate_projections_without_display;

CREATE TABLE candidate_projections (
    collection_batch_id TEXT NOT NULL,
    candidate_id TEXT NOT NULL,
    plan_id TEXT NOT NULL,
    raw_observation_id TEXT NOT NULL,
    data_source_tag TEXT NOT NULL,
    source_entity_id TEXT NOT NULL,
    geometry_part_id TEXT NOT NULL,
    category TEXT NOT NULL CHECK(category IN ('Building', 'Road', 'Water', 'Vegetation', 'Sports', 'Other')),
    display_title TEXT NOT NULL,
    display_tags TEXT NOT NULL,
    geometry_kind TEXT NOT NULL CHECK(geometry_kind IN ('point', 'line_string', 'polygon')),
    normalized_geometry TEXT NOT NULL,
    validation TEXT NOT NULL CHECK(validation IN ('retained', 'repaired', 'rejected')),
    eligibility TEXT NOT NULL CHECK(eligibility IN ('reviewable', 'isolated')),
    isolation_reason TEXT,
    automatically_repaired INTEGER NOT NULL CHECK(automatically_repaired IN (0, 1)),
    missing_in_latest_batch INTEGER NOT NULL DEFAULT 0 CHECK(missing_in_latest_batch IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(collection_batch_id, candidate_id)
);

INSERT INTO candidate_projections (
    collection_batch_id,
    candidate_id,
    plan_id,
    raw_observation_id,
    data_source_tag,
    source_entity_id,
    geometry_part_id,
    category,
    display_title,
    display_tags,
    geometry_kind,
    normalized_geometry,
    validation,
    eligibility,
    isolation_reason,
    automatically_repaired,
    missing_in_latest_batch,
    created_at,
    updated_at
)
SELECT
    projection.collection_batch_id,
    projection.candidate_id,
    projection.plan_id,
    projection.raw_observation_id,
    projection.data_source_tag,
    projection.source_entity_id,
    projection.geometry_part_id,
    projection.category,
    COALESCE(
        (
            SELECT json_extract(observation.source_data, '$.tags.name')
            FROM raw_observations AS observation
            WHERE observation.id = projection.raw_observation_id
        ),
        projection.source_entity_id
    ),
    COALESCE(
        (
            SELECT json_group_array(
                json_array(
                    tag.key,
                    CASE tag.type
                        WHEN 'true' THEN 'true'
                        WHEN 'false' THEN 'false'
                        ELSE CAST(tag.value AS TEXT)
                    END
                )
            )
            FROM raw_observations AS observation,
                 json_each(json_extract(observation.source_data, '$.tags')) AS tag
            WHERE observation.id = projection.raw_observation_id
              AND tag.type IN ('text', 'integer', 'real', 'true', 'false')
        ),
        '[]'
    ),
    projection.geometry_kind,
    projection.normalized_geometry,
    projection.validation,
    projection.eligibility,
    projection.isolation_reason,
    projection.automatically_repaired,
    projection.missing_in_latest_batch,
    projection.created_at,
    projection.updated_at
FROM candidate_projections_without_display AS projection;

DROP TABLE candidate_projections_without_display;
CREATE INDEX idx_candidate_projections_batch_eligibility
    ON candidate_projections(collection_batch_id, eligibility);
