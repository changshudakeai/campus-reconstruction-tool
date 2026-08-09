//! 候选对象内存模型
//!
//! 缝 4 契约：候选集在进台时从 B2 已发布的可评审投影一次性读入本模型，
//! 评审期间所有状态都在这里改，不碰数据库。
//! 类别与三态复用 B1 共享领域类型（不重新定义）。

use data_persistence::{CandidateProjection, CandidateShape};
use shared_domain_types::{CandidateCategory, ReviewState};

/// 候选对象的稳定标识。
///
/// 身份只由 `candidate_id` 决定；category 是可随新投影变化的当前属性。
/// `candidate_id` 区分数据源与几何分片，不等同于 `source_entity_id`（ADR-0040）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateKey {
    /// B2 候选投影的稳定 ID。
    pub candidate_id: String,
}

impl CandidateKey {
    /// 构建候选标识
    pub fn new(candidate_id: impl Into<String>) -> Self {
        Self {
            candidate_id: candidate_id.into(),
        }
    }
}

impl std::fmt::Display for CandidateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.candidate_id)
    }
}

/// 评审台上的一个候选（纯内存状态）
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// 稳定候选标识。
    pub key: CandidateKey,
    /// 候选投影当前的六类别属性。
    pub category: CandidateCategory,
    /// 卡片标题：原始标签里的 `name`，没有则回落到实体 ID
    pub title: String,
    /// 标题是否来自真实名称（OSM `name` 或 regeo 补名）；false = 回退标识，
    /// UI 显示"未命名建筑 #id"（T38 抽屉详情）。
    pub named: bool,
    /// 来源标签（`data_source_tag`，如 overpass；详情面板"来源"行）。
    pub source: String,
    /// 标签与属性（key=value 对，信息面板展示用）
    pub tags: Vec<(String, String)>,
    /// GCJ-02 几何（点/线/面；评审地图标注与"定位到地图"用）。
    pub shape: CandidateShape,
    /// 评审三态（初始一律"待定"，ADR-0022）
    pub state: ReviewState,
    /// 卡片复选框勾选状态（批量操作的输入）
    pub selected: bool,
}

impl Candidate {
    /// 从 B2 已发布的可评审投影构建候选（初始态"待定"、未勾选）。
    pub fn from_projection(projection: &CandidateProjection) -> Self {
        Self {
            key: CandidateKey::new(projection.candidate_id.clone()),
            category: projection.category,
            title: projection.display.title.clone(),
            named: projection.display.title != projection.source_entity_id,
            source: projection.data_source_tag.clone(),
            tags: projection.display.tags.clone(),
            shape: projection.shape.clone(),
            state: ReviewState::Pending,
            selected: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_persistence::{
        CandidateDisplay, CandidateEligibility, CandidateShape, CandidateValidation,
    };

    fn projection() -> CandidateProjection {
        CandidateProjection::new(
            "overpass:way/100:outer",
            "plan-1",
            "raw-1",
            "overpass",
            "way/100",
            "outer",
            CandidateCategory::Building,
            CandidateDisplay::new(
                "体育馆",
                vec![("building".to_owned(), "gymnasium".to_owned())],
            ),
            CandidateShape::polygon(serde_json::json!([
                [121.4, 31.2],
                [121.5, 31.2],
                [121.4, 31.3],
                [121.4, 31.2]
            ])),
            CandidateValidation::Retained,
            CandidateEligibility::Reviewable,
        )
    }

    #[test]
    fn projection_display_and_stable_candidate_id_are_preserved() {
        let candidate = Candidate::from_projection(&projection());
        assert_eq!(candidate.key.candidate_id, "overpass:way/100:outer");
        assert_eq!(candidate.category, CandidateCategory::Building);
        assert_eq!(candidate.title, "体育馆");
        assert!(candidate.named, "有真实名称的候选必须标记为 named");
        assert_eq!(candidate.source, "overpass");
        assert_eq!(
            candidate.tags,
            vec![("building".to_owned(), "gymnasium".to_owned())]
        );
        assert_eq!(candidate.shape.kind, "polygon");
        assert_eq!(candidate.state, ReviewState::Pending);
        assert!(!candidate.selected);
    }

    #[test]
    fn projection_without_name_is_marked_unnamed_and_keeps_fallback_identifier() {
        // T38：无名称候选的标题回退为实体 ID；named=false 供 UI 显示
        // "未命名建筑 #id"（id 即回退标识）。
        let unnamed = CandidateProjection::new(
            "overpass:way/101:outer",
            "plan-1",
            "raw-2",
            "overpass",
            "way/101",
            "outer",
            CandidateCategory::Building,
            CandidateDisplay::new("way/101", Vec::new()),
            CandidateShape::point(serde_json::json!([121.4, 31.2])),
            CandidateValidation::Retained,
            CandidateEligibility::Reviewable,
        );
        let candidate = Candidate::from_projection(&unnamed);
        assert_eq!(candidate.title, "way/101");
        assert!(!candidate.named, "无名称候选必须标记为未命名");
        assert_eq!(candidate.source, "overpass");
        assert_eq!(candidate.shape.kind, "point");
    }

    #[test]
    fn candidate_key_carries_only_the_stable_candidate_id() {
        let key = CandidateKey::new("overpass:way/100:outer");
        assert_eq!(key.candidate_id, "overpass:way/100:outer");
    }
}
