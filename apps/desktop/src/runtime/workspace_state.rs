//! 工作现场恢复：安全检查点读写（工单 workspace-restore）。
//!
//! 这些方法借道 F3 共享数据库连接，把方案级工作区快照（已确认边界/朝向/
//! 步骤）、全局"上次打开方案"标记与未封账评审草稿落库/读回；状态变更点由
//! 应用流层调用（workspace_adapter / review_adapter）。

use data_persistence::{
    PlanWorkspaceState, ReviewDecisionsApi, ReviewDraft, ReviewDraftApi, WorkspaceStateApi,
};

use crate::ViewModelInjector;

impl ViewModelInjector {
    /// 读取一个方案的工作区快照（已确认边界/朝向/步骤）。
    pub fn load_workspace_state(
        &self,
        plan_id: &str,
    ) -> data_persistence::Result<Option<PlanWorkspaceState>> {
        let database = self.projects.database();
        database.load_plan_workspace_state(plan_id)
    }

    /// 保存一个方案的工作区快照（状态变更即落库）。
    pub fn save_workspace_state(
        &mut self,
        state: &PlanWorkspaceState,
    ) -> data_persistence::Result<()> {
        let mut database = self.projects.database();
        database.save_plan_workspace_state(state)
    }

    /// 清除一个方案的工作区快照（方案删除/数据损坏回退时）。
    pub fn clear_workspace_state(&mut self, plan_id: &str) -> data_persistence::Result<()> {
        let mut database = self.projects.database();
        database.clear_plan_workspace_state(plan_id)
    }

    /// 读取全局"上次打开方案"。
    pub fn load_last_active_plan(&self) -> data_persistence::Result<Option<String>> {
        let database = self.projects.database();
        database.load_last_active_plan()
    }

    /// 记录全局"上次打开方案"（打开方案时调用；None 清除标记）。
    pub fn save_last_active_plan(&mut self, plan_id: Option<&str>) -> data_persistence::Result<()> {
        let mut database = self.projects.database();
        database.save_last_active_plan(plan_id)
    }

    /// 读取某方案未封账评审草稿（检查点）。
    pub fn load_review_draft(
        &self,
        plan_id: &str,
    ) -> data_persistence::Result<Option<ReviewDraft>> {
        let database = self.projects.database();
        database.load_review_draft(plan_id)
    }

    /// 保存某方案未封账评审草稿（每次状态变更后调用）。
    pub fn save_review_draft(&mut self, draft: &ReviewDraft) -> data_persistence::Result<()> {
        let mut database = self.projects.database();
        database.save_review_draft(draft)
    }

    /// 清空某方案未封账评审草稿（封账成功后调用；终态以 review_decisions 为准）。
    pub fn clear_review_draft(&mut self, plan_id: &str) -> data_persistence::Result<()> {
        let mut database = self.projects.database();
        database.clear_review_draft(plan_id)
    }

    /// 该方案是否已有封账终态（有则草稿不得覆盖终态）。
    pub fn has_sealed_review_states(&self, plan_id: &str) -> data_persistence::Result<bool> {
        let database = self.projects.database();
        let (pending, keep, remove) = database.count_review_states(plan_id)?;
        Ok(pending + keep + remove > 0)
    }
}
