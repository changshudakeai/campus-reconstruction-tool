//! F3 实体：卡片三件套视图数据与进度描述
//!
//! 卡片三件套（ADR-0018）：方案名 / 进度描述 / 最后修改时间（相对表述）。
//! 所有用户可见文字走文本键（ADR-0005），本文件只产出键与参数，不硬编码文案。

use shared_domain_types::PlanId;

/// 校区视图（列表页顶部展示当前校区名，ADR-0006）
#[derive(Debug, Clone, PartialEq)]
pub struct CampusView {
    /// 校区 ID
    pub id: String,
    /// 校区名称
    pub name: String,
    /// 校区地址（ADR-0006：最近记录卡片与搜索结果展示；无地址时为空串）
    pub address: String,
    /// T05：锚点经度（GCJ-02），用于高德地图自动定位
    pub anchor_lng: f64,
    /// T05：锚点纬度（GCJ-02），用于高德地图自动定位
    pub anchor_lat: f64,
}

/// 方案卡片三件套（ADR-0018）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCardView {
    /// 方案 ID
    pub plan_id: String,
    /// 方案名
    pub name: String,
    /// 进度描述（文本键 + 参数，由 UI 层经 B6 解析成文案）
    pub progress: PlanProgress,
    /// 最后修改时间（RFC3339 文本；相对表述由 UI 层格式化）
    pub last_modified_at: String,
}

/// 方案进度状态（ADR-0010/0018：最早期状态如实显示"尚未确定范围"）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanProgress {
    /// 尚未确定范围（未画边界）
    BoundaryNotSet,
    /// 进行中："已完成 A → 下一步 B"
    InProgress {
        /// 已完成步骤的文本键列表
        completed_keys: Vec<String>,
        /// 下一步的文本键
        next_key: String,
    },
}

impl PlanProgress {
    /// 进度描述的文本键（UI 层用它查 B6 文本表）
    pub fn text_key(&self) -> &'static str {
        match self {
            PlanProgress::BoundaryNotSet => "plan.boundary_not_set",
            PlanProgress::InProgress { .. } => "plan.card_progress",
        }
    }
}

/// 回收站条目视图（F3 面向 UI 的纯数据，ADR-0018：方案名/删除时间/剩余保留时间）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItemView {
    /// 回收站条目 ID
    pub trash_id: String,
    /// 被删除的方案 ID
    pub plan_id: String,
    /// 被删除的方案名
    pub name: String,
    /// 原校区名
    pub campus_name: String,
    /// 删除时间（RFC3339 文本）
    pub deleted_at: String,
    /// 剩余保留天数（0～30，回收站保留 30 天）
    pub expires_in_days: i64,
}

/// 恢复方案的结果（ADR-0018 §五：重名时自动加"（恢复 N）"后缀，零交互）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredPlan {
    /// 恢复后的方案 ID
    pub plan_id: PlanId,
    /// 恢复后实际使用的方案名
    pub name: String,
}

/// 方案工作区上下文（S1-05）：打开方案时一次取得校区名、方案名与地图锚点。
/// 五个步骤顶部始终同时显示校区名与方案名（ADR-0027），锚点用于地图定位
/// （ADR-0008：地图直接定位到校区锚点）。
#[derive(Debug, Clone, PartialEq)]
pub struct PlanContextView {
    /// 方案 ID
    pub plan_id: String,
    /// 方案名
    pub plan_name: String,
    /// 所属校区 ID
    pub campus_id: String,
    /// 所属校区名
    pub campus_name: String,
    /// 校区锚点经度（GCJ-02）
    pub anchor_lng: f64,
    /// 校区锚点纬度（GCJ-02）
    pub anchor_lat: f64,
}

/// F3 一次返回校区选择与当前方案列表所需的全部正式状态。
#[derive(Debug, Clone, PartialEq)]
pub struct CampusPlanSnapshot {
    pub campuses: Vec<CampusView>,
    pub landing_campus: Option<CampusView>,
    pub plans: Vec<PlanCardView>,
}
