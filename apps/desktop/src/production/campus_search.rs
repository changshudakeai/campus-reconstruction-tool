//! 校区在线搜索控制（T30 D-3）
//!
//! 搜索状态机（候选 → 详情 → 显式确认）与响应解析、学校类筛选在 B3
//! `gaode_client`（CampusSearchFlow / parse_place_search_response），网络传输
//! 可注入（生产走校区搜索 WebView，测试注入罐头响应）。S1 只转交"搜索 /
//! 轮询 / 点选 / 确认 / 取消"意图并呈现结果，不持有正式业务数据。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use gaode_client::{parse_place_search_response, CampusSearchFlow, SchoolPoi};

/// 校区在线搜索传输：`(api_key, security_key, query)` → REST 风格响应 JSON。
///
/// 生产实现经校区搜索 WebView（`build_map_page_html`）执行高德 PlaceSearch；
/// 测试注入罐头 JSON，离线可测。S1 只把意图交给传输并呈现结果（ADR-0037）。
pub(crate) type CampusSearchTransport =
    Box<dyn Fn(&str, &str, &str) -> std::result::Result<String, String> + Send + Sync>;

/// 校区搜索查询请求序号（WebView 桥响应与请求配对，防旧响应串台）。
static CAMPUS_SEARCH_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

/// 生产校区搜索传输：等待校区搜索 WebView 就绪 → 求值 `searchCampus(requestId,
/// keyword)`（B3 `build_map_page_html` 已定义）→ 等待匹配信封返回响应 JSON。
///
/// S1 只把原始 IPC 消息转交响应通道（见 `ProductionEntries::handle_map_ipc`），
/// 解析与学校类筛选在 B3 `parse_place_search_response`。
pub(crate) fn campus_search_production_transport() -> (mpsc::Sender<String>, CampusSearchTransport)
{
    let (response_tx, response_rx) = mpsc::channel::<String>();
    // Receiver 非 Sync：桥闭包要求 Send + Sync，用互斥包一层。
    let response_rx = Arc::new(std::sync::Mutex::new(response_rx));
    let transport = Box::new(
        move |_api_key: &str,
              _security_key: &str,
              query: &str|
              -> std::result::Result<String, String> {
            let request_id = CAMPUS_SEARCH_REQUEST_ID.fetch_add(1, Ordering::SeqCst) + 1;
            // 等待 WebView 就绪（UI 线程已在 present(SearchCampus) 里发起 show）。
            let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                if std::time::Instant::now() >= ready_deadline {
                    return Err("校区搜索地图页加载超时".to_owned());
                }
                let (ready_tx, ready_rx) = mpsc::channel();
                let dispatched = slint::invoke_from_event_loop(move || {
                    let ready = crate::map_webview::campus_search_ready();
                    let _ = ready_tx.send(ready);
                });
                if dispatched.is_ok()
                    && ready_rx
                        .recv_timeout(std::time::Duration::from_millis(150))
                        .unwrap_or(false)
                {
                    break;
                }
                // 未就绪：用通道超时阻塞作为节奏（禁 thread::sleep，避免冻结 UI）
                let (_, tick_rx) = mpsc::channel::<()>();
                let _ = tick_rx.recv_timeout(std::time::Duration::from_millis(100));
            }
            let script = campus_search_request_script(request_id, query);
            let _ = slint::invoke_from_event_loop(move || {
                crate::map_webview::evaluate_script(&script);
            });
            // 等待匹配的校区搜索响应（25 秒超时；测试直接注入罐头响应）。
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err("校区搜索响应超时".to_owned());
                }
                let message = response_rx
                    .lock()
                    .expect("campus search response lock")
                    .recv_timeout(remaining)
                    .map_err(|_| "校区搜索响应超时".to_owned())?;
                let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&message) else {
                    continue;
                };
                if envelope.get("type").and_then(serde_json::Value::as_str)
                    != Some("campus_search_response")
                {
                    continue;
                }
                if envelope
                    .get("request_id")
                    .and_then(serde_json::Value::as_u64)
                    != Some(request_id)
                {
                    continue;
                }
                return envelope
                    .get("payload")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| "校区搜索响应缺少 payload".to_owned());
            }
        },
    );
    (response_tx, transport)
}

