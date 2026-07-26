//! 集成测试：采集流水线全链路（窗口契约缝 3）。
//!
//! 覆盖工单三条测试要求：
//! 1. 给定一小块边界 → 采集对象数 ≥1 且进入 raw_observations 表；
//! 2. 第二次采集同一边界 → 差异展示（新增/更新/未变）；
//! 3. 负责人验收点：体育馆归体育、校门归其他。

use data_acquisition::{
    AcquisitionPipeline, CollectionProgressView, DataSource, DiffKind, GaodeDataSource,
};
use data_persistence::{Database, RawObservationsApi};
use shared_domain_types::{Boundary, CandidateCategory, PlanId};

/// 一小块校园边界（GCJ-02 多边形）
fn small_boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
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
        .collect(&mut db, &plan_id, &small_boundary(), &source)
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
        )
        .unwrap();
    let second = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(SECOND_SWEEP),
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
            )
            .unwrap();
    }
    let third = pipeline
        .collect(
            &mut db,
            &plan_id,
            &small_boundary(),
            &gaode_source_with(FIRST_SWEEP),
        )
        .unwrap();
    assert!(!third.diff.has_changes());
    assert_eq!(third.diff.unchanged_count(), 3);
    assert_eq!(third.written, 0, "digest 相同不产生写入");
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
        .collect(&mut db, &plan_id, &small_boundary(), &OtherSource)
        .unwrap();
    assert_eq!(report.source_tag, "other-source");
    let observations = db
        .list_raw_observations_by_category(&plan_id.to_string(), CandidateCategory::Water)
        .unwrap();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].data_source_tag, "other-source");
}
