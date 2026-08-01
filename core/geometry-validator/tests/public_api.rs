use geometry_validator::{CandidateGeometry, GeometryValidator, ValidationDisposition};

#[test]
fn validates_a_batch_without_losing_valid_candidates() {
    let validator = GeometryValidator::new();
    let result = validator.validate_batch(vec![
        CandidateGeometry::new("safe", vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        CandidateGeometry::new("bad", vec![(0.0, 0.0), (1.0, 1.0)]),
    ]);

    assert_eq!(result.rejected_count, 1);
    assert_eq!(result.retained().len(), 1);
    assert!(matches!(
        result.outcomes[0].disposition,
        ValidationDisposition::Repaired
    ));
    assert!(matches!(
        result.outcomes[1].disposition,
        ValidationDisposition::Rejected(_)
    ));
}
