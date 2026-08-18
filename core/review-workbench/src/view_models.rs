//! 抽屉布局 ViewModel（ADR-0016：地图为主区 + 左侧抽屉）
//!
//! 全部是面向 UI 的纯数据：Slint 声明层只做绑定，不含业务逻辑。
//! 文案一律产出 B6 文本键（[`text_keys`]），由 UI 层经 `localization::t()`
//! 解析（ADR-0005，B6 国际化迁移）。

use shared_domain_types::{CandidateCategory, ReviewState};

use crate::command::ConfirmationRequest;
use crate::confidence::ConfidenceFilter;
use crate::suggestion::{SuggestionAction, SuggestionApplyRequest, SuggestionCategory};

/// 本 crate 产出的全部文本键（zh-CN.json 中必须逐条存在，由测试保证）
pub mod text_keys {
    /// 评审台标题
    pub const WORKBENCH_TITLE: &str = "review.workbench_title";
    /// 三态显示名：待定
    pub const STATE_PENDING: &str = "review.pending";
    /// 三态显示名：保留
    pub const STATE_KEEP: &str = "review.keep";
    /// 三态显示名：剔除
    pub const STATE_REJECT: &str = "review.reject";
    /// 信息面板状态行（含 `{state}` 占位符）
    pub const STATE_LABEL: &str = "review.state_label";
    /// 信息面板：来源行标签
    pub const INFO_SOURCE: &str = "review.info_source";
    /// 浮动按钮：全选
    pub const SELECT_ALL: &str = "review.select_all";
    /// 浮动按钮：取消全选
    pub const DESELECT_ALL: &str = "review.deselect_all";
    /// 批量操作按钮：改为保留
    pub const SET_KEEP: &str = "review.set_keep";
    /// 批量操作按钮：改为剔除
    pub const SET_REJECT: &str = "review.set_reject";
    /// 批量操作按钮：改回待定（恢复动作，ADR-0022）
    pub const SET_PENDING: &str = "review.set_pending";
    /// 批量剔除二次确认弹窗标题
    pub const BATCH_REJECT_CONFIRM_TITLE: &str = "review.batch_reject_confirm_title";
    /// 批量剔除二次确认弹窗正文（含 `{count}` 占位符）
    pub const BATCH_REJECT_CONFIRM_BODY: &str = "review.batch_reject_confirm_body";
    /// 已选计数（含 `{count}` 占位符）
    pub const SELECTED_COUNT: &str = "review.selected_count";
    /// 候选总数（含 `{count}` 占位符）
    pub const ITEM_COUNT: &str = "review.item_count";
    /// 待定计数（含 `{count}` 占位符）
    pub const PENDING_COUNT: &str = "review.pending_count";
    /// 信息面板：类别行标签
    pub const INFO_CATEGORY: &str = "review.info_category";
    /// 信息面板：标签与属性行标签
    pub const INFO_TAGS: &str = "review.info_tags";
    /// 确认按钮（弹窗共用，app 命名空间既有键）
    pub const CONFIRM_BUTTON: &str = "app.confirm_button";
    /// 取消按钮（弹窗共用，app 命名空间既有键）
    pub const CANCEL_BUTTON: &str = "app.cancel_button";
    /// 置信度筛选区标题
    pub const CONFIDENCE_FILTERS_LABEL: &str = "review.confidence_filters_label";
    /// 置信度筛选芯片标签行（含 `{label}`/`{count}` 占位符）
    pub const CONFIDENCE_FILTER_TAB: &str = "review.confidence_filter_tab";
    /// 置信度筛选：全部
    pub const FILTER_ALL: &str = "review.filter_all";
    /// 置信度筛选：高
    pub const FILTER_HIGH: &str = "review.filter_high";
    /// 置信度筛选：中
    pub const FILTER_MEDIUM: &str = "review.filter_medium";
    /// 置信度筛选：低
    pub const FILTER_LOW: &str = "review.filter_low";
    /// 一键应用建议按钮
    pub const APPLY_SUGGESTIONS: &str = "review.apply_suggestions";
    /// 撤销上一批按钮
    pub const UNDO_SUGGESTIONS: &str = "review.undo_suggestions";
    /// 建议类别：未命名
    pub const SUGGESTION_CATEGORY_UNNAMED: &str = "review.suggestion_category_unnamed";
    /// 建议类别：需要关注
    pub const SUGGESTION_CATEGORY_NEEDS_ATTENTION: &str =
        "review.suggestion_category_needs_attention";
    /// 建议类别：无需处理
    pub const SUGGESTION_CATEGORY_NO_ACTION: &str = "review.suggestion_category_no_action";
    /// 建议动作：建议保留
    pub const SUGGESTION_ACTION_KEEP: &str = "review.suggestion_action_keep";
    /// 建议动作：建议人工确认
    pub const SUGGESTION_ACTION_HUMAN_REVIEW: &str = "review.suggestion_action_human_review";
    /// 建议动作：建议剔除
    pub const SUGGESTION_ACTION_REMOVE: &str = "review.suggestion_action_remove";
    /// 建议理由：未命名
    pub const SUGGESTION_REASON_UNNAMED: &str = "review.suggestion_reason_unnamed";
    /// 建议理由：疑似重叠
    pub const SUGGESTION_REASON_OVERLAP: &str = "review.suggestion_reason_overlap";
    /// 建议理由：重复投影
    pub const SUGGESTION_REASON_EXACT_DUPLICATE: &str = "review.suggestion_reason_exact_duplicate";
    /// 建议理由：重复嫌疑
    pub const SUGGESTION_REASON_DUPLICATE_SUSPECT: &str =
        "review.suggestion_reason_duplicate_suspect";
    /// 建议理由：形状可疑
    pub const SUGGESTION_REASON_SUSPICIOUS_SHAPE: &str =
        "review.suggestion_reason_suspicious_shape";
    /// 建议理由：几何经自动修复
    pub const SUGGESTION_REASON_REPAIRED: &str = "review.suggestion_reason_repaired";
    /// 建议理由：缺少标签
    pub const SUGGESTION_REASON_SPARSE_TAGS: &str = "review.suggestion_reason_sparse_tags";
    /// 建议理由：缺少来源信息（来源类型输入）
    pub const SUGGESTION_REASON_MISSING_SOURCE: &str = "review.suggestion_reason_missing_source";
    /// 建议理由：携带隔离/警告理由（D）
    pub const SUGGESTION_REASON_ISOLATED: &str = "review.suggestion_reason_isolated";
    /// 建议理由：本次采集未找到
    pub const SUGGESTION_REASON_MISSING_LATEST: &str = "review.suggestion_reason_missing_latest";
    /// 建议理由：无需处理（建议保留）
    pub const SUGGESTION_REASON_KEEP: &str = "review.suggestion_reason_keep";
    /// 理由摘要：未命名
    pub const SUGGESTION_SUMMARY_UNNAMED: &str = "review.suggestion_summary_unnamed";
    /// 理由摘要：疑似重叠
    pub const SUGGESTION_SUMMARY_OVERLAP: &str = "review.suggestion_summary_overlap";
    /// 理由摘要：重复投影
    pub const SUGGESTION_SUMMARY_EXACT_DUPLICATE: &str =
        "review.suggestion_summary_exact_duplicate";
    /// 理由摘要：疑似重复
    pub const SUGGESTION_SUMMARY_DUPLICATE_SUSPECT: &str =
        "review.suggestion_summary_duplicate_suspect";
    /// 理由摘要：形状可疑
    pub const SUGGESTION_SUMMARY_SUSPICIOUS_SHAPE: &str =
        "review.suggestion_summary_suspicious_shape";
    /// 理由摘要：形状经修复
    pub const SUGGESTION_SUMMARY_REPAIRED: &str = "review.suggestion_summary_repaired";
    /// 理由摘要：缺少标签
    pub const SUGGESTION_SUMMARY_SPARSE_TAGS: &str = "review.suggestion_summary_sparse_tags";
    /// 理由摘要：缺少来源
    pub const SUGGESTION_SUMMARY_MISSING_SOURCE: &str = "review.suggestion_summary_missing_source";
    /// 理由摘要：携带隔离理由
    pub const SUGGESTION_SUMMARY_ISOLATED: &str = "review.suggestion_summary_isolated";
    /// 理由摘要：本次未找到
    pub const SUGGESTION_SUMMARY_MISSING_LATEST: &str = "review.suggestion_summary_missing_latest";
    /// 理由摘要：无需处理
    pub const SUGGESTION_SUMMARY_KEEP: &str = "review.suggestion_summary_keep";
    /// 应用建议确认弹窗标题
    pub const APPLY_SUGGEST_CONFIRM_TITLE: &str = "review.apply_suggest_confirm_title";
    /// 应用建议确认弹窗正文（含 {count}/{keep}/{remove} 占位符）
    pub const APPLY_SUGGEST_CONFIRM_BODY: &str = "review.apply_suggest_confirm_body";
    /// 确认弹窗"主要理由分布"标签
    pub const APPLY_SUGGEST_REASON_LABEL: &str = "review.apply_suggest_reason_label";
    /// 理由分布行（含 {label}/{count} 占位符）
    pub const APPLY_SUGGEST_REASON_LINE: &str = "review.apply_suggest_reason_line";
}

