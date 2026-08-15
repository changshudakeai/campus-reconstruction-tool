//! 工作现场恢复持久化 API
//!
//! 方案级工作区状态（已确认边界 + 自定义朝向 + 工作区步骤）与全局"上次
//! 打开方案"标记。写操作在状态变更点立即落库（安全检查点语义，工单
//! workspace-restore）：重启（含意外退出）后由应用流层读回并恢复现场。
//!
//! 本模块不改变评审终态语义：封账终态仍只写 `review_decisions`，本表只是
//! 未封账阶段的检查点，封账成功后由 F5 应用流层清空。

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use crate::database::Database;
use crate::entities::{timestamp_from_db, timestamp_to_db, PlanWorkspaceState};
use crate::error::{Error, Result};

/// 方案工作区状态读写（供 F3/应用流层调用）。
pub trait WorkspaceStateApi {
    /// UPSERT 一个方案的工作区快照（状态变更即落库）。
    fn save_plan_workspace_state(&mut self, state: &PlanWorkspaceState) -> Result<()>;

    /// 读取一个方案的工作区快照；无记录返回 None。
    fn load_plan_workspace_state(&self, plan_id: &str) -> Result<Option<PlanWorkspaceState>>;

    /// 清除一个方案的工作区快照（方案删除/数据损坏回退时调用）。
    fn clear_plan_workspace_state(&mut self, plan_id: &str) -> Result<()>;

    /// 记录全局"上次打开方案"（None 表示清除标记）。
    fn save_last_active_plan(&mut self, plan_id: Option<&str>) -> Result<()>;

    /// 读取全局"上次打开方案"；无标记返回 None。
    fn load_last_active_plan(&self) -> Result<Option<String>>;
}

impl WorkspaceStateApi for Database {
    fn save_plan_workspace_state(&mut self, state: &PlanWorkspaceState) -> Result<()> {
        let boundary_json =
            serde_json::to_string(&state.boundary_gcj02).map_err(Error::Serialization)?;
        let confirmed = if state.boundary_confirmed { 1 } else { 0 };
        self.conn.execute(
            "INSERT INTO plan_workspace_state
                (plan_id, boundary_name, boundary_gcj02, boundary_confirmed,
                 orientation_angle, active_step, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(plan_id) DO UPDATE SET
                boundary_name = excluded.boundary_name,
                boundary_gcj02 = excluded.boundary_gcj02,
                boundary_confirmed = excluded.boundary_confirmed,
                orientation_angle = excluded.orientation_angle,
                active_step = excluded.active_step,
                updated_at = excluded.updated_at",
            params![
                state.plan_id,
                state.boundary_name,
                boundary_json,
                confirmed,
                state.orientation_angle,
                state.active_step,
                timestamp_to_db(state.updated_at),
            ],
        )?;
        Ok(())
    }

    fn load_plan_workspace_state(&self, plan_id: &str) -> Result<Option<PlanWorkspaceState>> {
        let row = self
            .conn
            .query_row(
                "SELECT plan_id, boundary_name, boundary_gcj02, boundary_confirmed,
                        orientation_angle, active_step, updated_at
                 FROM plan_workspace_state WHERE plan_id = ?1",
                params![plan_id],
                |row| {
                    let confirmed: i64 = row.get(3)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        confirmed != 0,
                        row.get::<_, Option<f64>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((plan_id, boundary_name, boundary_json, confirmed, orientation, step, updated)) =
            row
        else {
            return Ok(None);
        };
        let boundary_gcj02: Vec<[f64; 2]> =
            serde_json::from_str(&boundary_json).map_err(Error::Serialization)?;
        Ok(Some(PlanWorkspaceState {
            plan_id,
            boundary_name,
            boundary_gcj02,
            boundary_confirmed: confirmed,
            orientation_angle: orientation,
            active_step: step.clamp(0, 4) as i32,
            updated_at: timestamp_from_db(&updated)?,
        }))
    }

    fn clear_plan_workspace_state(&mut self, plan_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plan_workspace_state WHERE plan_id = ?1",
            params![plan_id],
        )?;
        Ok(())
    }

    fn save_last_active_plan(&mut self, plan_id: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workspace_last_active (id, active_plan_id, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET
                active_plan_id = excluded.active_plan_id,
                updated_at = excluded.updated_at",
            params![plan_id, timestamp_to_db(Utc::now())],
        )?;
        Ok(())
    }

    fn load_last_active_plan(&self) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT active_plan_id FROM workspace_last_active WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|row| row.flatten())
            .map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_state_roundtrips_and_updates() {
        let mut db = Database::open_in_memory().unwrap();
        assert!(db.load_plan_workspace_state("plan-1").unwrap().is_none());

        let state = PlanWorkspaceState::new(
            "plan-1",
            "上海交通大学（闵行校区）",
            vec![[121.4, 31.0], [121.5, 31.0], [121.5, 31.1]],
            true,
            Some(90.0),
            2,
        );
        db.save_plan_workspace_state(&state).unwrap();
        let loaded = db.load_plan_workspace_state("plan-1").unwrap().unwrap();
        assert_eq!(loaded.plan_id, "plan-1");
        assert!(loaded.boundary_confirmed);
        assert_eq!(loaded.boundary_gcj02, state.boundary_gcj02);
        assert_eq!(loaded.orientation_angle, Some(90.0));
        assert_eq!(loaded.active_step, 2);

        // 再次保存覆盖（安全检查点语义：旧值被替换）
        let updated = PlanWorkspaceState::new("plan-1", "", Vec::new(), false, None, 0);
        db.save_plan_workspace_state(&updated).unwrap();
        let loaded = db.load_plan_workspace_state("plan-1").unwrap().unwrap();
        assert!(!loaded.boundary_confirmed);
        assert!(loaded.boundary_gcj02.is_empty());
        assert_eq!(loaded.orientation_angle, None);

        db.clear_plan_workspace_state("plan-1").unwrap();
        assert!(db.load_plan_workspace_state("plan-1").unwrap().is_none());
    }

    #[test]
    fn corrupt_boundary_json_is_a_read_error_not_silent_default() {
        let db = Database::open_in_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO plan_workspace_state
                    (plan_id, boundary_name, boundary_gcj02, boundary_confirmed)
                 VALUES ('plan-x', '坏数据', 'not-json', 1)",
                [],
            )
            .unwrap();
        let result = db.load_plan_workspace_state("plan-x").unwrap_err();
        assert!(matches!(result, Error::Serialization(_)));
    }

    #[test]
    fn last_active_plan_is_a_single_row_marker() {
        let mut db = Database::open_in_memory().unwrap();
        assert!(db.load_last_active_plan().unwrap().is_none());
        db.save_last_active_plan(Some("plan-a")).unwrap();
        db.save_last_active_plan(Some("plan-b")).unwrap();
        assert_eq!(
            db.load_last_active_plan().unwrap().as_deref(),
            Some("plan-b")
        );
        db.save_last_active_plan(None).unwrap();
        assert!(db.load_last_active_plan().unwrap().is_none());
    }
}
