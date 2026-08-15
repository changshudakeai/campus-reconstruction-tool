-- 工作现场恢复（工单：workspace-restore）：
-- 1) plan_workspace_state：方案级已确认边界 + 自定义朝向 + 工作区步骤（状态变更即落库）。
-- 2) workspace_last_active：全局"上次打开方案"单行标记（启动时恢复工作区用）。
-- 3) review_draft_states + review_draft_meta：未封账评审三态的安全检查点
--    （评审期间零写库契约不适用于本表——本表由应用流层在每次状态变更后写检查点；
--    封账成功后清空，封账终态仍以 review_decisions 为唯一权威）。
CREATE TABLE IF NOT EXISTS plan_workspace_state (
    plan_id            TEXT PRIMARY KEY,
    boundary_name      TEXT NOT NULL DEFAULT '',
    boundary_gcj02     TEXT NOT NULL DEFAULT '[]',
    boundary_confirmed INTEGER NOT NULL DEFAULT 0,
    orientation_angle  REAL,
    active_step        INTEGER NOT NULL DEFAULT 0,
    updated_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS workspace_last_active (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    active_plan_id  TEXT,
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS review_draft_states (
    plan_id       TEXT NOT NULL,
    candidate_id  TEXT NOT NULL,
    review_state  TEXT NOT NULL,
    selected      INTEGER NOT NULL DEFAULT 0,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (plan_id, candidate_id)
);

CREATE TABLE IF NOT EXISTS review_draft_meta (
    plan_id         TEXT PRIMARY KEY,
    active_category TEXT NOT NULL,
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

