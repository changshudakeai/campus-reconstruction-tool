//! 采集进度反馈 UI 占位（纯数据视图 + B6 文本键）。
//!
//! 负责人验收点："正在从地图平台拉数据……完成了 N 个对象"。
//! 本模块只产出可绑定的进度数据与文本键，不渲染界面（渲染归壳层，
//! ADR-0005：代码零硬编码文案，带变量文案用占位符插值）。

use shared_domain_types::{CandidateCategory, CollectionJobStatus};

use crate::pipeline::CollectionReport;

/// 采集处理中阶段（T36：S1 抽屉显示“拉取数据 / 补名 / 写库”）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionStage {
    /// 从数据源拉取原始对象（Overpass 等）
    FetchingData,
    /// 缺名关键建筑补名（regeo 有界并发）
    Naming,
    /// 原始观察落库 / 候选投影发布
    Writing,
    /// 采集完成
    Finished,
}

/// 阶段 → B6 文本键（zh-CN.json collection 段）
pub fn stage_text_key(stage: CollectionStage) -> &'static str {
    match stage {
        CollectionStage::FetchingData => "collection.stage_fetching",
        CollectionStage::Naming => "collection.stage_naming",
        CollectionStage::Writing => "collection.stage_writing",
        CollectionStage::Finished => "collection.stage_finished",
    }
}

/// 阶段上报监听器（A1 注册后把阶段事实写入自己的 Fetching 状态）。
pub type StageListener = Box<dyn Fn(CollectionStage) + Send + Sync>;

/// 本 crate 产出的全部 B6 文本键（zh-CN.json collection 段）
pub mod text_keys {
    /// 进度面板标题
    pub const PROGRESS_TITLE: &str = "collection.progress_title";
    /// 拉取中提示（"正在从地图平台拉数据……"）
    pub const PROGRESS_FETCHING: &str = "collection.progress_fetching";
    /// 完成提示（占位符 {count}）
    pub const PROGRESS_DONE: &str = "collection.progress_done";
    /// 差异：新增
    pub const DIFF_ADDED: &str = "collection.diff_added";
    /// 差异：更新
    pub const DIFF_UPDATED: &str = "collection.diff_updated";
    /// 差异：未变
    pub const DIFF_UNCHANGED: &str = "collection.diff_unchanged";
    /// 差异汇总（占位符 {added}/{updated}/{unchanged}）
    pub const DIFF_SUMMARY: &str = "collection.diff_summary";
    /// 高德数据源显示名
    pub const SOURCE_GAODE: &str = "collection.source_gaode";
}

/// 六类别固定顺序（进度条逐类展示；B1 枚举带 non_exhaustive，此处锚定顺序）
pub const ALL_CATEGORIES: [CandidateCategory; 6] = [
    CandidateCategory::Building,
    CandidateCategory::Road,
    CandidateCategory::Water,
    CandidateCategory::Vegetation,
    CandidateCategory::Sports,
    CandidateCategory::Other,
];

/// 类别 → B6 文本键（zh-CN.json 既有条目，与 B13 映射表 category_tkey 同源）
pub fn category_text_key(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "collection.category_building",
        CandidateCategory::Road => "collection.category_road",
        CandidateCategory::Water => "collection.category_water",
        CandidateCategory::Vegetation => "collection.category_vegetation",
        CandidateCategory::Sports => "collection.category_sports",
        _ => "collection.category_other",
    }
}

/// 单个类别的进度行（进度条"当前类别进度"的一格）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryProgress {
    /// 类别
    pub category: CandidateCategory,
    /// 类别显示名的文本键
    pub label_key: &'static str,
    /// 已采集对象数
    pub collected: usize,
    /// 该类别的采集状态
    pub status: CollectionJobStatus,
}

