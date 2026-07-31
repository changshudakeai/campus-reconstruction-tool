//! B1 共享领域类型的行为断言。
//!
//! 公开 API 的结构面（类型、方法、derive 清单）由 `public_api.rs` 的快照
//! 机器比对；本文件只保留类型行为语义（Display、优先级铁律、边界/朝向
//! 约束），不复制快照的枚举清单，避免两处维护。

#[test]
fn domain_types_behave_as_contract() {
    // CampusId & PlanId
    let campus_id = shared_domain_types::CampusId::generate();
    let plan_id = shared_domain_types::PlanId::generate();

    assert_eq!(format!("{}", campus_id), campus_id.to_string());
    assert_eq!(format!("{}", plan_id), plan_id.to_string());

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

    // Boundary 与 Orientation 的公开行为
    let boundary = shared_domain_types::Boundary::empty();
    let orientation = shared_domain_types::Orientation::new(90.0).unwrap();
    assert!(boundary.is_empty());
    assert_eq!(orientation.degree(), 90.0);
}
