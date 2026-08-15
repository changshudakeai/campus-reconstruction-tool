//! 评审地图回推同步（T39/T41：只绘制当前类别 + 当前页的可见集合）。
//!
//! 候选标注/定位/高亮经 map_webview 桥推送；本模块只做"当前可见集合"计算
//! 与脚本组装，业务状态仍由 F5 工作台持有。

use std::collections::HashMap;

use review_workbench::{MapObjectView, WorkbenchView};

use super::workspace_boundary::WorkspaceProductionContext;

/// T39：评审地图回推同步状态（T41：地图只绘制当前类别 + 当前页的可见集合）。
#[derive(Default)]
pub(super) struct ReviewMapSync {
    /// 已推送到 JS 的三态（candidate_id -> state 标识符；增量 diff 基准）。
    pub(super) pushed_states: HashMap<String, String>,
    /// 已推送的高亮候选（None = 无高亮）。
    pub(super) pushed_highlight: Option<String>,
    /// 已推送的可见集合身份（active_category_index, page_index）。
    /// 分类/翻页变化时触发一次全量重推（清旧 overlay + 画新集合）。
    pub(super) pushed_visible: Option<(usize, usize)>,
    /// 全量推送是否已排定（幂等，防止多次 map_ready 重复排定）。
    pub(super) full_push_scheduled: bool,
}

/// 当前应绘制到评审地图的可见候选集合（当前类别 + 当前分页）。
pub(super) struct VisibleReviewSet {
    pub(super) active_category_index: usize,
    pub(super) page_index: usize,
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

/// 全量推送脚本：一条 `setReviewCandidates(可见集合)` + 高亮命令（如有）。
/// 可见集合受当前类别 + 当前分页限制，因此不会把全量候选排进 JS 缓冲。
pub(super) fn full_push_scripts(
    objects: &[serde_json::Value],
    highlight: Option<&String>,
) -> Vec<String> {
    let json = serde_json::to_string(objects).unwrap_or_else(|_| "[]".to_string());
    let mut scripts = vec![format!("window.setReviewCandidates({json});")];
    if let Some(key) = highlight {
        let id = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
        scripts.push(format!("window.highlightReviewCandidate({id});"));
    }
    scripts
}

/// 全量推送当前可见集合：`setReviewCandidates` 会清掉旧 overlay 再画新集合，
/// 因此分类/翻页时不会残留上一页/上一类别的标注。
pub(super) fn push_full_visible_sync(visible: &VisibleReviewSet, sync: &mut ReviewMapSync) {
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
    let scripts = full_push_scripts(&objects, highlight.as_ref());
    for script in &scripts {
        push_review_script(script);
    }
    sync.pushed_states = states;
    sync.pushed_highlight = highlight;
    sync.pushed_visible = Some((visible.active_category_index, visible.page_index));
}

/// 评审地图回推命令：先计数（T39 验收观察），再执行（无 WebView 时为空操作）。
pub(super) fn push_review_script(script: &str) {
    crate::map_webview::note_review_push(script);
    crate::map_webview::evaluate_script(script);
}