/// 采集进度视图（UI 占位：进度条 + 逐类别明细 + 状态文案键）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionProgressView {
    /// 面板标题文本键
    pub title_key: &'static str,
    /// 当前状态文案键（拉取中 / 完成）
    pub status_key: &'static str,
    /// 当前阶段（T36：拉取数据 / 补名 / 写库 / 完成）
    pub stage: CollectionStage,
    /// 已用时长（秒；处理中实时更新）
    pub elapsed_secs: u64,
    /// 本次是否“部分建筑未命名”（补名截止 / 上限 / 调用失败导致）
    pub naming_partial: bool,
    /// 总进度（0~100，占位实现：开始 0、完成 100）
    pub percent: u8,
    /// 已采集对象总数（完成文案的 {count} 实参）
    pub collected_total: usize,
    /// 六类别逐行进度
    pub categories: Vec<CategoryProgress>,
}

impl CollectionProgressView {
    /// 拉取中的初始视图（进度 0%，六类别全部待执行）
    pub fn fetching() -> Self {
        Self {
            title_key: text_keys::PROGRESS_TITLE,
            status_key: text_keys::PROGRESS_FETCHING,
            stage: CollectionStage::FetchingData,
            elapsed_secs: 0,
            naming_partial: false,
            percent: 0,
            collected_total: 0,
            categories: ALL_CATEGORIES
                .iter()
                .map(|&category| CategoryProgress {
                    category,
                    label_key: category_text_key(category),
                    collected: 0,
                    status: CollectionJobStatus::Pending,
                })
                .collect(),
        }
    }

    /// 处理中视图（阶段 + 已用时长由 A1 实时填充）
    pub fn fetching_at(stage: CollectionStage, elapsed_secs: u64) -> Self {
        Self {
            stage,
            elapsed_secs,
            ..Self::fetching()
        }
    }

    /// 采集完成后的视图（进度 100%，按报告填充各类别对象数）
    pub fn completed(report: &CollectionReport) -> Self {
        Self {
            title_key: text_keys::PROGRESS_TITLE,
            status_key: text_keys::PROGRESS_DONE,
            stage: CollectionStage::Finished,
            elapsed_secs: 0,
            naming_partial: report.naming_partial,
            percent: 100,
            collected_total: report.total,
            categories: ALL_CATEGORIES
                .iter()
                .map(|&category| CategoryProgress {
                    category,
                    label_key: category_text_key(category),
                    collected: report.category_counts.get(&category).copied().unwrap_or(0),
                    status: CollectionJobStatus::Completed,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refresh::RefreshDiff;
    use std::collections::BTreeMap;

    #[test]
    fn fetching_view_starts_at_zero_with_all_pending() {
        let view = CollectionProgressView::fetching();
        assert_eq!(view.percent, 0);
        assert_eq!(view.status_key, "collection.progress_fetching");
        assert_eq!(view.categories.len(), 6);
        assert!(view
            .categories
            .iter()
            .all(|c| c.status == CollectionJobStatus::Pending && c.collected == 0));
    }

    #[test]
    fn completed_view_reports_per_category_counts() {
        let mut category_counts = BTreeMap::new();
        category_counts.insert(CandidateCategory::Building, 3usize);
        category_counts.insert(CandidateCategory::Sports, 1usize);
        let report = CollectionReport {
            plan_id: "p1".to_owned(),
            source_tag: "gaode".to_owned(),
            total: 4,
            written: 4,
            category_counts,
            fallback_count: 0,
            diff: RefreshDiff::new(Vec::new()),
            naming_partial: false,
        };

        let view = CollectionProgressView::completed(&report);
        assert_eq!(view.percent, 100);
        assert_eq!(view.collected_total, 4);
        assert_eq!(view.status_key, "collection.progress_done");
        let building = view
            .categories
            .iter()
            .find(|c| c.category == CandidateCategory::Building)
            .unwrap();
        assert_eq!(building.collected, 3);
        assert_eq!(building.label_key, "collection.category_building");
        assert_eq!(building.status, CollectionJobStatus::Completed);
    }
}
