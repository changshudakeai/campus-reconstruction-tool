//! T35 正式回归（b）：真实 OSM 环确认成功（罐头 Overpass/Nominatim 响应，
//! 去掉网络依赖）。
//!
//! 夹具来自真实抓取（2026-08-09）：
//! - 华东师大普陀（relation 6179557）：90 个 WGS-84 点，闭合节点**不在末尾**
//!   （第 88 点即首点，其后仍带 2 个尾点）——正是 T33
//!   `normalize_closed_ring` + 共享端点跳过的修复场景；
//! - 上海交通大学闵行校区（way 288249651）：39 个 WGS-84 点，末尾闭合。
//!
//! 断言链路：罐头 Nominatim → 罐头 Overpass by-id → WGS→GCJ → ADR-0029 排序
//! → `CampusBoundaryFetcher` 自动选中 → 经工作区确认 seam
//! （坐标转换 + B5 校验 + F9/A1 confirm_boundary）→ 边界确定、五步解锁。

use std::time::Duration;

use data_acquisition::overpass::{
    CampusBoundaryFetcher, CampusBoundaryResult, NominatimClient, OverpassClient,
};
use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;
use std::sync::Arc;

/// 华东师大普陀（relation 6179557）真实 WGS-84 外环（90 点，中途闭合 + 2 尾点）。
fn ecnu_putuo_ring_wgs84() -> Vec<[f64; 2]> {
    vec![
        [121.4000094, 31.233672],
        [121.4013932, 31.2334876],
        [121.402358, 31.2333508],
        [121.4023449, 31.2332638],
        [121.4027783, 31.2324008],
        [121.4029882, 31.2320416],
        [121.4031105, 31.2319391],
        [121.403352, 31.2315757],
        [121.4034108, 31.2314461],
        [121.4037537, 31.2315596],
        [121.4038474, 31.2313567],
        [121.4041307, 31.2314446],
        [121.4044873, 31.2306258],
        [121.4049268, 31.2307498],
        [121.4051607, 31.230142],
        [121.4052738, 31.230173],
        [121.4053363, 31.2300081],
        [121.405266, 31.2299882],
        [121.4054329, 31.2294966],
        [121.4055564, 31.229137],
        [121.4056186, 31.2291523],
        [121.4058215, 31.2285768],
        [121.4061182, 31.2276661],
        [121.4061806, 31.227369],
        [121.4061896, 31.2270571],
        [121.4061752, 31.226876],
        [121.4061201, 31.2268669],
        [121.4059384, 31.2268438],
        [121.4059561, 31.2267738],
        [121.4057927, 31.2267496],
        [121.4058727, 31.2263343],
        [121.4055609, 31.2262696],
        [121.4053658, 31.2269024],
        [121.4050285, 31.2268412],
        [121.4051381, 31.2264557],
        [121.4046839, 31.2263284],
        [121.40475, 31.2261039],
        [121.4044378, 31.2260159],
        [121.4042746, 31.2259699],
        [121.4041493, 31.2259109],
        [121.4028132, 31.2255219],
        [121.4028247, 31.2254837],
        [121.4019589, 31.2252694],
        [121.4015591, 31.2251705],
        [121.4014839, 31.2250969],
        [121.4014293, 31.2250497],
        [121.3998703, 31.2250533],
        [121.3998515, 31.2245124],
        [121.3999581, 31.2245094],
        [121.400066, 31.2239386],
        [121.3994113, 31.2238738],
        [121.3994545, 31.2236063],
        [121.3988212, 31.2235236],
        [121.398147, 31.2234204],
        [121.3981103, 31.2234598],
        [121.3981334, 31.2252089],
        [121.3980671, 31.2259014],
        [121.3980935, 31.2259069],
        [121.3992094, 31.2261665],
        [121.3994716, 31.2262722],
        [121.3992317, 31.2267031],
        [121.3989925, 31.2269524],
        [121.3989681, 31.2270617],
        [121.3987325, 31.2276803],
        [121.3985985, 31.227643],
        [121.3985556, 31.2277877],
        [121.3985383, 31.2278471],
        [121.3983702, 31.2284246],
        [121.3983873, 31.22856],
        [121.3984083, 31.2286807],
        [121.3984166, 31.228957],
        [121.3983594, 31.2291685],
        [121.3983226, 31.2293048],
        [121.3982176, 31.2296932],
        [121.3982064, 31.2297145],
        [121.3979695, 31.2301666],
        [121.3978041, 31.2303312],
        [121.3976544, 31.2315401],
        [121.3975887, 31.23193],
        [121.3973669, 31.2326028],
        [121.3975326, 31.2326781],
        [121.3975681, 31.2326869],
        [121.3976489, 31.2327042],
        [121.3986296, 31.2329504],
        [121.3987996, 31.232608],
        [121.3990608, 31.2326133],
        [121.4000966, 31.2328894],
        [121.4000094, 31.233672],
        [121.3975681, 31.2326869],
        [121.3976489, 31.2327042],
    ]
}

