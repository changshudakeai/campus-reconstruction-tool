//! 三栏布局 ViewModel（ADR-0016：左卡片列表 + 中间大地图 + 右信息面板）
//!
//! 全部是面向 UI 的纯数据：Slint 声明层只做绑定，不含业务逻辑。
//! 文案一律产出 B6 文本键（[`text_keys`]），由 UI 层经 `localization::t()`
//! 解析（ADR-0005，B6 国际化迁移）。

use shared_domain_types::{CandidateCategory, ReviewState};

use crate::command::ConfirmationRequest;

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
    /// 暂停评审按钮（进度存临时文件，随时回来继续）
    pub const PAUSE: &str = "review.pause";
    /// 继续评审按钮（从临时文件恢复进度）
    pub const RESUME: &str = "review.resume";
    /// 确认按钮（弹窗共用，app 命名空间既有键）
    pub const CONFIRM_BUTTON: &str = "app.confirm_button";
    /// 取消按钮（弹窗共用，app 命名空间既有键）
    pub const CANCEL_BUTTON: &str = "app.cancel_button";
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
    /// 当前三态
    pub state: ReviewState,
    /// 三态显示名文本键
    pub state_key: &'static str,
    /// 复选框勾选状态
    pub selected: bool,
    /// 是否与地图联动高亮
    pub highlighted: bool,
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
    /// 是否与卡片联动高亮
    pub highlighted: bool,
}

/// 右栏信息面板：当前高亮候选的详情（类别、标签、属性、状态）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InfoPanelView {
    /// 标题（候选名称）
    pub title: String,
    /// "类别"行标签文本键
    pub category_label_key: &'static str,
    /// 类别显示名文本键
    pub category_key: &'static str,
    /// "标签"行标签文本键
    pub tags_label_key: &'static str,
    /// 标签与属性（key=value 对）
    pub tags: Vec<(String, String)>,
    /// 状态行文本键（含 `{state}` 占位符）
    pub state_label_key: &'static str,
    /// 当前三态显示名文本键
    pub state_key: &'static str,
}

/// 评审台整体视图：三栏布局一次性产出（Slint 绑定层直接消费）
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
    /// 勾选 ≥2 个时自动浮现“全选/取消全选”按钮（ADR-0016）。
    pub bulk_buttons_visible: bool,
    /// 等待中的二次确认弹窗（批量剔除 ≥5 项时出现）
    pub pending_confirmation: Option<ConfirmationRequest>,
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
