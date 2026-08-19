//! 未封账评审草稿的安全检查点（B 工单 A.6/A.4）。
//!
//! F5 核心工作台保持"评审期间零写库"（缝 4 契约）；每次状态变更后由本模块
//! 把三态/勾选/激活类别写入 B2 草稿表，使意外退出后未封账决定仍可恢复；
//! 封账成功后清空草稿，封账终态以 review_decisions 为唯一权威。

use data_persistence::ReviewDraft;
use shared_domain_types::PlanId;

use super::workspace_boundary::WorkspaceProductionContext;

/// 把当前评审会话（未封账三态 + 勾选 + 激活类别）写成安全检查点。
/// 落库失败只告警——下一次状态变更会重试，且不阻塞评审操作。
pub(super) fn checkpoint_review(context: &WorkspaceProductionContext) {
    let Some(plan_id) = context.active_plan_id() else {
        return;
    };
    let injector = context.injector();
    let mut injector = injector.borrow_mut();
    let Some(workbench) = injector.review() else {
        return;
    };
    if injector.has_sealed_review_states(&plan_id).unwrap_or(true) {
        // 封账后终态以 review_decisions 为唯一权威，任何呈现层变更（如切换
        // 三态分组）都不得再写回未封账草稿；重进台的内存工作台不带封账
        // 标记，因此以 B2 终态为准。
        return;
    }
    let draft = ReviewDraft {
        plan_id: plan_id.clone(),
        active_category: workbench.active_category(),
        entries: workbench.draft_entries(),
    };
    if let Err(error) = injector.save_review_draft(&draft) {
        log::warn!("评审草稿检查点落库失败（plan={plan_id}）: {error}");
    }
}

/// 未封账草稿恢复：只有该方案没有封账终态时才把检查点对回内存；
/// 已封账（review_decisions 有记录）时忽略并清空残留草稿，保证
/// "封账后终态不可撤销"（A.4）。
pub(super) fn restore_review_draft_if_unsealed(
    context: &WorkspaceProductionContext,
    plan_id: &PlanId,
) {
    let plan_key = plan_id.to_string();
    let injector = context.injector();
    let mut injector = injector.borrow_mut();
    let sealed = injector.has_sealed_review_states(&plan_key).unwrap_or(true);
    let draft = injector.load_review_draft(&plan_key);
    let Ok(Some(draft)) = draft else {
        return;
    };
    if sealed {
        let _ = injector.clear_review_draft(&plan_key);
        return;
    }
    let Some(workbench) = injector.review_mut() else {
        return;
    };
    if let Err(error) = workbench.restore_draft_entries(&draft.entries) {
        log::warn!("评审草稿恢复失败（plan={plan_key}）: {error}");
        return;
    }
    workbench.set_active_category(draft.active_category);
}