/// 校区搜索求值脚本：调用 B3 页面已定义的 `searchCampus(requestId, keyword)`。
/// keyword 经 JSON 序列化转义，避免引号/反斜杠注入。
fn campus_search_request_script(request_id: u64, keyword: &str) -> String {
    let keyword_json = serde_json::to_string(keyword).unwrap_or_else(|_| "\"\"".to_owned());
    format!(
        "(function(){{if (typeof searchCampus !== 'function') return;searchCampus({request_id},{keyword});}})();",
        request_id = request_id,
        keyword = keyword_json
    )
}

/// 一次后台搜索的结果：Ok(REST 风格响应 JSON) / Err(可显示原因)
pub(crate) type SearchOutcome = std::result::Result<String, String>;

/// 校区在线搜索控制器：驱动 B3 状态机 + 后台传输。
pub(crate) struct CampusSearchController {
    transport: Arc<CampusSearchTransport>,
    flow: CampusSearchFlow,
    /// 当前关键词（状态机离开 Searching 后仍需用于页面回显）
    query: String,
    /// 后台搜索的结果通道（None = 无进行中的搜索）
    pending: Option<mpsc::Receiver<SearchOutcome>>,
    /// 最近一次候选列表（点选时按 poi_id 反查完整 POI）
    candidates: Vec<SchoolPoi>,
    /// 已进入详情确认的候选
    selected: Option<SchoolPoi>,
}

impl CampusSearchController {
    pub(crate) fn new(transport: Arc<CampusSearchTransport>) -> Self {
        Self {
            transport,
            flow: CampusSearchFlow::new(),
            query: String::new(),
            pending: None,
            candidates: Vec::new(),
            selected: None,
        }
    }

    /// 当前关键词（页面回显与失败重试用）
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    /// 发起搜索：B3 状态机进入 Searching，网络在后台线程执行。
    pub(crate) fn start_search(
        &mut self,
        query: &str,
        api_key: &str,
        security_key: &str,
    ) -> Result<(), String> {
        self.flow.start_search(query).map_err(|e| e.to_string())?;
        self.query = query.trim().to_owned();
        self.candidates.clear();
        self.selected = None;
        let transport = Arc::clone(&self.transport);
        let query = self.query.clone();
        let api_key = api_key.to_owned();
        let security_key = security_key.to_owned();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = transport(&api_key, &security_key, &query);
            let _ = tx.send(outcome);
        });
        self.pending = Some(rx);
        Ok(())
    }

    /// 轮询后台搜索；返回 Some 表示已到终态（成功或失败）
    pub(crate) fn poll(&mut self) -> Option<SearchOutcome> {
        let rx = self.pending.as_ref()?;
        match rx.try_recv() {
            Ok(outcome) => {
                self.pending = None;
                Some(outcome)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err("校区搜索任务意外终止".to_owned()))
            }
        }
    }

    /// 结果到达：B3 解析（学校类筛选/同名去重）并进入候选状态。
    pub(crate) fn receive(&mut self, payload: &str) -> Result<(), String> {
        let pois = parse_place_search_response(payload).map_err(|e| e.to_string())?;
        self.flow.receive_results(pois).map_err(|e| e.to_string())?;
        self.candidates = self.flow.candidates().to_vec();
        Ok(())
    }

    /// 当前候选列表（只读）
    pub(crate) fn candidates(&self) -> &[SchoolPoi] {
        &self.candidates
    }

    /// 点选候选：进入详情确认（T05 显式确认，不自动建立校区）。
    pub(crate) fn view_candidate(&mut self, poi_id: &str) -> Result<&SchoolPoi, String> {
        let index = self
            .candidates
            .iter()
            .position(|poi| poi.poi_id == poi_id)
            .ok_or_else(|| "候选不存在或已过期，请重新搜索".to_owned())?;
        let poi = self.flow.view_detail(index).map_err(|e| e.to_string())?;
        self.selected = Some(poi.clone());
        Ok(poi)
    }

    /// 确认添加：B3 状态机唯一出口（详情页显式确认），返回被确认的 POI。
    pub(crate) fn confirm_selected(&mut self, poi_id: &str) -> Result<SchoolPoi, String> {
        let poi = self
            .selected
            .clone()
            .filter(|poi| poi.poi_id == poi_id)
            .ok_or_else(|| "待确认校区不存在或已过期".to_owned())?;
        self.flow.confirm().map_err(|e| e.to_string())?;
        Ok(poi)
    }

    /// 取消确认：返回候选列表（重选）。
    pub(crate) fn cancel_selection(&mut self) -> Result<(), String> {
        self.selected = None;
        self.flow.back_to_candidates().map_err(|e| e.to_string())
    }
}

