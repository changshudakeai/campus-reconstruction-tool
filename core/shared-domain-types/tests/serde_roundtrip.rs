//! 手写 serde 实现的往返正确性测试
//!
//! CampusId/PlanId/ReviewState 的 Serialize/Deserialize 为手写实现，
//! 本文件断言"序列化 → 反序列化"得到原值，防止两侧实现漂移。

use shared_domain_types::{
    Boundary, CampusId, CandidateCategory, Orientation, PlanId, ReviewState,
};

#[test]
fn campus_id_roundtrip() {
    let id = CampusId::generate();
    let json = serde_json::to_string(&id).unwrap();
    let back: CampusId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn plan_id_roundtrip() {
    let id = PlanId::generate();
    let json = serde_json::to_string(&id).unwrap();
    let back: PlanId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn campus_id_parse_rejects_garbage() {
    assert!(CampusId::parse("not-a-uuid").is_err());
}

#[test]
fn review_state_roundtrip() {
    for state in [ReviewState::Pending, ReviewState::Keep, ReviewState::Remove] {
        let json = serde_json::to_string(&state).unwrap();
        let back: ReviewState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }
}

#[test]
fn review_state_parse_accepts_chinese_names() {
    assert_eq!(ReviewState::parse("待定"), Some(ReviewState::Pending));
    assert_eq!(ReviewState::parse("保留"), Some(ReviewState::Keep));
    assert_eq!(ReviewState::parse("剔除"), Some(ReviewState::Remove));
    assert_eq!(ReviewState::parse("unknown"), None);
}

#[test]
fn candidate_category_roundtrip() {
    let json = serde_json::to_string(&CandidateCategory::Sports).unwrap();
    let back: CandidateCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(back, CandidateCategory::Sports);
}

#[test]
fn boundary_roundtrip() {
    let boundary = Boundary::empty();
    let json = serde_json::to_string(&boundary).unwrap();
    let back: Boundary = serde_json::from_str(&json).unwrap();
    assert_eq!(boundary, back);
}

#[test]
fn orientation_rejects_out_of_range() {
    assert!(Orientation::new(-1.0).is_none());
    assert!(Orientation::new(360.1).is_none());
    assert!(Orientation::new(0.0).is_some());
    assert!(Orientation::new(360.0).is_some());
}
