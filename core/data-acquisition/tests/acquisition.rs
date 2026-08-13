//! 集成测试：采集流水线全链路（窗口契约缝 3）。
//!
//! 覆盖工单三条测试要求：
//! 1. 给定一小块边界 → 采集对象数 ≥1 且进入 raw_observations 表；
//! 2. 第二次采集同一边界 → 差异展示（新增/更新/未变）；
//! 3. 负责人验收点：体育馆归体育、校门归其他。

use data_acquisition::{
    AcquisitionError, AcquisitionPipeline, BoundaryDisposition, CollectionProgressView, DataSource,
    DiffKind, EnrichedEntities, GaodeDataSource, OverpassDataSource, RawEntity, SourceGeometry,
};
use data_persistence::{Database, RawObservationsApi};
use shared_domain_types::{Boundary, CandidateCategory, PlanId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn run_deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

/// 一小块校园边界（GCJ-02 多边形）
fn small_boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
            [121.40, 31.21],
            [121.40, 31.20]
        ]]),
    }
}

/// 罐头桥接：固定回传一份高德地点搜索响应（离线，不打真实 API）
fn gaode_source_with(json: &'static str) -> GaodeDataSource {
    GaodeDataSource::new(Box::new(move |_| Ok(json.to_owned())))
}

const FIRST_SWEEP: &str = r#"{"status":"1","info":"OK","pois":[
    {"id":"B01","name":"第一教学楼","address":"校内","location":"121.401,31.201","typecode":"141201"},
    {"id":"B02","name":"体育馆","address":"校内","location":"121.402,31.202","typecode":"080300"},
    {"id":"B03","name":"南校门","address":"校内","location":"121.403,31.203","typecode":"991400"}
]}"#;

/// 第二次采集：B02 体育馆改了名（更新），B05 新泳池出现（新增），其余未变
const SECOND_SWEEP: &str = r#"{"status":"1","info":"OK","pois":[
    {"id":"B01","name":"第一教学楼","address":"校内","location":"121.401,31.201","typecode":"141201"},
    {"id":"B02","name":"综合体育馆","address":"校内","location":"121.402,31.202","typecode":"080300"},
    {"id":"B03","name":"南校门","address":"校内","location":"121.403,31.203","typecode":"991400"},
    {"id":"B05","name":"游泳馆","address":"校内","location":"121.405,31.205","typecode":"080500"}
]}"#;

#[test]
fn collect_small_boundary_lands_in_granary() {
    let pipeline = AcquisitionPipeline::new().expect("默认映射表可用");
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let source = gaode_source_with(FIRST_SWEEP);

    let report = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &source,
            run_deadline(),
        )
        .unwrap();

    // 采集回来的对象数 ≥1
    assert!(report.total >= 1, "一小块边界至少采回 1 个对象");
    assert_eq!(report.total, 3);
    assert_eq!(report.source_tag, "gaode");

    // 全部进入 raw_observations 表（数据粮仓）
    let observations = db.list_raw_observations(&plan_id.to_string()).unwrap();
    assert_eq!(observations.len(), 3, "对象逐条落库，禁止静默丢弃");
    assert!(observations.iter().all(|o| o.data_source_tag == "gaode"));

    // 负责人验收点：体育馆被标记为体育、校门被标记为其他
    let category_of = |entity_id: &str| {
        observations
            .iter()
            .find(|o| o.entity_id == entity_id)
            .map(|o| o.entity_type)
            .unwrap()
    };
    assert_eq!(category_of("B01"), CandidateCategory::Building);
    assert_eq!(category_of("B02"), CandidateCategory::Sports);
    assert_eq!(category_of("B03"), CandidateCategory::Other);

    // 首次采集全部为"新增"
    assert_eq!(report.diff.added_count(), 3);
    assert_eq!(report.diff.updated_count(), 0);
    assert_eq!(report.diff.unchanged_count(), 0);
}