use crate::presentation::{
    CampusPlanPageState, ConfirmationPresentation, NavigationDecision, Presentation, Progress,
    Screen,
};
use crate::production::campus_plan_trash::{
    campus_error_fact, campus_info_fact, campus_page_fallback, campus_select_page, plan_error_fact,
    plan_list_page, CampusPlanProductionAdapter,
};
use crate::CampusData;

impl CampusPlanProductionAdapter {
    /// 高德在线校区搜索（D-3）：读取已保存密钥 → 显示校区搜索 WebView →
    /// 后台传输查询 → 立即呈现"搜索中"；失败路径由轮询/重试/取消接管。
    pub(crate) fn present_search(&mut self, query: &str) -> Presentation<CampusPlanPageState> {
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        if query.trim().is_empty() {
            // 空关键词不搜索（ADR-0008：点击搜索或回车才触发；空输入无意义）
            let mut page = campus_select_page(&injector, &self.workspace)
                .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
            page.campus_search_query = query.to_owned();
            return Presentation::ready(page);
        }
        let keys = match (
            injector.settings().gaode_api_key(),
            injector.settings().gaode_security_key(),
        ) {
            (Ok(Some(api_key)), Ok(Some(security_key))) => (api_key, security_key),
            _ => {
                let page = campus_select_page(&injector, &self.workspace)
                    .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
                return Presentation::failed(page)
                    .with_notification(campus_error_fact(l10n, &l10n.t("campus.missing_keys")));
            }
        };
        drop(injector);
        crate::map_webview::show_campus_search(
            self.workspace.window.clone(),
            keys.0.clone(),
            keys.1.clone(),
        );
        match self.search.start_search(query, &keys.0, &keys.1) {
            Ok(()) => {
                let injector = self.injector.borrow();
                let l10n = injector.l10n();
                let mut page = campus_select_page(&injector, &self.workspace)
                    .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
                page.campus_search_query = self.search.query().to_owned();
                page.campus_show_results = true;
                page.campus_search_status = l10n.t("campus.searching");
                Presentation::processing(page, Progress::ZERO)
            }
            Err(message) => {
                let injector = self.injector.borrow();
                let l10n = injector.l10n();
                let mut page = campus_select_page(&injector, &self.workspace)
                    .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
                page.campus_search_query = self.search.query().to_owned();
                Presentation::failed(page).with_notification(campus_error_fact(l10n, &message))
            }
        }
    }

    /// 轮询后台搜索终态（D-3）：成功 → 候选列表；失败 → 重试/取消弹窗；
    /// 仍进行中 → 保持"搜索中"页面。
    pub(crate) fn present_poll_search(&mut self) -> Presentation<CampusPlanPageState> {
        let outcome = self.search.poll();
        let Some(outcome) = outcome else {
            let injector = self.injector.borrow();
            let l10n = injector.l10n();
            let mut page = campus_select_page(&injector, &self.workspace)
                .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
            page.campus_search_query = self.search.query().to_owned();
            page.campus_show_results = true;
            page.campus_search_status = l10n.t("campus.searching");
            return Presentation::processing(page, Progress::ZERO);
        };
        let payload = match outcome {
            Ok(payload) => payload,
            Err(message) => return self.search_failure(&message),
        };
        if let Err(message) = self.search.receive(&payload) {
            return self.search_failure(&message);
        }
        crate::map_webview::hide();
        let injector = self.injector.borrow();
        let mut page = campus_select_page(&injector, &self.workspace)
            .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
        page.campus_search_query = self.search.query().to_owned();
        page.campus_show_results = true;
        page.campus_search_status = String::new();
        page.campus_search_results = search_results_data(&self.search);
        Presentation::ready(page)
    }

    /// 点选搜索候选：进入详情确认（T05 显式确认，不自动建立校区）。
    pub(crate) fn present_select_search_candidate(
        &mut self,
        poi_id: &str,
    ) -> Presentation<CampusPlanPageState> {
        let poi = match self.search.view_candidate(poi_id) {
            Ok(poi) => poi.clone(),
            Err(message) => {
                let injector = self.injector.borrow();
                let l10n = injector.l10n();
                let page = campus_select_page(&injector, &self.workspace)
                    .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
                return Presentation::failed(page)
                    .with_notification(campus_error_fact(l10n, &message));
            }
        };
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        let mut page = campus_select_page(&injector, &self.workspace)
            .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
        page.campus_search_query = self.search.query().to_owned();
        page.campus_show_results = true;
        page.campus_search_results = search_results_data(&self.search);
        Presentation::needs_confirmation(
            page,
            ConfirmationPresentation::new(
                l10n.t("campus.confirm_add_title"),
                l10n.t_with_array("campus.confirm_add_body", &[&poi.name]),
                l10n.t("dialog.confirm_button"),
                l10n.t("dialog.cancel_button"),
            ),
        )
    }

