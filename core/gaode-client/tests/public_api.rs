//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use gaode_client::{
    build_map_page_html, parse_place_search_response, CampusPoiRecord, CampusSearchFlow, Error,
    MapPageConfig, SchoolPoi, SearchFlowState, GAODE_CDN_URL_TEMPLATE, MAP_MIN_HEIGHT_PX,
    SCHOOL_TYPECODE_PREFIX,
};

#[test]
fn public_api_types_exist() {
    // 常量：官方 CDN v2.0 + 最小高度 300px + 教育类目前缀
    assert!(GAODE_CDN_URL_TEMPLATE.starts_with("https://webapi.amap.com/maps?v=2.0"));
    assert_eq!(MAP_MIN_HEIGHT_PX, 300);
    assert_eq!(SCHOOL_TYPECODE_PREFIX, "1412");

    // POI 解析：筛选学校类目 + 去重
    let json = r#"{"status":"1","info":"OK","pois":[
        {"id":"B01","name":"测试大学(东校区)","address":"学院路1号","location":"121.4,31.2","typecode":"141201"},
        {"id":"B02","name":"测试大学站(公交站)","address":"学院路","location":"121.41,31.21","typecode":"150700"}
    ]}"#;
    let pois: Vec<SchoolPoi> = parse_place_search_response(json).unwrap();
    assert_eq!(pois.len(), 1);
    assert!(pois[0].is_school());

    // 确认流程状态机：搜索 → 候选 → 详情 → 显式确认（唯一出口）
    let mut flow = CampusSearchFlow::new();
    assert_eq!(*flow.state(), SearchFlowState::Idle);
    flow.start_search("测试大学").unwrap();
    flow.receive_results(pois).unwrap();
    assert_eq!(flow.candidates().len(), 1);

    // 未看详情直接确认 → 拒绝（不自动进入校区）
    let err = flow.confirm().unwrap_err();
    assert!(matches!(err, Error::InvalidFlowStep(_)));
    assert!(!err.to_string().is_empty());

    flow.view_detail(0).unwrap();
    flow.back_to_candidates().unwrap();
    flow.view_detail(0).unwrap();
    let confirmed = flow.confirm().unwrap();
    assert_eq!(confirmed.record.name, "测试大学(东校区)");
    assert_eq!(*flow.state(), SearchFlowState::Confirmed);

    // CampusPoiRecord：POI identity + coordinate lineage（serde 往返）
    let record: CampusPoiRecord = confirmed.record;
    assert_eq!(record.gaode_poi_id, "B01");
    assert_eq!(
        record.coordinate_system,
        CampusPoiRecord::COORDINATE_SYSTEM_GCJ02
    );
    assert_eq!(record.data_source, CampusPoiRecord::DATA_SOURCE_GAODE);
    let round_trip: CampusPoiRecord =
        serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
    assert_eq!(round_trip, record);

    // 地图页：官方 CDN v2.0 + 最小高度 300px
    let html =
        build_map_page_html(&MapPageConfig::new("0123456789abcdef", "fedcba9876543210")).unwrap();
    assert!(html.contains("webapi.amap.com/maps?v=2.0"));
    assert!(html.contains("min-height: 300px"));
}
