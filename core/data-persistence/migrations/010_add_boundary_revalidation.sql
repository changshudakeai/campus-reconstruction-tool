-- 边界变化后的本地候选资格重验证（D 工单）：
-- 1) 方案级"上次采集时使用的边界"指纹：确认边界时与它对比，不同才重验证；
-- 2) 评审决定作废标注：保留记录、不物理删除；
-- 3) 作废历史表：每次作废事件永久留痕（含被作废前的评审三态）。
CREATE TABLE IF NOT EXISTS plan_collection_boundary (
    plan_id     TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

ALTER TABLE review_decisions ADD COLUMN voided INTEGER NOT NULL DEFAULT 0;
ALTER TABLE review_decisions ADD COLUMN voided_reason TEXT;
ALTER TABLE review_decisions ADD COLUMN voided_at TEXT;

CREATE TABLE IF NOT EXISTS review_decision_invalidations (
    plan_id        TEXT NOT NULL,
    candidate_id   TEXT NOT NULL,
    previous_state TEXT NOT NULL,
    reason         TEXT NOT NULL,
    invalidated_at TEXT NOT NULL,
    PRIMARY KEY (plan_id, candidate_id, invalidated_at)
);