/// 上海交通大学闵行校区（way 288249651）真实 WGS-84 外环（39 点，末尾闭合）。
fn sjtu_minhang_ring_wgs84() -> Vec<[f64; 2]> {
    vec![
        [121.4184319, 31.029509],
        [121.4190287, 31.0281309],
        [121.4190911, 31.0279867],
        [121.4204292, 31.0248775],
        [121.421485, 31.0224584],
        [121.4230024, 31.0189542],
        [121.4258833, 31.0198888],
        [121.4264014, 31.0200589],
        [121.4271203, 31.0202956],
        [121.4272939, 31.019984],
        [121.427723, 31.0198737],
        [121.4303838, 31.0207012],
        [121.4302625, 31.0210198],
        [121.4308969, 31.0211946],
        [121.4310061, 31.0209035],
        [121.4321433, 31.0212529],
        [121.4320575, 31.0214367],
        [121.4402758, 31.0240663],
        [121.440426, 31.0244708],
        [121.440846, 31.0245908],
        [121.4411985, 31.0246915],
        [121.4416276, 31.0245444],
        [121.4446746, 31.0254454],
        [121.4458912, 31.0257805],
        [121.4459784, 31.0259286],
        [121.4459319, 31.026199],
        [121.4455325, 31.0271228],
        [121.444965, 31.0283949],
        [121.4440577, 31.0304284],
        [121.4413219, 31.0368176],
        [121.4362809, 31.0350989],
        [121.4362024, 31.0350738],
        [121.432237, 31.0338054],
        [121.4321987, 31.0337932],
        [121.4286084, 31.0326099],
        [121.4268438, 31.0320683],
        [121.4210741, 31.030322],
        [121.420967, 31.0302911],
        [121.4184319, 31.029509],
    ]
}

/// 罐头 Overpass 响应：relation by-id（华东师大普陀）。
fn canned_overpass_relation(ring: &[[f64; 2]]) -> String {
    let geometry: Vec<serde_json::Value> = ring
        .iter()
        .map(|[lon, lat]| serde_json::json!({"lon": lon, "lat": lat}))
        .collect();
    serde_json::json!({
        "elements": [{
            "type": "relation",
            "id": 6179557,
            "tags": {"name": "华东师范大学"},
            "members": [{"type": "way", "ref": 1, "role": "outer", "geometry": geometry}]
        }]
    })
    .to_string()
}

/// 罐头 Overpass 响应：way by-id（上交闵行）。
fn canned_overpass_way(ring: &[[f64; 2]]) -> String {
    let geometry: Vec<serde_json::Value> = ring
        .iter()
        .map(|[lon, lat]| serde_json::json!({"lon": lon, "lat": lat}))
        .collect();
    serde_json::json!({
        "elements": [{
            "type": "way",
            "id": 288249651,
            "tags": {"name": "上海交通大学（闵行校区）"},
            "geometry": geometry
        }]
    })
    .to_string()
}

/// 罐头 Nominatim 命中（university 面元素）。
fn canned_nominatim(osm_type: &str, osm_id: i64, display_name: &str) -> String {
    format!(
        r#"[{{"osm_type":"{osm_type}","osm_id":{osm_id},"class":"amenity","type":"university","display_name":"{display_name}"}}]"#
    )
}

fn canned_fetcher(overpass_body: String, nominatim_body: String) -> CampusBoundaryFetcher {
    let overpass = OverpassClient::with_transport(Box::new(move |_: &str, _: Duration| {
        Ok(overpass_body.clone())
    }));
    let nominatim = NominatimClient::with_transport(Box::new(move |_: &str, _: Duration| {
        Ok(nominatim_body.clone())
    }));
    CampusBoundaryFetcher::with_clients(overpass, nominatim)
}

