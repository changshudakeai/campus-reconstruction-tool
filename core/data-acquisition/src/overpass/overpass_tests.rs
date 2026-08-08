//! Overpass/Nominatim 单元测试（T31）：union 查询、端点回退、Nominatim 解析、
//! WGS→GCJ、边界级联、真实链路冒烟（`--ignored`）。

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::overpass::*;

    fn bbox() -> (f64, f64, f64, f64) {
        (31.0, 121.4, 31.1, 121.5)
    }

    #[test]
    fn union_query_has_no_pipe_regex() {
        let query = university_query(bbox());
        assert!(!query.contains('|'), "union 写法禁止 | 正则: {query}");
        assert!(query.contains("way[\"amenity\"=\"university\"]"));
        assert!(query.contains("relation[\"amenity\"=\"college\"]"));
        assert!(query.contains("out geom"));
    }

    #[test]
    fn buildings_query_uses_union_and_keeps_labels() {
        let query = buildings_query(bbox());
        assert!(!query.contains('|'));
        assert!(query.contains("way[\"building\"]"));
        assert!(query.contains("relation[\"building\"]"));
        assert!(query.contains("out geom"));
    }

    #[test]
    fn landuse_query_uses_union() {
        let query = landuse_education_query(bbox());
        assert!(!query.contains('|'));
        assert!(query.contains("way[\"landuse\"=\"education\"]"));
        assert!(query.contains("relation[\"landuse\"=\"education\"]"));
    }

    #[test]
    fn by_id_query_targets_element() {
        let query = element_by_id_query("way", 144183801);
        assert!(query.contains("way(144183801);out geom;"));
        assert!(!query.contains('|'));
    }

    #[test]
    fn query_url_uses_data_parameter() {
        let query = university_query(bbox());
        let url = format!(
            "https://overpass-api.de/api/interpreter?data={}",
            encode_query(&query)
        );
        assert!(
            url.contains("?data=%5Bout%3Ajson%5D"),
            "data= 参数必须存在: {url}"
        );
        assert!(url.contains("%3A"), "查询体必须百分号编码");
    }

    #[test]
    fn encode_query_handles_utf8_and_syntax() {
        assert_eq!(
            encode_query("上海交通大学"),
            "%E4%B8%8A%E6%B5%B7%E4%BA%A4%E9%80%9A%E5%A4%A7%E5%AD%A6"
        );
        assert_eq!(encode_query("a b"), "a%20b");
        assert_eq!(encode_query("[out:json]"), "%5Bout%3Ajson%5D");
        assert_eq!(encode_query("abc-_.~"), "abc-_.~");
    }

    #[test]
    fn endpoint_fallback_tries_next_endpoint_on_failure() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let transport = Box::new(move |url: &str, _timeout: Duration| {
            calls_clone.lock().unwrap().push(url.to_owned());
            if url.contains("overpass-api.de") {
                Err("连接超时".to_owned())
            } else {
                Ok(r#"{"elements":[{"type":"way","id":1}]}"#.to_owned())
            }
        });
        let client = OverpassClient::with_transport(transport);
        let body = client.query_with_fallback("q").unwrap();
        assert!(body.contains("id\":1"));
        let urls = calls.lock().unwrap();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("overpass-api.de"));
        assert!(urls[1].contains("kumi"));
    }

    #[test]
    fn endpoint_fallback_skips_error_pages() {
        let transport = Box::new(|url: &str, _timeout: Duration| {
            if url.contains("kumi") {
                Ok("parse error: Unknown type \"%\"".to_owned())
            } else {
                Ok(r#"{"elements":[]}"#.to_owned())
            }
        });
        let client = OverpassClient::with_transport(transport);
        let body = client.query_with_fallback("q").unwrap();
        assert!(body.contains("elements"));
    }

    #[test]
    fn all_endpoints_down_reports_structured_error() {
        let transport = Box::new(|url: &str, _timeout: Duration| {
            let _ = url;
            Err("网络不可达".to_owned())
        });
        let client = OverpassClient::with_transport(transport);
        let error = client.query_with_fallback("q").unwrap_err();
        assert!(error.contains("https://overpass-api.de"));
        assert!(error.contains("https://overpass.kumi.systems"));
        assert!(error.contains("https://maps.mail.ru"));
    }

    #[test]
    fn nominatim_parse_picks_university_way_not_railway_node() {
        let json = r#"[
            {"osm_type":"node","osm_id":3800185706,"class":"railway","type":"stop","display_name":"交通大学"},
            {"osm_type":"way","osm_id":144183801,"class":"amenity","type":"university","display_name":"上海交通大学（徐汇校区）"}
        ]"#;
        let results = parse_nominatim_results(json);
        assert_eq!(results.len(), 1, "node 干扰项应被过滤");
        assert_eq!(results[0].osm_type, "way");
        assert_eq!(results[0].osm_id, 144183801);
        assert_eq!(results[0].kind, "university");
    }

    #[test]
    fn nominatim_resolution_falls_back_to_stripped_name() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let transport = Box::new(move |url: &str, _timeout: Duration| {
            calls_clone.lock().unwrap().push(url.to_owned());
            if url.contains("%28%E9%97%B5%E8%A1%8C%E6%9C%AC%E9%83%A8%E6%A0%A1%E5%8C%BA%29") {
                Ok("[]".to_owned())
            } else {
                Ok(r#"[{"osm_type":"way","osm_id":288249651,"class":"amenity","type":"university","display_name":"上海交通大学（闵行校区）"}]"#.to_owned())
            }
        });
        let client = NominatimClient::with_transport(transport);
        let matched = client
            .resolve_campus("上海交通大学(闵行本部校区)")
            .unwrap()
            .expect("去掉括号后缀后应命中");
        assert_eq!(matched.osm_id, 288249651);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "先精确查询，失败后去括号再查");
    }

    #[test]
    fn campus_name_candidates_strips_parentheses() {
        assert_eq!(
            campus_name_candidates("上海交通大学(闵行本部校区)"),
            vec![
                "上海交通大学(闵行本部校区)".to_owned(),
                "上海交通大学".to_owned()
            ]
        );
        assert_eq!(
            campus_name_candidates("上海交通大学（徐汇校区）"),
            vec![
                "上海交通大学（徐汇校区）".to_owned(),
                "上海交通大学".to_owned()
            ]
        );
        assert_eq!(
            campus_name_candidates("上海交通大学"),
            vec!["上海交通大学".to_owned()]
        );
    }

    #[test]
    fn parse_elements_handles_way_and_relation_outer_members() {
        let json = r#"{"elements":[
            {"type":"way","id":11,"tags":{"name":"A楼"},"geometry":[{"lat":31.0,"lon":121.4},{"lat":31.1,"lon":121.5}]},
            {"type":"relation","id":22,"tags":{"name":"校园"},"members":[
                {"type":"way","ref":1,"role":"outer","geometry":[{"lat":31.0,"lon":121.4}]},
                {"type":"way","ref":2,"role":"outer","geometry":[{"lat":31.1,"lon":121.5}]},
                {"type":"way","ref":3,"role":"inner","geometry":[{"lat":31.2,"lon":121.6}]}
            ]}
        ]}"#;
        let elements = parse_elements(json);
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].geometry.as_ref().unwrap().len(), 2);
        assert_eq!(
            elements[1].geometry.as_ref().unwrap(),
            &[[121.4, 31.0], [121.5, 31.1]],
            "relation 只拼接 outer/空 role 成员"
        );
    }

    #[test]
    fn boundary_bbox_covers_polygon_and_multipolygon() {
        let boundary = Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[[121.40, 31.20], [121.41, 31.20], [121.41, 31.21]]]),
        };
        let (s, w, n, e) = boundary_bbox(&boundary, 0.01).unwrap();
        assert!(s < 31.20 && w < 121.40 && n > 31.21 && e > 121.41);

        let multi = Boundary {
            r#type: "MultiPolygon".to_owned(),
            coordinates: serde_json::json!([[[[121.40, 31.20], [121.41, 31.21]]]]),
        };
        assert!(boundary_bbox(&multi, 0.0).is_some());
    }

    #[test]
    fn select_best_converts_wgs84_to_gcj02_before_sorting() {
        // 锚点 GCJ-02 ≈ (121.433, 31.029)；元素 WGS-84 环应被转为 GCJ-02 后参与
        let json = r#"{"elements":[{"type":"way","id":288249651,"tags":{"name":"上海交通大学（闵行校区）"},"geometry":[
            {"lat":31.0295,"lon":121.4184},{"lat":31.03,"lon":121.43},{"lat":31.02,"lon":121.44},{"lat":31.0295,"lon":121.4184}
        ]}]}"#;
        let best = select_best(json, 121.433, 31.029, "上海交通大学(闵行本部校区)").unwrap();
        assert_eq!(best.name, "上海交通大学（闵行校区）");
        assert!(
            best.geometry.iter().any(|p| (p[0] - 121.433).abs() < 0.02),
            "几何必须已转 GCJ-02"
        );
        assert_eq!(best.candidate_count, 1);
    }

    #[test]
    fn fetcher_uses_nominatim_by_id_path() {
        let encoded_by_id = encode_query("way(288249651)");
        let overpass = OverpassClient::with_transport(Box::new(move |url: &str, _: Duration| {
            if url.contains(&encoded_by_id) {
                Ok(r#"{"elements":[{"type":"way","id":288249651,"tags":{"name":"上海交通大学（闵行校区）"},"geometry":[{"lat":31.0295,"lon":121.4184},{"lat":31.03,"lon":121.43},{"lat":31.02,"lon":121.44},{"lat":31.0295,"lon":121.4184}]}]}"#.to_owned())
            } else {
                Ok(r#"{"elements":[]}"#.to_owned())
            }
        }));
        let nominatim = NominatimClient::with_transport(Box::new(|_: &str, _: Duration| {
            Ok(r#"[{"osm_type":"way","osm_id":288249651,"class":"amenity","type":"university","display_name":"上海交通大学（闵行校区）"}]"#.to_owned())
        }));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);
        match fetcher.fetch_campus("上海交通大学", 121.433, 31.029) {
            CampusBoundaryResult::AutoSelected { source, gcj02, .. } => {
                assert_eq!(source, BoundarySourceKind::NominatimByElementId);
                assert!(!gcj02.is_empty());
            }
            other => panic!("期望自动选中，得到 {other:?}"),
        }
    }

    #[test]
    fn fetcher_falls_back_to_amenity_nearby_when_nominatim_empty() {
        let overpass = OverpassClient::with_transport(Box::new(|url: &str, _: Duration| {
            if url.contains("amenity%22%3D%22university") {
                Ok(r#"{"elements":[{"type":"way","id":288249651,"tags":{"name":"上海交通大学（闵行校区）"},"geometry":[{"lat":31.0295,"lon":121.4184},{"lat":31.03,"lon":121.43},{"lat":31.02,"lon":121.44},{"lat":31.0295,"lon":121.4184}]}]}"#.to_owned())
            } else {
                Ok(r#"{"elements":[]}"#.to_owned())
            }
        }));
        let nominatim =
            NominatimClient::with_transport(Box::new(|_: &str, _: Duration| Ok("[]".to_owned())));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);
        match fetcher.fetch_campus("上海交通大学", 121.433, 31.029) {
            CampusBoundaryResult::AutoSelected {
                source,
                candidate_count,
                ..
            } => {
                assert_eq!(source, BoundarySourceKind::OverpassAmenity);
                assert_eq!(candidate_count, 1);
            }
            other => panic!("期望 amenity 近域兜底，得到 {other:?}"),
        }
    }

    #[test]
    fn fetcher_reports_not_found_when_all_sources_empty() {
        let overpass = OverpassClient::with_transport(Box::new(|_: &str, _: Duration| {
            Ok(r#"{"elements":[]}"#.to_owned())
        }));
        let nominatim =
            NominatimClient::with_transport(Box::new(|_: &str, _: Duration| Ok("[]".to_owned())));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);
        assert_eq!(
            fetcher.fetch_campus("示例大学", 121.4, 31.2),
            CampusBoundaryResult::NotFound
        );
    }

    #[test]
    fn fetcher_reports_unreachable_with_message() {
        let overpass = OverpassClient::with_transport(Box::new(|_: &str, _: Duration| {
            Err("全部端点不可达".to_owned())
        }));
        let nominatim = NominatimClient::with_transport(Box::new(|_: &str, _: Duration| {
            Err("Nominatim 超时".to_owned())
        }));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);
        match fetcher.fetch_campus("示例大学", 121.4, 31.2) {
            CampusBoundaryResult::Unreachable { message } => {
                assert!(message.contains("Nominatim 超时"));
                assert!(message.contains("全部端点不可达"));
            }
            other => panic!("期望 Unreachable，得到 {other:?}"),
        }
    }

    /// 真实链路冒烟（不进 CI；T31 交接证据）：
    /// 上海交通大学（闵行本部校区，高德校区名 + GCJ-02 锚点近似值）
    /// → Nominatim → by-id → WGS→GCJ → 排序 → 自动选中。
    #[test]
    #[ignore = "真实网络：仅用于 T31 实施窗口实测证据"]
    fn live_shanghai_jiaotong_boundary_fetch() {
        let fetcher = CampusBoundaryFetcher::production();
        let outcome = fetcher.fetch_campus("上海交通大学(闵行本部校区)", 121.433, 31.028);
        match outcome {
            CampusBoundaryResult::AutoSelected {
                name,
                gcj02,
                source,
                candidate_count,
            } => {
                assert!(!gcj02.is_empty(), "必须拿到面坐标");
                assert!(candidate_count > 0);
                // 实测输出（T31 证据）：LIVE_OK name=… source=… candidates=… points=…
                assert!(
                    !name.is_empty() && !source.to_string().is_empty(),
                    "LIVE_OK name={name} source={source} candidates={candidate_count} points={}",
                    gcj02.len()
                );
            }
            other => panic!("真实链路未自动选中：{other:?}"),
        }
    }

    /// 真实采集链路冒烟（不进 CI；T31 交接证据）：
    /// OverpassDataSource（生产 transport：union building=* + 端点回退）
    /// 对交大闵行边界实拉 → 面候选 > 0、OSM name 优先、WGS→GCJ 已转。
    #[test]
    #[ignore = "真实网络：仅用于 T31 实施窗口实测证据"]
    fn live_building_collection_via_overpass_source() {
        use crate::source::{DataSource, OverpassDataSource, SourceGeometry};
        use shared_domain_types::Boundary;

        let boundary = Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [121.42, 31.02],
                [121.44, 31.02],
                [121.44, 31.04],
                [121.42, 31.04],
                [121.42, 31.02]
            ]]),
        };
        let transport = Box::new(move |b: &Boundary| {
            let bbox = boundary_bbox(b, 0.01).ok_or_else(|| "无包围盒".to_owned())?;
            OverpassClient::production()
                .query_with_fallback(&buildings_query(bbox))
                .map_err(|message| format!("采集查询失败：{message}"))
        });
        let source = OverpassDataSource::new(transport);
        let entities = source
            .fetch_raw_entities(&boundary)
            .expect("真实采集必须成功");
        let polygons = entities
            .iter()
            .filter(|e| matches!(e.source_geometry, Some(SourceGeometry::Polygon(_))))
            .count();
        let named = entities.iter().filter(|e| e.name != e.entity_id).count();
        assert!(polygons > 0, "面候选必须 > 0，实际 {polygons}");
        assert!(named > 0, "OSM name 必须存在，实际 {named}");
        // WGS-84 → GCJ-02 已在入口完成：面首点应与原始 WGS 不同
        let first_polygon = entities
            .iter()
            .find_map(|e| match &e.source_geometry {
                Some(SourceGeometry::Polygon(points)) => Some(points.clone()),
                _ => None,
            })
            .expect("存在面");
        assert!(
            (first_polygon[0].0 - 121.42).abs() > 0.0005,
            "几何必须已转 GCJ-02: {:?}",
            first_polygon[0]
        );
        assert!(
            polygons >= named / 2,
            "采集报告应同时具备面候选与名称：polygons={polygons} named={named}"
        );
    }

    #[test]
    fn normalize_closed_ring_trims_tail_after_midway_closure() {
        // T33 回归：OSM 环在中途闭合（v4 == v0）后仍带尾点 →
        // 归一化为干净单环，避免确认校验把共享端点误判为自相交。
        let ring = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
            [5.0, 12.0],
            [4.0, 12.5],
        ];
        assert_eq!(
            normalize_closed_ring(ring),
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
        );
    }

    #[test]
    fn normalize_closed_ring_keeps_clean_ring_and_drops_trailing_closure_duplicate() {
        let clean = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert_eq!(normalize_closed_ring(clean.clone()), clean);

        let closed = vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ];
        assert_eq!(normalize_closed_ring(closed), clean);
    }
}