/// 三态 → 显示名文本键
pub(crate) fn state_text_key(state: ReviewState) -> &'static str {
    if state.is_keep() {
        text_keys::STATE_KEEP
    } else if state.is_remove() {
        text_keys::STATE_REJECT
    } else {
        text_keys::STATE_PENDING
    }
}

/// 类别 → 显示名文本键（collection 命名空间既有键，与 tag-rules.json 同源）
pub(crate) fn category_text_key(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "collection.category_building",
        CandidateCategory::Road => "collection.category_road",
        CandidateCategory::Water => "collection.category_water",
        CandidateCategory::Vegetation => "collection.category_vegetation",
        CandidateCategory::Sports => "collection.category_sports",
        // B1 枚举带 #[non_exhaustive]：未知新类别兜底进"其他"显示
        _ => "collection.category_other",
    }
}

/// 建议类别 → 显示名文本键
pub(crate) fn suggestion_category_text_key(category: SuggestionCategory) -> &'static str {
    match category {
        SuggestionCategory::Unnamed => text_keys::SUGGESTION_CATEGORY_UNNAMED,
        SuggestionCategory::NeedsAttention => text_keys::SUGGESTION_CATEGORY_NEEDS_ATTENTION,
        SuggestionCategory::NoActionNeeded => text_keys::SUGGESTION_CATEGORY_NO_ACTION,
    }
}

