//! 候选对象内存模型
//!
//! 缝 4 契约：候选集在进台时从 B2 已发布的可评审投影一次性读入本模型，
//! 评审期间所有状态都在这里改，不碰数据库。
//! 类别与三态复用 B1 共享领域类型（不重新定义）。

use data_persistence::CandidateProjection;
use shared_domain_types::{CandidateCategory, ReviewState};

/// 候选对象的稳定标识：类别 + 候选投影 ID。
///
/// `candidate_id` 区分数据源与几何分片，不等同于来源对象的
/// `source_entity_id`（ADR-0040）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateKey {
    /// 实体类别（六类别之一）
    pub category: CandidateCategory,
    /// B2 候选投影的稳定 ID。
    pub candidate_id: String,
}

impl CandidateKey {
    /// 构建候选标识
    pub fn new(category: CandidateCategory, candidate_id: impl Into<String>) -> Self {
        Self {
            category,
            candidate_id: candidate_id.into(),
        }
    }
}

impl std::fmt::Display for CandidateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}/{}", self.category, self.candidate_id)
    }
}

/// 评审台上的一个候选（纯内存状态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 稳定标识（类别 + 实体 ID）
    pub key: CandidateKey,
    /// 卡片标题：原始标签里的 `name`，没有则回落到实体 ID
    pub title: String,
    /// 标签与属性（key=value 对，信息面板展示用）
    pub tags: Vec<(String, String)>,
    /// 评审三态（初始一律"待定"，ADR-0022）
    pub state: ReviewState,
    /// 卡片复选框勾选状态（批量操作的输入）
    pub selected: bool,
}

impl Candidate {
    /// 从 B2 已发布的可评审投影构建候选（初始态"待定"、未勾选）。
    pub fn from_projection(projection: &CandidateProjection) -> Self {
        Self {
            key: CandidateKey::new(projection.category, projection.candidate_id.clone()),
            title: projection.display.title.clone(),
            tags: projection.display.tags.clone(),
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
        assert_eq!(candidate.title, "体育馆");
        assert_eq!(
            candidate.tags,
            vec![("building".to_owned(), "gymnasium".to_owned())]
        );
        assert_eq!(candidate.state, ReviewState::Pending);
        assert!(!candidate.selected);
    }
}