    /// 详情确认窗点"确认"：F1 建/选校区（重复点选只切换），直接进入方案列表。
    pub(crate) fn present_confirm_select_campus(
        &mut self,
        poi_id: &str,
    ) -> Presentation<CampusPlanPageState> {
        let poi = match self.search.confirm_selected(poi_id) {
            Ok(poi) => poi,
            Err(message) => {
                let injector = self.injector.borrow();
                let l10n = injector.l10n();
                let page = campus_select_page(&injector, &self.workspace)
                    .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
                return Presentation::failed(page)
                    .with_notification(campus_error_fact(l10n, &message));
            }
        };
        let selected = {
            let mut injector = self.injector.borrow_mut();
            injector
                .settings_mut()
                .select_campus_by_poi_id(
                    &poi.name,
                    &poi.poi_id,
                    &poi.address,
                    poi.longitude,
                    poi.latitude,
                )
                .map_err(|error| error.to_string())
        };
        crate::map_webview::hide();
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        let page = plan_list_page(&injector, &self.workspace);
        match (selected, page) {
            (Ok(selection), Ok(page)) => {
                let mut result = Presentation::succeeded(page)
                    .with_navigation(NavigationDecision::Show(Screen::PlanList));
                if selection.already_added {
                    result = result.with_notification(campus_info_fact(
                        l10n,
                        "campus.already_added_title",
                        "campus.already_added",
                    ));
                }
                result
            }
            (Err(message), _) => {
                Presentation::failed(campus_page_fallback(&injector, &self.workspace))
                    .with_notification(campus_error_fact(l10n, &message))
            }
            (_, Err(error)) => {
                Presentation::failed(campus_page_fallback(&injector, &self.workspace))
                    .with_notification(plan_error_fact(l10n, &error.to_string()))
            }
        }
    }

    /// 详情确认窗点"取消"：返回候选列表重选，不创建校区。
    pub(crate) fn present_cancel_select_campus(&mut self) -> Presentation<CampusPlanPageState> {
        if let Err(message) = self.search.cancel_selection() {
            let injector = self.injector.borrow();
            let l10n = injector.l10n();
            let page = campus_select_page(&injector, &self.workspace)
                .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
            return Presentation::failed(page).with_notification(campus_error_fact(l10n, &message));
        }
        let injector = self.injector.borrow();
        let mut page = campus_select_page(&injector, &self.workspace)
            .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
        page.campus_search_query = self.search.query().to_owned();
        page.campus_show_results = true;
        page.campus_search_results = search_results_data(&self.search);
        Presentation::ready(page)
    }

    /// 搜索失败：停留搜索页 + 弹窗"重试/取消"（ADR-0008 第 9 条）。
    /// 取消不创建校区、不能绕过校区选择进入方案。
    fn search_failure(&mut self, message: &str) -> Presentation<CampusPlanPageState> {
        log::warn!("校区搜索失败: {message}");
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        let mut page = campus_select_page(&injector, &self.workspace)
            .unwrap_or_else(|_| campus_page_fallback(&injector, &self.workspace));
        page.campus_search_query = self.search.query().to_owned();
        page.campus_show_results = true;
        page.campus_search_status = String::new();
        Presentation::needs_confirmation(
            page,
            ConfirmationPresentation::new(
                l10n.t("campus.search_failed_title"),
                l10n.t_with_array("campus.search_failed_body", &[message]),
                l10n.t("campus.retry_button"),
                l10n.t("dialog.cancel_button"),
            ),
        )
    }
}

/// 搜索候选 → 校区行数据（id = 高德 POI 标识；点选时按 id 反查完整 POI）
fn search_results_data(search: &CampusSearchController) -> Vec<CampusData> {
    search
        .candidates()
        .iter()
        .map(|poi| CampusData {
            id: poi.poi_id.clone().into(),
            name: poi.name.clone().into(),
            address: poi.address.clone().into(),
        })
        .collect()
}
