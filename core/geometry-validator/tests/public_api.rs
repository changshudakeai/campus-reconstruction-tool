use geometry_validator::{
    CandidateGeometry, GeometryShape, GeometryValidator, ValidationDisposition,
};

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

#[test]
fn retains_a_valid_point_without_coercing_it_to_a_polygon() {
    let outcome = GeometryValidator::new().validate(CandidateGeometry::with_shape(
        "poi",
        GeometryShape::Point((121.401, 31.201)),
    ));

    assert!(matches!(
        outcome.disposition,
        ValidationDisposition::Retained
    ));
    assert!(matches!(
        outcome.geometry.unwrap().shape,
        GeometryShape::Point(_)
    ));
}

#[test]
fn isolates_a_line_with_a_zero_length_segment() {
    let outcome = GeometryValidator::new().validate(CandidateGeometry::with_shape(
        "road",
        GeometryShape::LineString(vec![(121.4, 31.2), (121.4, 31.2)]),
    ));

    assert!(matches!(
        outcome.disposition,
        ValidationDisposition::Rejected(_)
    ));
    assert!(outcome.geometry.is_none());
}
