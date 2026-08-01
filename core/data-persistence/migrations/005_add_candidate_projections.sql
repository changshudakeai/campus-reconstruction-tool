-- ADR-0040：原始观测与可重建评审候选分层。
CREATE TABLE candidate_batches (
    id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('building', 'published')),
    created_at TEXT NOT NULL,
    published_at TEXT
);
CREATE INDEX idx_candidate_batches_plan ON candidate_batches(plan_id);

CREATE TABLE candidate_projections (
    collection_batch_id TEXT NOT NULL,
    candidate_id TEXT NOT NULL,
    plan_id TEXT NOT NULL,
    raw_observation_id TEXT NOT NULL,
    data_source_tag TEXT NOT NULL,
    source_entity_id TEXT NOT NULL,
    geometry_part_id TEXT NOT NULL,
    category TEXT NOT NULL CHECK(category IN ('Building', 'Road', 'Water', 'Vegetation', 'Sports', 'Other')),
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
CREATE INDEX idx_candidate_projections_batch_eligibility ON candidate_projections(collection_batch_id, eligibility);

CREATE TABLE current_candidate_batches (
    plan_id TEXT PRIMARY KEY,
    collection_batch_id TEXT NOT NULL
);