/// 抓取真实校区环（罐头传输）并断言自动选中事实。
fn fetch_canned_campus(
    fetcher: CampusBoundaryFetcher,
    campus_name: &str,
    anchor_lon: f64,
    anchor_lat: f64,
    expect_source: data_acquisition::overpass::BoundarySourceKind,
) -> Vec<[f64; 2]> {
    match fetcher.fetch_campus(campus_name, anchor_lon, anchor_lat) {
        CampusBoundaryResult::AutoSelected {
            name,
            gcj02,
            source,
            candidate_count,
        } => {
            assert_eq!(source, expect_source, "必须走 by-id 主路径");
            assert_eq!(candidate_count, 1, "罐头响应只含一个候选");
            assert!(!name.is_empty(), "必须保留 OSM name");
            assert!(gcj02.len() >= 3, "GCJ-02 环点过少：{gcj02:?}");
            gcj02
        }
        other => panic!("罐头抓取必须自动选中 {campus_name}：{other:?}"),
    }
}

#[test]
fn s1_25_real_osm_rings_confirm_with_canned_responses() {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-25.db");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接库"))
            .expect("正式注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("验收校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    assert_eq!(window.get_active_screen(), 4);
    assert_eq!(window.get_workspace_active_step(), 0);

    // ── 1. 华东师大普陀（relation 6179557，90 点、中途闭合 + 尾点）──
    let ecnu_ring = ecnu_putuo_ring_wgs84();
    assert_eq!(ecnu_ring.len(), 90, "罐头夹具必须是真实 90 点环");
    let closure_index = ecnu_ring
        .iter()
        .rposition(|point| *point == ecnu_ring[0])
        .expect("夹具必须包含中途闭合点（首点重复出现）");
    assert_eq!(
        closure_index + 1,
        ecnu_ring.len() - 2,
        "夹具必须复现'闭合节点不在末尾'（闭合在第 {closure_index} 点，其后应只剩 2 个尾点）"
    );
    let ecnu_fetcher = canned_fetcher(
        canned_overpass_relation(&ecnu_ring),
        canned_nominatim("relation", 6179557, "华东师范大学"),
    );
    let ecnu_gcj02 = fetch_canned_campus(
        ecnu_fetcher,
        "华东师范大学普陀校区",
        121.40468,
        31.227938,
        data_acquisition::overpass::BoundarySourceKind::NominatimByElementId,
    );
    // T33 修复断言：中途闭合后的尾点必须被 normalize_closed_ring 清理，
    // 进入确认链路的环不再带共享端点尾边。
    assert!(
        ecnu_gcj02.len() < ecnu_ring.len(),
        "归一化必须截断尾点：raw={} normalized={}",
        ecnu_ring.len(),
        ecnu_gcj02.len()
    );

    let payload = format!(
        r#"{{"type":"confirm_boundary","coords":{}}}"#,
        serde_json::to_string(&ecnu_gcj02).expect("序列化 GCJ-02 环")
    );
    window.invoke_workspace_map_ipc(payload.into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "华东师大普陀真实 OSM 环必须通过确认校验（T33 中途闭合场景）"
    );
    assert!(
        !window.get_error_dialog_visible(),
        "有效 OSM 环不得弹出错误弹窗"
    );
    let locked: Vec<bool> = (0..5)
        .map(|i| window.get_workspace_step_locked().row_data(i).unwrap())
        .collect();
    assert!(
        locked.iter().all(|locked| !*locked),
        "确认后五步必须全部解锁：{locked:?}"
    );

    // ── 2. 上交闵行（way 288249651，39 点、末尾闭合）──
    window.invoke_workspace_boundary_reset_clicked();
    assert!(
        !window.get_workspace_boundary_is_determined(),
        "重置后边界回到未确认"
    );
    let sjtu_ring = sjtu_minhang_ring_wgs84();
    assert_eq!(sjtu_ring.len(), 39, "罐头夹具必须是真实 39 点环");
    let sjtu_fetcher = canned_fetcher(
        canned_overpass_way(&sjtu_ring),
        canned_nominatim("way", 288249651, "上海交通大学（闵行校区）"),
    );
    let sjtu_gcj02 = fetch_canned_campus(
        sjtu_fetcher,
        "上海交通大学(闵行本部校区)",
        121.433,
        31.028,
        data_acquisition::overpass::BoundarySourceKind::NominatimByElementId,
    );
    let payload = format!(
        r#"{{"type":"confirm_boundary","coords":{}}}"#,
        serde_json::to_string(&sjtu_gcj02).expect("序列化 GCJ-02 环")
    );
    window.invoke_workspace_map_ipc(payload.into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "上交闵行真实 OSM 环必须通过确认校验"
    );
    assert!(
        !window.get_error_dialog_visible(),
        "有效 OSM 环不得弹出错误弹窗"
    );
}