#[test]
fn second_collect_reports_incremental_diff() {
    let pipeline = AcquisitionPipeline::new().unwrap();
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();

    pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(FIRST_SWEEP),
            run_deadline(),
        )
        .unwrap();
    let second = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(SECOND_SWEEP),
            run_deadline(),
        )
        .unwrap();

    // 增量刷新检测：新增 1（游泳馆）、更新 1（体育馆改名）、未变 2
    assert_eq!(second.diff.added_count(), 1);
    assert_eq!(second.diff.updated_count(), 1);
    assert_eq!(second.diff.unchanged_count(), 2);
    assert!(second.diff.has_changes());

    let kind_of = |entity_id: &str| {
        second
            .diff
            .entries()
            .iter()
            .find(|e| e.entity_id == entity_id)
            .map(|e| e.kind)
            .unwrap()
    };
    assert_eq!(kind_of("B05"), DiffKind::Added);
    assert_eq!(kind_of("B02"), DiffKind::Updated);
    assert_eq!(kind_of("B01"), DiffKind::Unchanged);
    assert_eq!(kind_of("B03"), DiffKind::Unchanged);

    // B2 只写有变化的行（未变的原样保留），粮仓总量只增不减
    assert_eq!(second.written, 2);
    let observations = db.list_raw_observations(&plan_id.to_string()).unwrap();
    assert_eq!(observations.len(), 4, "粮仓只增不减");
    let gym = observations.iter().find(|o| o.entity_id == "B02").unwrap();
    assert_eq!(gym.source_data["name"], "综合体育馆", "更新行内容已刷新");
    assert!(gym.created_at <= gym.updated_at, "created_at 保留原值");
}

#[test]
fn identical_recollect_is_all_unchanged() {
    let pipeline = AcquisitionPipeline::new().unwrap();
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();

    for _ in 0..2 {
        pipeline
            .collect(
                &mut db,
                &plan_id,
                &small_boundary(),
                &gaode_source_with(FIRST_SWEEP),
                run_deadline(),
            )
            .unwrap();
    }
    let third = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(FIRST_SWEEP),
            run_deadline(),
        )
        .unwrap();
    assert!(!third.diff.has_changes());
    assert_eq!(third.diff.unchanged_count(), 3);
    assert_eq!(third.written, 0, "digest 相同不产生写入");
}

#[test]
fn changing_plan_boundary_does_not_change_raw_osm_payload_or_digest() {
    #[derive(Clone)]
    struct OsmFixtureSource {
        entity: RawEntity,
    }

    impl DataSource for OsmFixtureSource {
        fn source_tag(&self) -> &str {
            "overpass-fixture"
        }

        fn fetch_raw_entities(
            &self,
            _boundary: &Boundary,
        ) -> data_acquisition::Result<Vec<RawEntity>> {
            Ok(vec![self.entity.clone()])
        }

        fn enrich(
            &self,
            mut entities: Vec<RawEntity>,
            _deadline: Instant,
        ) -> data_acquisition::Result<EnrichedEntities> {
            for entity in &mut entities {
                entity.name = "方案派生补名".to_owned();
            }
            Ok(EnrichedEntities {
                entities,
                partial: false,
                attempted: 1,
            })
        }
    }

    let source_payload = serde_json::json!({
        "type": "node",
        "id": 88001,
        "lon": 121.405,
        "lat": 31.205,
        "tags": {"natural": "tree"}
    });
    let source = OsmFixtureSource {
        entity: RawEntity::with_geometry(
            "node/88001",
            "OSM 原始名称",
            data_transformers::TagMap::from([("natural".to_owned(), "tree".to_owned())]),
            source_payload.clone(),
            Some(SourceGeometry::Point((121.405, 31.205))),
            "node",
        ),
    };
    let outside_boundary = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [121.50, 31.30],
            [121.51, 31.30],
            [121.51, 31.31],
            [121.50, 31.31],
            [121.50, 31.30]
        ]]),
    };
    let pipeline = AcquisitionPipeline::new().expect("pipeline");
    let mut db = Database::open_in_memory().expect("内存库");
    let plan_id = PlanId::generate();

    let inside = pipeline
        .acquire_batch(
            &mut db,
            &plan_id,
            &small_boundary(),
            &source,
            run_deadline(),
        )
        .expect("校内批次");
    pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &source,
            run_deadline(),
        )
        .expect("先持久化相同来源证据");
    let outside = pipeline
        .acquire_batch(
            &mut db,
            &plan_id,
            &outside_boundary,
            &source,
            run_deadline(),
        )
        .expect("校外批次");

    let inside_raw = &inside.raw_observations[0];
    let outside_raw = &outside.raw_observations[0];
    assert_eq!(inside_raw.source_data, outside_raw.source_data);
    assert_eq!(inside_raw.digest, outside_raw.digest);
    assert_eq!(inside_raw.source_data["payload"], source_payload);
    assert!(
        inside_raw.source_data.get("boundary_disposition").is_none(),
        "方案相关资格不得污染原始来源载荷"
    );
    assert_eq!(
        inside.candidate_drafts[0].boundary_disposition,
        BoundaryDisposition::Inside
    );
    assert_eq!(
        outside.candidate_drafts[0].boundary_disposition,
        BoundaryDisposition::Outside
    );
    assert_eq!(outside.diff.entries()[0].kind, DiffKind::Unchanged);
}

