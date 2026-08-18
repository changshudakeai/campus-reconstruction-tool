//! 评审地图回推同步（T39/T41：只绘制当前类别 + 当前页的可见集合）。
//!
//! 候选标注/定位/高亮经 map_webview 桥推送；本模块只做"当前可见集合"计算
//! 与脚本组装，业务状态仍由 F5 工作台持有。

use std::collections::HashMap;
use std::rc::Rc;

use review_workbench::{ConfidenceFilter, MapObjectView, WorkbenchView};

use super::review::{ReviewProductionAdapter, REVIEW_PAGE_SIZE};
use super::workspace_boundary::WorkspaceProductionContext;

/// T39：评审地图回推同步状态（T41：地图只绘制当前类别 + 当前页的可见集合）。
#[derive(Default)]
pub(super) struct ReviewMapSync {
    /// 已推送到 JS 的三态（candidate_id -> state 标识符；增量 diff 基准）。
    pub(super) pushed_states: HashMap<String, String>,
    /// 已推送的高亮候选（None = 无高亮）。
    pub(super) pushed_highlight: Option<String>,
    /// 已推送的可见集合身份（active_category_index, page_index,
    /// active_confidence_filter）。分类/翻页/筛选变化时触发一次全量重推
    /// （清旧 overlay + 画新集合，T51：筛选变化不得残留旧筛选标注）。
    pub(super) pushed_visible: Option<(usize, usize, ConfidenceFilter)>,
    /// 全量推送是否已排定（幂等，防止多次 map_ready 重复排定）。
    pub(super) full_push_scheduled: bool,
}

impl ReviewMapSync {
    /// 新 WebView 代开始时只清理“本代已应用”，期望状态仍由工作台持有并重放。
    pub(super) fn reset_applied(&mut self) {
        self.pushed_states.clear();
        self.pushed_highlight = None;
        self.pushed_visible = None;
        self.full_push_scheduled = false;
    }
}

/// 当前应绘制到评审地图的可见候选集合（当前类别 + 当前分页）。
pub(super) struct VisibleReviewSet {
    pub(super) active_category_index: usize,
    pub(super) page_index: usize,
    pub(super) active_filter: ConfidenceFilter,
    pub(super) objects: Vec<MapObjectView>,
}

/// 当前应绘制到评审地图的候选集合：当前类别 + 当前分页（每页 ≤60）。
///
/// 分页索引由工作区会话保存；此处同时把越界索引钳制回写，保证地图与列表
/// 永远绘制同一批候选，且全量推送不会把 12,000 条候选全部排进 JS 缓冲。
pub(super) fn visible_review_set_for(
    context: &WorkspaceProductionContext,
    view: &WorkbenchView,
    page_size: usize,
) -> VisibleReviewSet {
    let active_category_index = view
        .category_tabs
        .iter()
        .position(|tab| tab.active)
        .unwrap_or(0);
    let active_filter = view
        .confidence_filters
        .iter()
        .find(|chip| chip.active)
        .map(|chip| chip.filter)
        .unwrap_or(ConfidenceFilter::All);
    let page_total = view.cards.len().div_ceil(page_size).max(1);
    let mut session = context.session.borrow_mut();
    let page_index = session.review_page_index.min(page_total - 1);
    session.review_page_index = page_index;
    drop(session);

    let start = page_index * page_size;
    let end = (start + page_size).min(view.cards.len());
    let id_set: std::collections::HashSet<String> = view.cards[start..end]
        .iter()
        .map(|card| card.candidate_id.clone())
        .collect();
    let objects: Vec<MapObjectView> = view
        .map_objects
        .iter()
        .filter(|object| id_set.contains(&object.candidate_id))
        .cloned()
        .collect();
    VisibleReviewSet {
        active_category_index,
        page_index,
        active_filter,
        objects,
    }
}

/// 评审地图对象 → JS 载荷（与 B3 回推协议同构）。
pub(super) fn map_object_json(object: &review_workbench::MapObjectView) -> serde_json::Value {
    serde_json::json!({
        "candidate_id": object.candidate_id,
        "kind": object.shape_kind,
        "coordinates": object.shape_coordinates,
        "state": object.state.to_identifier(),
    })
}

/// 全量推送当前可见集合：`setReviewCandidates` 会清掉旧 overlay 再画新集合，
/// 因此分类/翻页时不会残留上一页/上一类别的标注。
pub(super) fn push_full_visible_sync(visible: &VisibleReviewSet, sync: &mut ReviewMapSync) -> bool {
    let objects: Vec<serde_json::Value> = visible.objects.iter().map(map_object_json).collect();
    let states: HashMap<String, String> = visible
        .objects
        .iter()
        .map(|object| {
            (
                object.candidate_id.clone(),
                object.state.to_identifier().to_string(),
            )
        })
        .collect();
    let highlight = visible
        .objects
        .iter()
        .find(|object| object.highlighted)
        .map(|object| object.candidate_id.clone());
    if crate::map_session::command(crate::map_session::MapCommand::ReviewReplace(objects))
        == crate::map_session::MapCommandResult::Unavailable
    {
        return false;
    }
    if let Some(candidate_id) = highlight.as_ref() {
        let _ = crate::map_session::command(crate::map_session::MapCommand::ReviewHighlight(Some(
            candidate_id.clone(),
        )));
    }
    sync.pushed_states = states;
    sync.pushed_highlight = highlight;
    sync.pushed_visible = Some((
        visible.active_category_index,
        visible.page_index,
        visible.active_filter,
    ));
    true
}

