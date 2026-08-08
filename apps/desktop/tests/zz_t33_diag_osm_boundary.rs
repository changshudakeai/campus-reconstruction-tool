//! T33 诊断（临时）：用真实 OSM 校区边界跑一遍确认路径的转换+校验，
//! 定位“确认边界后无反应”是否为真实多边形校验失败。
use data_acquisition::overpass::{CampusBoundaryFetcher, CampusBoundaryResult};
use foundation_mode::{validate_polygon_closure, CoordinateConverter, MercatorCoord, Vertex};

fn centroid(coords: &[[f64; 2]]) -> Option<(f64, f64)> {
    if coords.is_empty() {
        return None;
    }
    let n = coords.len() as f64;
    let lon = coords.iter().map(|c| c[0]).sum::<f64>() / n;
    let lat = coords.iter().map(|c| c[1]).sum::<f64>() / n;
    Some((lon, lat))
}

#[test]
#[ignore = "真实网络（Nominatim/Overpass）冒烟：华东师大普陀校区边界必须通过确认校验"]
fn diag_real_osm_boundary_passes_confirm_validation() {
    let fetcher = CampusBoundaryFetcher::production();
    let outcome = fetcher.fetch_campus("华东师范大学普陀校区", 121.40468, 31.227938);
    match outcome {
        CampusBoundaryResult::AutoSelected {
            name,
            gcj02,
            source,
            candidate_count,
        } => {
            let coords: Vec<[f64; 2]> = gcj02.iter().map(|p| [p[0], p[1]]).collect();
            assert!(
                coords.len() >= 3,
                "真实 OSM 边界点过少：name={name} source={source:?} candidates={candidate_count} points={}",
                coords.len()
            );
            let (center_lon, center_lat) = centroid(&coords).expect("centroid");
            let mut converter = CoordinateConverter::default();
            converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
            let mut vertices = Vec::with_capacity(coords.len());
            for [lon, lat] in &coords {
                let mercator = MercatorCoord::from_lat_lon(*lat, *lon);
                let plane = converter.mercator_to_plane(mercator).expect("坐标转换失败");
                vertices.push(Vertex::new(plane.x, plane.y));
            }
            let validation = validate_polygon_closure(&vertices);
            assert!(
                validation.is_valid,
                "真实 OSM 边界必须通过确认校验：name={name} source={source:?} \
                 candidates={candidate_count} points={} validation={validation:?}",
                coords.len()
            );
        }
        other => panic!("真实 OSM 抓取未返回 AutoSelected：{other:?}"),
    }
}