#[test]
fn progress_view_reflects_collection_report() {
    let pipeline = AcquisitionPipeline::new().unwrap();
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let report = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(FIRST_SWEEP),
            run_deadline(),
        )
        .unwrap();

    let view = CollectionProgressView::completed(&report);
    assert_eq!(view.percent, 100);
    assert_eq!(view.collected_total, 3);
    let sports = view
        .categories
        .iter()
        .find(|c| c.category == CandidateCategory::Sports)
        .unwrap();
    assert_eq!(sports.collected, 1, "进度条按类别报数");
}

/// 换源零改动：任何 DataSource 实现都能走同一条流水线（ADR-0013）
#[test]
fn pipeline_accepts_any_data_source() {
    struct OtherSource;
    impl DataSource for OtherSource {
        fn source_tag(&self) -> &str {
            "other-source"
        }
        fn fetch_raw_entities(
            &self,
            _boundary: &Boundary,
        ) -> data_acquisition::Result<Vec<data_acquisition::RawEntity>> {
            let mut tags = data_transformers::TagMap::new();
            tags.insert("natural".to_owned(), "water".to_owned());
            Ok(vec![data_acquisition::RawEntity::new(
                "W01",
                "镜湖",
                tags,
                serde_json::json!({"id": "W01"}),
            )])
        }
    }

    let pipeline = AcquisitionPipeline::new().unwrap();
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let report = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &OtherSource,
            run_deadline(),
        )
        .unwrap();
    assert_eq!(report.source_tag, "other-source");
    let observations = db
        .list_raw_observations_by_category(&plan_id.to_string(), CandidateCategory::Water)
        .unwrap();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].data_source_tag, "other-source");
}

fn putuo_boundary_gcj02() -> Boundary {
    let convert = |lon: f64, lat: f64| {
        let (lon, lat) = gaode_client::wgs84_to_gcj02(lon, lat);
        serde_json::json!([lon, lat])
    };
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            convert(121.3990, 31.2270),
            convert(121.4050, 31.2270),
            convert(121.4050, 31.2330),
            convert(121.3990, 31.2330),
            convert(121.3990, 31.2270)
        ]]),
    }
}

#[test]
fn putuo_fixture_is_filtered_after_wgs84_to_gcj02_with_conserved_boundary_counts() {
    let payload = include_str!("fixtures/putuo-boundary-eligibility.json").to_owned();
    let source = OverpassDataSource::new(Box::new(move |_| Ok(payload.clone())));
    let pipeline = AcquisitionPipeline::new().expect("默认映射表可用");
    let mut db = Database::open_in_memory().expect("内存库");
    let plan_id = PlanId::generate();

    let report = pipeline
        .collect(
            &mut db,
            &plan_id,
            &putuo_boundary_gcj02(),
            &source,
            run_deadline(),
        )
        .expect("边界外或坏 relation 不得拖垮整批");

    assert_eq!(report.total, 9, "来源数必须如实");
    assert_eq!(report.boundary_inside, 6);
    assert_eq!(report.boundary_crossing, 1);
    assert_eq!(report.boundary_outside, 1);
    assert_eq!(report.invalid_geometry, 1);
    assert_eq!(
        report.total,
        report.boundary_inside
            + report.boundary_crossing
            + report.boundary_outside
            + report.invalid_geometry,
        "来源数必须由边界内、相交、边界外、无效四类守恒"
    );
    assert_eq!(
        report.category_counts.values().sum::<usize>(),
        6,
        "六类覆盖统计只计算真实方案边界内对象"
    );
    assert_eq!(
        db.list_raw_observations(&plan_id.to_string())
            .expect("原始证据 API")
            .len(),
        9,
        "边界外和无法安全解析的 relation 仍保留来源证据"
    );
}