/// 建议动作 → 显示名文本键
pub(crate) fn suggestion_action_text_key(action: SuggestionAction) -> &'static str {
    match action {
        SuggestionAction::Keep => text_keys::SUGGESTION_ACTION_KEEP,
        SuggestionAction::HumanReview => text_keys::SUGGESTION_ACTION_HUMAN_REVIEW,
        SuggestionAction::Remove => text_keys::SUGGESTION_ACTION_REMOVE,
    }
}

/// 左栏顶部的类别标签页（"建筑 (120)"由 UI 层拼装：标签文本 + 计数）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CategoryTabView {
    /// 类别（六类别之一）
    pub category: CandidateCategory,
    /// 类别显示名文本键
    pub label_key: &'static str,
    /// 该类别的候选数
    pub count: usize,
    /// 是否为当前激活的抽屉
    pub active: bool,
}

/// 左栏候选卡片（当前激活类别下的一张）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CandidateCardView {
    /// 稳定候选 ID（点击卡片时回传给高亮入口）。
    pub candidate_id: String,
    /// 卡片标题（原始标签 name，无则实体 ID）
    pub title: String,
    /// 标题是否来自真实名称（false → UI 显示"未命名建筑 #id"）
    pub named: bool,
    /// 当前三态
    pub state: ReviewState,
    /// 三态显示名文本键
    pub state_key: &'static str,
    /// 复选框勾选状态
    pub selected: bool,
    /// 是否与地图联动高亮
    pub highlighted: bool,
    /// 轻量建议（无建议时不会出现；生成建议不改变三态）
    pub suggestion: Option<SuggestionCardView>,
}

/// 候选卡片上的建议呈现数据（类别/动作/理由全部为文本键，由 UI 层解析）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SuggestionCardView {
    /// 建议类别显示名文本键（未命名 / 需要关注 / 无需处理）。
    pub category_key: &'static str,
    /// 建议动作显示名文本键（建议保留 / 建议人工确认 / 建议剔除）。
    pub action_key: &'static str,
    /// 一句话可读理由文本键。
    pub reason_key: &'static str,
    /// 理由插值参数。
    pub reason_args: serde_json::Value,
}

