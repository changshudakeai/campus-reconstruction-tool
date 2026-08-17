//! WebView 适配器的代际、延迟销毁、超时与测试探针契约。

use super::*;

fn reset_state() {
    REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.set(false));
    REVIEW_PUSH_COUNT.with(|s| s.set(0));
    REVIEW_PUSHED_SCRIPTS.with(|s| s.borrow_mut().clear());
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.ipc_handler = None;
        state.status_handler = None;
        state.webview = None;
        state.retiring.clear();
        state.hide_scheduled = false;
        state.generation = 0;
        state.creation_in_flight = false;
        state.pending_show = None;
        state.load_timer = None;
        state.review_map_text_visible = false;
    });
}

#[test]
fn hide_without_webview_keeps_state_clean_in_headless_environment() {
    reset_state();
    hide();
    STATE.with(|s| {
        let state = s.borrow();
        assert!(!state.hide_scheduled);
        assert!(state.retiring.is_empty());
        assert!(!state.campus_search_mode);
        assert_eq!(state.last_slot_scale, None);
    });
    assert!(!is_visible());
}

#[test]
fn orientation_page_creation_success_has_activation_channel() {
    assert_eq!(
        creation_activation_script(MapPageKind::Orientation),
        Some("activateOrientationWhenReady();")
    );
    assert_eq!(creation_activation_script(MapPageKind::Boundary), None);
    assert_eq!(creation_activation_script(MapPageKind::CampusSearch), None);
    assert_eq!(creation_activation_script(MapPageKind::Review), None);
}

#[test]
fn ipc_dispatch_releases_borrow_before_invoking_handler() {
    reset_state();
    register_ipc_handler(Rc::new(|_page, _body| hide()));
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.generation = 7;
        state.last_page_kind = Some(MapPageKind::Boundary);
    });
    dispatch_ipc(
        7,
        MapPageKind::Boundary,
        r#"{"type":"confirm_boundary","coords":[]}"#,
    );
    STATE.with(|s| {
        let state = s.borrow();
        assert!(!state.hide_scheduled);
        assert!(state.retiring.is_empty());
    });
    reset_state();
}

#[test]
fn creation_failure_reports_map_unavailable_immediately() {
    reset_state();
    let received: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&received);
    register_status_handler(Rc::new(move |_page, available| {
        captured.borrow_mut().push(available);
    }));
    let config = gaode_client::BoundaryEditPageConfig::new("bad key!", "xyz789")
        .with_anchor(116.4, 39.9)
        .with_orientation_mode(true);
    show_with_config(Weak::<crate::AppWindow>::default(), config);
    assert_eq!(received.borrow().as_slice(), &[false]);
    STATE.with(|s| {
        let state = s.borrow();
        assert!(state.webview.is_none());
        assert!(!state.creation_in_flight);
        assert!(state.pending_show.is_none());
        assert!(state.retiring.is_empty());
    });
    reset_state();
}

#[test]
fn review_page_skips_rust_load_timeout() {
    reset_state();
    let weak = Weak::<crate::AppWindow>::default();
    assert!(!PendingShow::Review {
        window: weak.clone(),
        api_key: "testapikey123".into(),
        security_key: "testsecurity123".into(),
        anchor_lon: 116.4,
        anchor_lat: 39.9,
        map_text_label: "显示地图文字".into(),
        map_text_visible: false,
        initial_viewport: None,
    }
    .has_load_timeout());
    assert!(PendingShow::Boundary {
        window: weak,
        api_key: "testapikey123".into(),
        security_key: "testsecurity123".into(),
        anchor_lon: 116.4,
        anchor_lat: 39.9,
        initial_viewport: None,
    }
    .has_load_timeout());
    reset_state();
}

#[test]
fn review_push_counter_and_probe_record_diagnostic_scripts() {
    reset_state();
    set_review_push_probe_visible(true);
    note_review_push("window.highlightReviewCandidate(\"candidate\");");
    assert_eq!(review_push_count(), 1);
    assert_eq!(
        review_pushed_scripts(),
        vec!["window.highlightReviewCandidate(\"candidate\");"]
    );
    set_review_push_probe_visible(false);
    reset_review_push_count();
    reset_state();
}
