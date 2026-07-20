use campus_tool_protocol::{
    MapAreaGeometry, MapCoarseRasterDecision, MapCoarseRasterEvidence, MapCoordinate,
    MapEvidenceAssessment, ToolEvent,
};

#[test]
fn coarse_raster_comparison_and_three_review_actions_round_trip() {
    let evidence = MapCoarseRasterEvidence {
        id: "raster-water-east-v1".into(),
        linked_gap_id: "gap:water:osm:31-121-1:0".into(),
        label: "Coarse water evidence".into(),
        decision: "unresolved".into(),
        dataset_summary: "Sentinel-2 L2A · 2026-07-01".into(),
        resolution_class_summary: "10 m · surface-water".into(),
        lineage_summary: "coarse-gap-water-v1.0.0 · gdal-polygonize-3.11+simplify-v1".into(),
        exclusion_summary: "8 cells excluded because structured geometry retains priority".into(),
        assessment: MapEvidenceAssessment {
            geometry: "approximate · connected pixels".into(),
            semantics: "supported · bundle-pinned threshold".into(),
            entity_match: "not_applicable · no precise entity claim".into(),
            name_match: "not_applicable · no name claim".into(),
        },
        approximate_geometry: MapAreaGeometry::Polygon {
            rings: vec![
                vec![
                    MapCoordinate {
                        lng: 121.4,
                        lat: 31.21,
                    },
                    MapCoordinate {
                        lng: 121.404,
                        lat: 31.21,
                    },
                    MapCoordinate {
                        lng: 121.404,
                        lat: 31.214,
                    },
                ],
                vec![
                    MapCoordinate {
                        lng: 121.401,
                        lat: 31.211,
                    },
                    MapCoordinate {
                        lng: 121.402,
                        lat: 31.211,
                    },
                    MapCoordinate {
                        lng: 121.401,
                        lat: 31.212,
                    },
                ],
            ],
        },
        warnings: vec![
            "Approximate coverage only; this is not a precise feature boundary.".into(),
            "The displayed edge may differ by at least one source pixel.".into(),
            "Minecraft/Axiom finishing is still expected.".into(),
        ],
    };
    let json = serde_json::to_value(&evidence).unwrap();
    assert_eq!(json["linkedGapId"], "gap:water:osm:31-121-1:0");
    assert_eq!(json["warnings"].as_array().unwrap().len(), 3);
    assert_eq!(
        json["approximateGeometry"]["rings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let request = ToolEvent::MapCoarseRasterSupplementRequested {
        category: "water".into(),
        gap_id: "gap:water:osm:31-121-1:0".into(),
    };
    assert_eq!(
        serde_json::from_slice::<ToolEvent>(&serde_json::to_vec(&request).unwrap()).unwrap(),
        request
    );
    for decision in [
        MapCoarseRasterDecision::Accept,
        MapCoarseRasterDecision::Reject,
        MapCoarseRasterDecision::LeaveUnresolved,
    ] {
        let event = ToolEvent::MapCoarseRasterDecisionRequested {
            category: "water".into(),
            observation_id: "raster-water-east-v1".into(),
            decision,
        };
        let encoded = serde_json::to_vec(&event).unwrap();
        let restored: ToolEvent = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored, event);
    }
}
