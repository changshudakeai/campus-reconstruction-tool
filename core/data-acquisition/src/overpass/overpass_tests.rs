//! Overpass/Nominatim 单元测试（T31）：union 查询、端点回退、Nominatim 解析、
//! WGS→GCJ、边界级联、真实链路冒烟（`--ignored`）。

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::overpass::*;
    use shared_domain_types::Boundary;

    fn bbox() -> (f64, f64, f64, f64) {
        (31.0, 121.4, 31.1, 121.5)
    }

    fn test_retry_policy() -> RetryPolicy {
        RetryPolicy {
            request_timeout: Duration::from_secs(1),
            max_rounds: 2,
            retry_backoff: Duration::ZERO,
            transient_cooldown: Duration::ZERO,
            overloaded_cooldown: Duration::from_secs(60),
        }
    }

    fn request_failure(kind: FailureKind, message: &str) -> RequestFailure {
        RequestFailure {
            kind,
            message: message.to_owned(),
        }
    }

    fn recording_production_client() -> (OverpassClient, Arc<Mutex<Vec<String>>>) {
        let paths = Arc::new(Mutex::new(Vec::new()));
        let paths_for_transport = Arc::clone(&paths);
        let real = reliability::production_transport(ureq_agent().expect("TLS agent"));
        let transport: RequestTransport = Arc::new(move |request, timeout| {
            paths_for_transport
                .lock()
                .unwrap()
                .push(request.endpoint.to_owned());
            real(request, timeout)
        });
        (
            OverpassClient::with_request_transport_and_policy(transport, production_retry_policy()),
            paths,
        )
    }

    #[test]
    fn endpoint_limits_are_bounded_per_ticket() {
        assert_eq!(OVERPASS_HTTP_TIMEOUT, Duration::from_secs(25));
        assert_eq!(OVERPASS_QUERY_DEADLINE, Duration::from_secs(90));
    }

    #[test]
    fn server_timeout_stays_below_client_timeout() {
        // 客户端 5s 放弃后，服务端必须更早取消（25s > 5s 的历史值会让公共端点
        // 在客户端断开后继续占用算力，是“端点负载起伏”的一个自制造因）。
        assert!(
            Duration::from_secs(u64::from(OVERPASS_SERVER_TIMEOUT_SECS)) < OVERPASS_HTTP_TIMEOUT,
            "服务端超时必须小于客户端超时"
        );
    }

    #[test]
    fn query_fallback_prefers_recently_successful_endpoint() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_clone = calls.clone();
        let transport = Box::new(move |url: &str, _timeout: Duration| {
            calls_clone.lock().unwrap().push(url.to_owned());
            if url.contains("overpass.kumi.systems") {
                Ok(r#"{"elements":[{"type":"way","id":1}]}"#.to_owned())
            } else {
                Err("端点挂起".to_owned())
            }
        });
        let client = OverpassClient::with_transport(transport);

        // 第一次：de 失败 → kumi 成功（自适应记住 kumi）
        client.query_with_fallback("q").unwrap();
        assert_eq!(client.endpoint_order()[0], OVERPASS_ENDPOINTS[1]);

        // 第二次：直接先试 kumi，不再在已知坏的 de 上空等
        client.query_with_fallback("q").unwrap();
        let urls = calls.lock().unwrap();
        assert_eq!(urls.len(), 3);
        assert!(urls[0].contains("overpass-api.de"));
        assert!(urls[1].contains("kumi"));
        assert!(urls[2].contains("kumi"), "第二次必须先试最近成功的端点");
    }

    #[test]
    fn query_fallback_emits_per_endpoint_progress() {
        let progress = std::cell::RefCell::new(Vec::new());
        let transport = Box::new(|_: &str, _timeout: Duration| Err("全部失败".to_owned()));
        let client = OverpassClient::with_transport(transport);
        let result = client.query_with_fallback_progress("q", FetchStage::ByElementId, &|p| {
            progress.borrow_mut().push(p);
        });
        assert!(result.is_err());
        let events = progress.into_inner();
        assert!(events.len() <= OVERPASS_ENDPOINTS.len() * usize::from(OVERPASS_MAX_ROUNDS));
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.stage, FetchStage::ByElementId);
            assert_eq!(event.attempt, index as u32 + 1);
            assert_eq!(
                event.total_attempts,
                (OVERPASS_ENDPOINTS.len() * usize::from(OVERPASS_MAX_ROUNDS)) as u32
            );
        }
    }

    #[test]
    fn boundary_cascade_keeps_failed_endpoint_cold_between_queries() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_transport = Arc::clone(&calls);
        let transport: RequestTransport = Arc::new(move |request, _| {
            let mut calls = calls_for_transport.lock().unwrap();
            calls.push(request.endpoint);
            match calls.len() {
                1 => Err(request_failure(FailureKind::RateLimited, "繁忙")),
                2 => Ok(r#"{"elements":[]}"#.to_owned()),
                3 => Err(request_failure(FailureKind::Connection, "断开")),
                _ => Ok(r#"{"elements":[{"type":"way","id":2,"tags":{"name":"示例大学"},"geometry":[{"lat":31.02,"lon":121.42},{"lat":31.03,"lon":121.43},{"lat":31.02,"lon":121.44},{"lat":31.02,"lon":121.42}]}]}"#.to_owned()),
            }
        });
        let overpass = OverpassClient::with_request_transport_and_policy(
            transport,
            RetryPolicy {
                max_rounds: 1,
                ..test_retry_policy()
            },
        );
        let nominatim = NominatimClient::with_transport(Box::new(|_, _| {
            Ok(r#"[{"osm_type":"way","osm_id":1,"class":"amenity","type":"university","display_name":"示例大学"}]"#.to_owned())
        }));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);

        assert!(matches!(
            fetcher.fetch_campus("示例大学", 121.43, 31.025),
            CampusBoundaryResult::AutoSelected { .. }
        ));
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                OVERPASS_ENDPOINTS[0],
                OVERPASS_ENDPOINTS[1],
                OVERPASS_ENDPOINTS[1],
                OVERPASS_ENDPOINTS[2],
            ],
            "by-id 中被 429 冷却的 de 不得在 amenity 查询立刻重试"
        );
    }

    #[test]
    fn fetch_campus_failure_names_the_failing_stage() {
        let overpass = OverpassClient::with_transport(Box::new(|_: &str, _: Duration| {
            Err("三个公共端点全部不可达".to_owned())
        }));
        let nominatim = NominatimClient::with_transport(Box::new(|_: &str, _: Duration| {
            Err("Nominatim 请求失败".to_owned())
        }));
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, nominatim);
        let progress = std::cell::RefCell::new(Vec::new());
        match fetcher.fetch_campus_with_progress("示例大学", 121.4, 31.2, &|p| {
            progress.borrow_mut().push(p);
        }) {
            CampusBoundaryResult::Unreachable { message } => {
                assert!(
                    message.contains("校名解析失败"),
                    "必须点名第一步失败：{message}"
                );
                assert!(
                    message.contains("amenity 近域查询失败"),
                    "必须点名兜底步骤失败：{message}"
                );
            }
            other => panic!("期望 Unreachable，得到 {other:?}"),
        }
        let events = progress.into_inner();
        assert!(
            events.iter().any(|p| p.stage == FetchStage::CampusName),
            "必须先上报校名解析阶段"
        );
        assert!(
            events.iter().any(|p| p.stage == FetchStage::Amenity),
            "必须上报 amenity 兜底阶段"
        );
    }

    #[test]
    fn three_endpoints_all_hang_respect_overall_query_deadline() {
        // 注入“三端点全挂 Overpass transport”：每个端点睡满超时后失败。
        // 测试策略把预算压到毫秒级，验证整体截止并保留全部端点诊断。
        let transport: RequestTransport = Arc::new(|_, timeout: Duration| {
            // 无卡顿铁律禁用 std::thread::sleep；用 recv_timeout 等满超时
            let (_tx, rx) = std::sync::mpsc::channel::<()>();
            let _ = rx.recv_timeout(timeout);
            Err(request_failure(FailureKind::Timeout, "模拟端点挂起"))
        });
        let client = OverpassClient::with_request_transport_and_policy(
            transport,
            RetryPolicy {
                request_timeout: Duration::from_millis(10),
                max_rounds: 1,
                retry_backoff: Duration::ZERO,
                transient_cooldown: Duration::ZERO,
                overloaded_cooldown: Duration::ZERO,
            },
        );
        let started = Instant::now();
        let result = client.query_with_fallback(&university_query(bbox()));
        let elapsed = started.elapsed();

        let message = result.expect_err("三端点全挂必须失败");
        for endpoint in OVERPASS_ENDPOINTS {
            assert!(
                message.contains(endpoint),
                "错误必须保留每个端点事实：{message}"
            );
        }
        assert!(
            elapsed <= Duration::from_secs(1),
            "整体查询不得超过截止时间：{elapsed:?}"
        );
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
    fn campus_objects_query_uses_central_rules_with_geometry_aware_selectors() {
        let query = campus_objects_query(bbox()).expect("集中标签规则必须能生成查询");

        for selector in [
            "way[\"building\"=\"school\"]",
            "way[\"highway\"]",
            "way[\"natural\"=\"water\"]",
            "way[\"landuse\"=\"grass\"]",
            "way[\"leisure\"=\"pitch\"]",
            "way[\"barrier\"=\"wall\"]",
        ] {
            assert!(
                query.contains(selector),
                "六类集中规则生成的查询缺少 {selector}: {query}"
            );
        }
        assert!(
            query.contains("node[\"natural\"=\"tree\"]"),
            "树点是合理的 node selector"
        );
        assert!(
            query.contains("node[\"barrier\"=\"gate\"]"),
            "校门点是合理的 node selector"
        );
        assert!(
            !query.contains("node[\"building\""),
            "明显面状的建筑不得无意义查询 node"
        );
        assert!(
            !query.contains("relation[\"highway\"]"),
            "道路粗查询只取实际几何 way，不取路线 relation"
        );
        for rejected_broad_key in ["historic", "power", "man_made"] {
            assert!(
                !query.contains(&format!("[\"{rejected_broad_key}\"]")),
                "宽规则 {rejected_broad_key}=* 缺少后续形态过滤，禁止直接拉全量"
            );
        }
        assert!(query.contains("out geom"));
    }

    #[test]
    fn campus_object_queries_split_transport_without_losing_six_category_selectors() {
        let shards = campus_object_query_shards(bbox()).expect("集中标签规则必须能生成有界子查询");
        assert!(
            (3..=5).contains(&shards.len()),
            "大型校园查询应拆成少量真实重试单元，得到 {} 个",
            shards.len()
        );
        let combined = shards
            .iter()
            .map(|shard| shard.query.as_str())
            .collect::<String>();
        for selector in [
            "way[\"building\"=\"school\"]",
            "way[\"highway\"]",
            "way[\"natural\"=\"water\"]",
            "way[\"landuse\"=\"grass\"]",
            "way[\"leisure\"=\"pitch\"]",
            "way[\"barrier\"=\"wall\"]",
        ] {
            assert!(
                combined.contains(selector),
                "拆分后丢失六类集中规则 selector {selector}"
            );
        }
        assert!(shards.iter().all(|shard| {
            shard.query.contains("out geom")
                && shard
                    .query
                    .contains(&format!("[timeout:{OVERPASS_SERVER_TIMEOUT_SECS}]"))
        }));
    }

    #[test]
    fn campus_subqueries_use_post_bodies_and_retain_success_while_retrying_failed_shard() {
        let calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let calls_for_transport = Arc::clone(&calls);
        let per_body_calls = Arc::new(Mutex::new(
            std::collections::BTreeMap::<String, usize>::new(),
        ));
        let per_body_for_transport = Arc::clone(&per_body_calls);
        let transport: RequestTransport = Arc::new(move |request, _| {
            calls_for_transport
                .lock()
                .unwrap()
                .push((request.endpoint.to_owned(), request.body.clone()));
            let mut counts = per_body_for_transport.lock().unwrap();
            let shard_number = counts.len() + usize::from(!counts.contains_key(&request.body));
            let count = counts.entry(request.body.clone()).or_default();
            *count += 1;
            let is_second_shard = shard_number == 2;
            if is_second_shard && *count < 3 {
                Err(request_failure(FailureKind::Timeout, "模拟前两节点超时"))
            } else {
                Ok(format!(
                    r#"{{"elements":[{{"type":"way","id":{}}}]}}"#,
                    shard_number
                ))
            }
        });
        let client =
            OverpassClient::with_request_transport_and_policy(transport, test_retry_policy());

        let body = client
            .query_campus_objects(bbox(), Instant::now() + Duration::from_secs(5), &|| {})
            .expect("前两节点失败后第三节点应完成失败分片，其余成功分片不重下");

        assert!(body.contains("elements"));
        let counts = per_body_calls.lock().unwrap();
        assert_eq!(counts.values().filter(|count| **count == 1).count(), 3);
        assert_eq!(counts.values().filter(|count| **count == 3).count(), 1);
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls[..4]
                .iter()
                .map(|(endpoint, _)| endpoint.as_str())
                .collect::<Vec<_>>(),
            vec![
                OVERPASS_ENDPOINTS[0],
                OVERPASS_ENDPOINTS[0],
                OVERPASS_ENDPOINTS[1],
                OVERPASS_ENDPOINTS[2]
            ],
            "失败分片必须顺序尝试前两节点，并由第三节点成功"
        );
        for (endpoint, form_body) in calls.iter() {
            assert!(!endpoint.contains("data="), "查询不得进入长 URL");
            assert!(form_body.starts_with("data="), "查询必须进入 POST 表单体");
        }
    }

    #[test]
    fn rate_limit_and_gateway_timeout_cool_nodes_for_later_shards() {
        let endpoints = Arc::new(Mutex::new(Vec::<String>::new()));
        let endpoints_for_transport = Arc::clone(&endpoints);
        let transport: RequestTransport = Arc::new(move |request, _| {
            endpoints_for_transport
                .lock()
                .unwrap()
                .push(request.endpoint.to_owned());
            if request.endpoint == OVERPASS_ENDPOINTS[0] {
                Err(request_failure(
                    FailureKind::RateLimited,
                    "Too Many Requests",
                ))
            } else if request.endpoint == OVERPASS_ENDPOINTS[1] {
                Err(request_failure(
                    FailureKind::GatewayTimeout,
                    "Gateway Timeout",
                ))
            } else {
                Ok(r#"{"elements":[]}"#.to_owned())
            }
        });
        let client =
            OverpassClient::with_request_transport_and_policy(transport, test_retry_policy());

        client
            .query_campus_objects(bbox(), Instant::now() + Duration::from_secs(5), &|| {})
            .unwrap();

        let endpoints = endpoints.lock().unwrap();
        assert_eq!(&endpoints[..3], &OVERPASS_ENDPOINTS.map(str::to_owned));
        assert!(
            endpoints[3..]
                .iter()
                .all(|endpoint| endpoint == OVERPASS_ENDPOINTS[2]),
            "429/504 节点冷却后，后续成功分片应优先健康节点：{endpoints:?}"
        );
    }

    #[test]
    fn failure_diagnostics_distinguish_429_504_error_page_and_parse_failure() {
        let cases = [
            (
                FailureKind::RateLimited,
                "HTTP 429 节点限流",
                "Too Many Requests",
            ),
            (
                FailureKind::GatewayTimeout,
                "HTTP 504 节点超时",
                "Gateway Timeout",
            ),
        ];
        for (kind, expected, message) in cases {
            let transport: RequestTransport =
                Arc::new(move |_, _| Err(request_failure(kind, message)));
            let client = OverpassClient::with_request_transport_and_policy(
                transport,
                RetryPolicy {
                    max_rounds: 1,
                    ..test_retry_policy()
                },
            );
            let error = client.query_with_fallback("q").unwrap_err();
            assert!(error.contains(expected), "{error}");
        }

        let error_page: RequestTransport =
            Arc::new(|_, _| Ok("<html><body>Runtime error: overloaded</body></html>".to_owned()));
        let client = OverpassClient::with_request_transport_and_policy(
            error_page,
            RetryPolicy {
                max_rounds: 1,
                ..test_retry_policy()
            },
        );
        assert!(client
            .query_with_fallback("q")
            .unwrap_err()
            .contains("服务端错误页"));

        let malformed: RequestTransport = Arc::new(|_, _| Ok("not-json".to_owned()));
        let client = OverpassClient::with_request_transport_and_policy(
            malformed,
            RetryPolicy {
                max_rounds: 1,
                ..test_retry_policy()
            },
        );
        assert!(client
            .query_with_fallback("q")
            .unwrap_err()
            .contains("响应解析失败"));
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
    fn post_body_uses_data_parameter() {
        let query = university_query(bbox());
        let body = format!("data={}", encode_query(&query));
        assert!(
            body.starts_with("data=%5Bout%3Ajson%5D"),
            "POST 请求体必须从 data= 查询参数开始: {body}"
        );
        assert!(body.contains("%3A"), "查询体必须百分号编码");
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
    fn nominatim_reuses_confirmed_resolution_and_rejects_malformed_payload() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_transport = Arc::clone(&calls);
        let client = NominatimClient::with_transport(Box::new(move |_, _| {
            calls_for_transport.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(r#"[{"osm_type":"way","osm_id":288249651,"class":"amenity","type":"university","display_name":"上海交通大学"}]"#.to_owned())
        }));

        client.resolve_campus("上海交通大学").unwrap();
        client.resolve_campus("上海交通大学").unwrap();
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "同一运行中已确认的校名解析必须复用，避免重复请求公共 Nominatim"
        );

        let malformed = NominatimClient::with_transport(Box::new(|_, _| {
            Ok("<html>upstream overloaded</html>".to_owned())
        }));
        assert!(
            malformed.resolve_campus("示例大学").is_err(),
            "服务端错误页/坏 JSON 不能伪装成未找到"
        );
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
    #[allow(
        clippy::disallowed_macros,
        clippy::print_stderr,
        reason = "显式运行的 ignored live test 需要把耗时与实际节点路径写入验收日志"
    )]
    fn live_shanghai_jiaotong_boundary_fetch() {
        let (overpass, paths) = recording_production_client();
        let fetcher = CampusBoundaryFetcher::with_clients(overpass, NominatimClient::production());
        let started = Instant::now();
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
                eprintln!(
                    "LIVE_BOUNDARY elapsed={:?} path={:?} name={} points={} candidates={}",
                    started.elapsed(),
                    paths.lock().unwrap(),
                    name,
                    gcj02.len(),
                    candidate_count
                );
            }
            other => panic!("真实链路未自动选中：{other:?}"),
        }
    }

    /// 真实采集链路冒烟（不进 CI；T31 交接证据）：
    /// OverpassDataSource（生产 transport：六类四分片 + 有限重试/健康排序）
    /// 对交大闵行边界实拉 → 面候选 > 0、OSM name 优先、WGS→GCJ 已转。
    #[test]
    #[ignore = "真实网络：采集可靠性修复多轮实测证据"]
    #[allow(
        clippy::disallowed_macros,
        clippy::print_stderr,
        reason = "显式运行的 ignored live test 需要把耗时与实际节点路径写入验收日志"
    )]
    fn live_campus_object_collection_via_reliable_overpass_source() {
        use crate::source::{
            DataSource, DeadlineOverpassTransport, OverpassDataSource, SourceGeometry,
        };
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
        let (client, paths) = recording_production_client();
        let started = Instant::now();
        let transport: DeadlineOverpassTransport = Box::new(move |b: &Boundary, deadline, _| {
            let bbox = boundary_bbox(b, 0.01).ok_or_else(|| "无包围盒".to_owned())?;
            client
                .query_campus_objects(bbox, deadline, &|| {})
                .map_err(|message| format!("采集查询失败：{message}"))
        });
        let source = OverpassDataSource::with_deadline_transport(transport);
        let entities = source
            .fetch_raw_entities_until(
                &boundary,
                Instant::now() + Duration::from_secs(220),
                &|_| {},
            )
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
        eprintln!(
            "LIVE_CANDIDATES elapsed={:?} path={:?} total={} polygons={} named={}",
            started.elapsed(),
            paths.lock().unwrap(),
            entities.len(),
            polygons,
            named
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
