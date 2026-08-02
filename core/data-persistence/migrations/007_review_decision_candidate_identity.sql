-- Candidate identity is the stable candidate_id alone; category is mutable projection data.
ALTER TABLE review_decisions RENAME TO review_decisions_with_category_identity;
DROP INDEX idx_review_decisions_state;

CREATE TABLE review_decisions (
    plan_id      TEXT NOT NULL,
    category     TEXT NOT NULL CHECK(category IN ('Building', 'Road', 'Water', 'Vegetation', 'Sports', 'Other')),
    candidate_id TEXT NOT NULL,
    review_state TEXT NOT NULL CHECK(review_state IN ('pending', 'keep', 'remove')),
    reviewer_id  TEXT,
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),

    PRIMARY KEY (plan_id, candidate_id)
);

INSERT INTO review_decisions (
    plan_id, category, candidate_id, review_state, reviewer_id, updated_at
)
SELECT plan_id, entity_type, entity_id, review_state, reviewer_id, updated_at
FROM (
    SELECT legacy.*,
           ROW_NUMBER() OVER (
               PARTITION BY plan_id, entity_id
               ORDER BY updated_at DESC, rowid DESC
           ) AS identity_rank
    FROM review_decisions_with_category_identity AS legacy
)
WHERE identity_rank = 1;

DROP TABLE review_decisions_with_category_identity;
CREATE INDEX idx_review_decisions_state ON review_decisions(review_state);
