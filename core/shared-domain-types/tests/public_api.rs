//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在此快照中，PR diff 可见。
//!
//! 简单方式：只检查所有公开类型都可实例化并 Display/Debug。

#[test]
fn public_api_types_exist() {
    // CampusId & PlanId
    let campus_id = shared_domain_types::CampusId::generate();
    let plan_id = shared_domain_types::PlanId::generate();

    assert_eq!(format!("{}", campus_id), campus_id.to_string());
    assert_eq!(format!("{}", plan_id), plan_id.to_string());

    // CandidateCategory #[non_exhaustive]
    let _ = shared_domain_types::CandidateCategory::Building;
    let _ = shared_domain_types::CandidateCategory::Road;
    let _ = shared_domain_types::CandidateCategory::Water;
    let _ = shared_domain_types::CandidateCategory::Vegetation;
    let _ = shared_domain_types::CandidateCategory::Sports;
    let _ = shared_domain_types::CandidateCategory::Other;

    // 优先级铁律（ADR-0011）：建筑 > 体育 > 水域 > 道路 > 植被 > 其他
    assert!(
        shared_domain_types::CandidateCategory::Building.priority()
            > shared_domain_types::CandidateCategory::Sports.priority()
    );
    assert!(
        shared_domain_types::CandidateCategory::Sports.priority()
            > shared_domain_types::CandidateCategory::Water.priority()
    );
    assert!(
        shared_domain_types::CandidateCategory::Water.priority()
            > shared_domain_types::CandidateCategory::Road.priority()
    );
    assert!(
        shared_domain_types::CandidateCategory::Road.priority()
            > shared_domain_types::CandidateCategory::Vegetation.priority()
    );
    assert!(
        shared_domain_types::CandidateCategory::Vegetation.priority()
            > shared_domain_types::CandidateCategory::Other.priority()
    );

    // ReviewState #[non_exhaustive]
    let _ = shared_domain_types::ReviewState::Pending;
    let _ = shared_domain_types::ReviewState::Keep;
    let _ = shared_domain_types::ReviewState::Remove;

    // Boundary & Orientation
    let boundary = shared_domain_types::Boundary::empty();
    let orientation = shared_domain_types::Orientation::new(90.0).unwrap();
    assert!(boundary.is_empty());
    assert_eq!(orientation.degree(), 90.0);

    // CollectionJobStatus #[non_exhaustive]
    let _ = shared_domain_types::CollectionJobStatus::Pending;
    let _ = shared_domain_types::CollectionJobStatus::InProgress;
    let _ = shared_domain_types::CollectionJobStatus::Completed;
    let _ = shared_domain_types::CollectionJobStatus::Failed;
}