impl ReviewProductionAdapter {
    /// 把 F5 当前**可见集合**（当前类别 + 当前分页）的候选标注与高亮同步到
    /// 评审地图（T39/T41）。
    ///
    /// 分类/翻页变化触发一次 `setReviewCandidates`（清旧 overlay + 画新集合），
    /// 同页内的 state/highlight 变更才走 `updateReviewCandidate` /
    /// `highlightReviewCandidate` 增量路径。地图会话不接受命令时不更新本代已应用状态。
    pub(super) fn sync_review_map(&self) {
        let injector = self.context.injector();
        let injector = injector.borrow();
        let Some(workbench) = injector.review() else {
            return;
        };
        let view = workbench.view();
        let visible = visible_review_set_for(&self.context, &view, REVIEW_PAGE_SIZE);
        drop(injector);

        let mut sync = self.map_sync.borrow_mut();
        let full_needed = sync.pushed_visible
            != Some((
                visible.active_category_index,
                visible.page_index,
                visible.active_filter,
            ));
        if full_needed {
            let _ = push_full_visible_sync(&visible, &mut sync);
            sync.full_push_scheduled = false;
            return;
        }

        for object in &visible.objects {
            let state = object.state.to_identifier().to_string();
            if sync.pushed_states.get(&object.candidate_id) != Some(&state) {
                if crate::map_session::command(crate::map_session::MapCommand::ReviewUpdate(
                    map_object_json(object),
                )) == crate::map_session::MapCommandResult::Unavailable
                {
                    sync.full_push_scheduled = false;
                    return;
                }
                sync.pushed_states
                    .insert(object.candidate_id.clone(), state);
            }
        }
        let highlighted = visible
            .objects
            .iter()
            .find(|object| object.highlighted)
            .map(|object| object.candidate_id.clone());
        if sync.pushed_highlight != highlighted {
            let command = crate::map_session::MapCommand::ReviewHighlight(highlighted.clone());
            if crate::map_session::command(command)
                == crate::map_session::MapCommandResult::Unavailable
            {
                sync.full_push_scheduled = false;
                return;
            }
            sync.pushed_highlight = highlighted;
        }
        sync.full_push_scheduled = false;
    }

    /// map_ready 后的全量推送：在事件循环安全上下文排定一次，按当前可见集合
    /// 生成脚本（IPC 回调栈内不执行 WebView2 脚本调用，T35/T38 纪律）。
    pub(super) fn schedule_review_map_full_push(&self) {
        {
            let mut sync = self.map_sync.borrow_mut();
            if sync.full_push_scheduled {
                return;
            }
            sync.full_push_scheduled = true;
        }
        let context = self.context.clone();
        let map_sync = Rc::clone(&self.map_sync);
        slint::Timer::single_shot(std::time::Duration::from_millis(1), move || {
            let injector = context.injector();
            let injector = injector.borrow();
            let Some(workbench) = injector.review() else {
                map_sync.borrow_mut().full_push_scheduled = false;
                return;
            };
            let view = workbench.view();
            let visible = visible_review_set_for(&context, &view, REVIEW_PAGE_SIZE);
            drop(injector);
            let mut sync = map_sync.borrow_mut();
            let visible_key = (
                visible.active_category_index,
                visible.page_index,
                visible.active_filter,
            );
            if sync.pushed_visible == Some(visible_key) {
                // 该可见集合已由用户操作路径内联推送（测试无事件循环或生产
                // 1ms 窗口内用户先操作）：不再重复推一次。
                sync.full_push_scheduled = false;
                return;
            }
            let _ = push_full_visible_sync(&visible, &mut sync);
            sync.full_push_scheduled = false;
        });
    }

    /// 创建评审地图（候选不再内嵌 HTML；无密钥/锚点时跳过，抽屉仍可操作）。
    /// 候选标注在页面 map_ready 后由 Rust 按当前可见集合推送。
    pub(super) fn show_review_map(&self) {
        let Some((keys, anchor)) = self.context.map_credentials() else {
            return;
        };
        if keys.0.is_empty() {
            return;
        }
        let injector = self.context.injector();
        let injector = injector.borrow();
        let map_text_label = injector.l10n().t("review.map_text_toggle");
        drop(injector);
        self.map_sync.borrow_mut().reset_applied();
        let plan_id = self
            .context
            .active_plan_id()
            .unwrap_or_else(|| "__adopted_workspace__".to_owned());
        crate::map_session::present(
            self.context.window.clone(),
            crate::map_session::MapDisplayIntent::Review {
                plan_id,
                api_key: keys.0,
                security_key: keys.1,
                anchor,
                map_text_label,
            },
        );
    }
}