#[test]
fn malformed_overpass_coordinates_keep_source_evidence_and_count_as_invalid() {
    let payload = serde_json::json!({"elements": [
        {"type":"node","id":201,"tags":{"natural":"tree"},"lon":121.4},
        {"type":"way","id":202,"tags":{"highway":"service"},"geometry":[
            {"lon":121.4,"lat":31.228},{"lon":"bad","lat":31.229}
        ]}
    ]})
    .to_string();
    let source = OverpassDataSource::new(Box::new(move |_| Ok(payload.clone())));
    let pipeline = AcquisitionPipeline::new().expect("默认映射表可用");
    let mut db = Database::open_in_memory().expect("内存库");
    let plan_id = PlanId::generate();

    let report = pipeline
        .collect(
            &mut db,
            &plan_id,
            &putuo_boundary_gcj02(),
            &source,
            run_deadline(),
        )
        .expect("单个坏坐标不拖垮批次");

    assert_eq!(report.total, 2);
    assert_eq!(report.invalid_geometry, 2);
    assert_eq!(
        db.list_raw_observations(&plan_id.to_string())
            .expect("原始证据 API")
            .len(),
        2,
        "可识别来源 ID 的坏几何不得在解析阶段静默消失"
    );
}

#[test]
fn boundary_eligibility_is_decided_before_optional_naming() {
    struct NamingProbe {
        named_targets: AtomicUsize,
    }
    impl DataSource for NamingProbe {
        fn source_tag(&self) -> &str {
            "naming-probe"
        }

        fn fetch_raw_entities(
            &self,
            _boundary: &Boundary,
        ) -> data_acquisition::Result<Vec<data_acquisition::RawEntity>> {
            let tags =
                data_transformers::TagMap::from([("building".to_owned(), "school".to_owned())]);
            Ok(vec![
                data_acquisition::RawEntity::with_geometry(
                    "inside",
                    "inside",
                    tags.clone(),
                    serde_json::json!({"id":"inside"}),
                    Some(data_acquisition::SourceGeometry::Point((121.405, 31.230))),
                    "point",
                ),
                data_acquisition::RawEntity::with_geometry(
                    "outside",
                    "outside",
                    tags,
                    serde_json::json!({"id":"outside"}),
                    Some(data_acquisition::SourceGeometry::Point((121.500, 31.300))),
                    "point",
                ),
            ])
        }

        fn enrich(
            &self,
            entities: Vec<data_acquisition::RawEntity>,
            _deadline: Instant,
        ) -> data_acquisition::Result<data_acquisition::EnrichedEntities> {
            self.named_targets.store(entities.len(), Ordering::SeqCst);
            assert_eq!(entities[0].entity_id, "inside");
            Ok(data_acquisition::EnrichedEntities {
                entities,
                partial: false,
                attempted: 1,
            })
        }
    }

    let source = NamingProbe {
        named_targets: AtomicUsize::new(0),
    };
    let mut db = Database::open_in_memory().expect("内存库");
    AcquisitionPipeline::new()
        .expect("pipeline")
        .collect(
            &mut db,
            &PlanId::generate(),
            &putuo_boundary_gcj02(),
            &source,
            run_deadline(),
        )
        .expect("采集");
    assert_eq!(source.named_targets.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_confirmed_boundary_stops_before_fetching_source_data() {
    struct FetchProbe {
        fetches: AtomicUsize,
    }

    impl DataSource for FetchProbe {
        fn source_tag(&self) -> &str {
            "fetch-probe"
        }

        fn fetch_raw_entities(
            &self,
            _boundary: &Boundary,
        ) -> data_acquisition::Result<Vec<data_acquisition::RawEntity>> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    let source = FetchProbe {
        fetches: AtomicUsize::new(0),
    };
    let invalid_boundary = Boundary {
        r#type: "LineString".to_owned(),
        coordinates: serde_json::json!([[121.40, 31.20], [121.41, 31.21]]),
    };
    let mut db = Database::open_in_memory().expect("内存库");

    let error = AcquisitionPipeline::new()
        .expect("pipeline")
        .collect(
            &mut db,
            &PlanId::generate(),
            &invalid_boundary,
            &source,
            run_deadline(),
        )
        .expect_err("非方案面边界必须失败关闭");

    assert!(matches!(error, AcquisitionError::InvalidBoundary));
    assert_eq!(source.fetches.load(Ordering::SeqCst), 0);
}

#[test]
fn confirmed_polygon_without_repeated_first_point_is_safely_closed() {
    let open_ring_boundary = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
            [121.40, 31.21]
        ]]),
    };
    let source = OverpassDataSource::new(Box::new(|_| {
        Ok(serde_json::json!({"elements": []}).to_string())
    }));
    let mut db = Database::open_in_memory().expect("内存库");

    let report = AcquisitionPipeline::new()
        .expect("pipeline")
        .collect(
            &mut db,
            &PlanId::generate(),
            &open_ring_boundary,
            &source,
            run_deadline(),
        )
        .expect("方案画布的未重复首点 Polygon 应按闭环解释");

    assert_eq!(report.total, 0);
}