/// 置信度筛选芯片（六类标签之外；全部/高/中/低单选，与类别组合）。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfidenceFilterView {
    /// 筛选器标识。
    pub filter: ConfidenceFilter,
    /// 筛选器显示名文本键。
    pub label_key: &'static str,
    /// 命中该筛选器的候选总数（跨类别）。
    pub count: usize,
    /// 是否已激活。
    pub active: bool,
}

/// 中间大地图上的一个对象（与卡片同源、双向高亮联动）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MapObjectView {
    /// 稳定候选 ID（点击地图对象时回传给高亮入口）。
    pub candidate_id: String,
    /// 类别（地图按类别着色）
    pub category: CandidateCategory,
    /// 当前三态（剔除态在地图上淡显）
    pub state: ReviewState,
    /// 几何种类（point / line_string / polygon；GCJ-02 工作坐标系）
    pub shape_kind: String,
    /// 几何坐标（GCJ-02；点 = `[lon, lat]`，线/面 = `[[lon, lat], ...]`）
    pub shape_coordinates: serde_json::Value,
    /// 来源标签（`data_source_tag`，详情面板"来源"行）
    pub source: String,
    /// 是否与卡片联动高亮
    pub highlighted: bool,
}

/// 右栏信息面板：当前高亮候选的详情（类别、标签、属性、状态）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InfoPanelView {
    /// 标题（候选名称）
    pub title: String,
    /// 标题是否来自真实名称（false → UI 显示"未命名建筑 #id"）
    pub named: bool,
    /// "类别"行标签文本键
    pub category_label_key: &'static str,
    /// 类别显示名文本键
    pub category_key: &'static str,
    /// "标签"行标签文本键
    pub tags_label_key: &'static str,
    /// 标签与属性（key=value 对）
    pub tags: Vec<(String, String)>,
    /// "来源"行标签文本键
    pub source_label_key: &'static str,
    /// 来源标签（data_source_tag）
    pub source: String,
    /// 状态行文本键（含 `{state}` 占位符）
    pub state_label_key: &'static str,
    /// 当前三态显示名文本键
    pub state_key: &'static str,
}

/// 评审台整体视图：抽屉布局一次性产出（Slint 绑定层直接消费）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorkbenchView {
    /// 评审台标题文本键
    pub title_key: &'static str,
    /// 左栏顶部类别标签页
    pub category_tabs: Vec<CategoryTabView>,
    /// 左栏卡片列表（当前激活类别）
    pub cards: Vec<CandidateCardView>,
    /// 中间大地图对象（全部类别，地图不分抽屉）
    pub map_objects: Vec<MapObjectView>,
    /// 右栏信息面板（无高亮候选时为 None）
    pub info_panel: Option<InfoPanelView>,
    /// 当前勾选数（"已选 {count} 项"插值用）
    pub selected_count: usize,
    /// 等待中的二次确认弹窗（批量剔除时出现，无数量门槛）
    pub pending_confirmation: Option<ConfirmationRequest>,
    /// 置信度筛选区标题文本键
    pub confidence_filters_label_key: &'static str,
    /// 置信度筛选芯片（固定顺序，单选，与类别组合）
    pub confidence_filters: Vec<ConfidenceFilterView>,
    /// 一键应用建议按钮文本键
    pub apply_suggestions_label_key: &'static str,
    /// 撤销上一批按钮文本键
    pub undo_suggestions_label_key: &'static str,
    /// 一键应用是否可用（未封账且存在可转为保留的高置信候选）
    pub apply_suggestions_enabled: bool,
    /// 是否存在可撤销的上一批（未封账时）
    pub undo_available: bool,
    /// 等待中的建议应用确认（对象数量 + 主要理由分布，验收 4）
    pub pending_suggestion_apply: Option<SuggestionApplyRequest>,
    /// 是否已封账（评审入口禁用信号）
    pub sealed: bool,
}

/// 封账前的导出请求汇总（缝 5：F5 → F9 递交的账本）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportSummary {
    /// 各类别保留数（类别汇总弹窗用；仅列出保留数 > 0 的类别）
    pub keep_by_category: Vec<(CandidateCategory, usize)>,
    /// 保留总数（唯一被导出的状态）
    pub keep_total: usize,
    /// 待定项数（"尚有 N 项待定，它们不会被导出"如实报数，ADR-0022）
    pub pending_count: usize,
    /// 剔除项数
    pub remove_count: usize,
}
