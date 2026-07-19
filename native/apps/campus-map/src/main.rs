#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("campus-map is currently supported only on Windows");
}

#[cfg(target_os = "windows")]
mod windows {
    use campus_tool_protocol::{
        forward_tool_events, read_message, write_message, MapBoundaryDesk,
        MapFoundationReviewDeskRequest, MapPurpose, ToolCommand, ToolEvent, ToolKind,
        PROTOCOL_VERSION,
    };
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::net::windows::named_pipe::ClientOptions;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};
    use wry::{
        dpi::{LogicalPosition, LogicalSize, PhysicalSize},
        Rect, WebView, WebViewBuilder,
    };

    type PipeThread = thread::JoinHandle<Result<(), String>>;
    type ToolConnection = (
        ToolCommand,
        Sender<ToolEvent>,
        Receiver<ToolCommand>,
        Vec<PipeThread>,
    );

    fn full_window_bounds(width: u32, height: u32, scale_factor: f64) -> Rect {
        let logical = PhysicalSize::new(width, height).to_logical::<f64>(scale_factor);
        Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(logical.width, logical.height).into(),
        }
    }

    pub fn run() -> Result<(), String> {
        let pipe = std::env::args()
            .nth(1)
            .ok_or("missing named pipe argument")?;
        let token = std::env::args().nth(2).ok_or("missing session token")?;
        if std::env::var_os("CAMPUS_MAP_HEADLESS").is_some() {
            return run_headless(pipe, token);
        }
        let (config, event_tx, command_rx, pipe_threads) = connect(pipe, token)?;
        let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
        let mut app = MapApplication {
            window: None,
            webview: None,
            config,
            event_tx,
            command_rx,
            pipe_threads,
        };
        event_loop
            .run_app(&mut app)
            .map_err(|error| error.to_string())
    }

    fn connect(pipe: String, token: String) -> Result<ToolConnection, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let mut client = runtime.block_on(async {
            let mut last_error = None;
            for _ in 0..40 {
                match ClientOptions::new().open(&pipe) {
                    Ok(client) => return Ok(client),
                    Err(error) => {
                        last_error = Some(error);
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
            Err(last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "named pipe unavailable".into()))
        })?;
        runtime.block_on(write_message(
            &mut client,
            &ToolCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
                session_token: token,
                tool: ToolKind::Map,
            },
        ))?;
        let config: ToolCommand = runtime.block_on(read_message(&mut client))?;
        let (event_tx, event_rx) = mpsc::channel::<ToolEvent>();
        let (command_tx, command_rx) = mpsc::channel::<ToolCommand>();
        let (mut reader, writer) = tokio::io::split(client);
        let writer_thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(forward_tool_events(writer, event_rx))
        });
        let reader_thread = thread::spawn(move || {
            runtime.block_on(async move {
                loop {
                    let command: ToolCommand = read_message(&mut reader).await?;
                    let shutdown = matches!(command, ToolCommand::Shutdown);
                    command_tx
                        .send(command)
                        .map_err(|_| "map command channel closed".to_string())?;
                    if shutdown {
                        return Ok(());
                    }
                }
            })
        });
        Ok((
            config,
            event_tx,
            command_rx,
            vec![writer_thread, reader_thread],
        ))
    }

    fn run_headless(pipe: String, token: String) -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async move {
            let mut client = None;
            for _ in 0..40 {
                match ClientOptions::new().open(&pipe) {
                    Ok(opened) => {
                        client = Some(opened);
                        break;
                    }
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                }
            }
            let mut client = client.ok_or("named pipe unavailable")?;
            write_message(
                &mut client,
                &ToolCommand::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    session_token: token,
                    tool: ToolKind::Map,
                },
            )
            .await?;
            let command: ToolCommand = read_message(&mut client).await?;
            if !matches!(
                command,
                ToolCommand::OpenMap { .. }
                    | ToolCommand::OpenBoundaryDesk { .. }
                    | ToolCommand::OpenFoundationReviewDesk { .. }
            ) {
                return Err("invalid map request".into());
            }
            write_message(
                &mut client,
                &ToolEvent::Ready {
                    protocol_version: PROTOCOL_VERSION,
                    tool: ToolKind::Map,
                },
            )
            .await?;
            write_message(
                &mut client,
                &ToolEvent::Closed {
                    tool: ToolKind::Map,
                },
            )
            .await
        })
    }

    struct MapApplication {
        window: Option<Window>,
        webview: Option<WebView>,
        config: ToolCommand,
        event_tx: Sender<ToolEvent>,
        command_rx: Receiver<ToolCommand>,
        pipe_threads: Vec<PipeThread>,
    }

    impl MapApplication {
        fn finish(&mut self, event_loop: &ActiveEventLoop, error: Option<String>) {
            if let Some(message) = error {
                let _ = self.event_tx.send(ToolEvent::Error { message });
            }
            let _ = self.event_tx.send(ToolEvent::Closed {
                tool: ToolKind::Map,
            });
            self.pipe_threads.clear();
            event_loop.exit();
        }
    }

    impl ApplicationHandler for MapApplication {
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(50),
            ));
            loop {
                match self.command_rx.try_recv() {
                    Ok(ToolCommand::UpdateBoundaryDesk { desk }) => {
                        if let Some(webview) = self.webview.as_ref() {
                            match serde_json::to_string(&desk) {
                                Ok(desk) => {
                                    let _ = webview.evaluate_script(&format!(
                                        "window.applyBoundaryDesk({desk})"
                                    ));
                                }
                                Err(error) => {
                                    self.finish(
                                        event_loop,
                                        Some(format!(
                                            "encode Boundary desk update failed: {error}"
                                        )),
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Ok(ToolCommand::UpdateFoundationReviewDesk { desk }) => {
                        if let Some(webview) = self.webview.as_ref() {
                            match serde_json::to_string(&desk) {
                                Ok(desk) => {
                                    let _ = webview.evaluate_script(&format!(
                                        "window.applyFoundationReviewDesk({desk})"
                                    ));
                                }
                                Err(error) => {
                                    self.finish(
                                        event_loop,
                                        Some(format!(
                                            "encode Foundation review desk update failed: {error}"
                                        )),
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Ok(ToolCommand::Shutdown) => {
                        self.finish(event_loop, None);
                        return;
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => {
                        self.finish(
                            event_loop,
                            Some("Boundary desk command channel disconnected".into()),
                        );
                        return;
                    }
                }
            }
        }

        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = match event_loop.create_window(
                Window::default_attributes()
                    .with_title("高德 3D 校区工具")
                    .with_inner_size(winit::dpi::LogicalSize::new(1100, 760)),
            ) {
                Ok(window) => window,
                Err(error) => {
                    self.finish(
                        event_loop,
                        Some(format!("create map window failed: {error}")),
                    );
                    return;
                }
            };
            let html = map_html(&self.config);
            let tx = self.event_tx.clone();
            let initial_size = window.inner_size();
            let webview = match WebViewBuilder::new()
                .with_html(html)
                .with_bounds(full_window_bounds(
                    initial_size.width,
                    initial_size.height,
                    window.scale_factor(),
                ))
                .with_ipc_handler(move |request| {
                    if let Ok(event) = serde_json::from_str::<ToolEvent>(request.body()) {
                        let _ = tx.send(event);
                    }
                })
                .build_as_child(&window)
            {
                Ok(webview) => webview,
                Err(error) => {
                    self.finish(
                        event_loop,
                        Some(format!("create map webview failed: {error}")),
                    );
                    return;
                }
            };
            let _ = self.event_tx.send(ToolEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
                tool: ToolKind::Map,
            });
            self.webview = Some(webview);
            self.window = Some(window);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::Resized(size) => {
                    if let (Some(window), Some(webview)) =
                        (self.window.as_ref(), self.webview.as_ref())
                    {
                        if let Err(error) = webview.set_bounds(full_window_bounds(
                            size.width,
                            size.height,
                            window.scale_factor(),
                        )) {
                            let _ = self.event_tx.send(ToolEvent::Error {
                                message: format!("map webview resize failed: {error}"),
                            });
                        }
                    }
                }
                WindowEvent::CloseRequested => {
                    self.finish(event_loop, None);
                }
                _ => {}
            }
        }
    }

    fn map_html(command: &ToolCommand) -> String {
        if let ToolCommand::OpenFoundationReviewDesk { request } = command {
            return foundation_review_html(request);
        }
        if let ToolCommand::OpenBoundaryDesk { request } = command {
            let open_map = ToolCommand::OpenMap {
                campus_name: request.campus_name.clone(),
                center_lng: request.center_lng,
                center_lat: request.center_lat,
                zoom: request.zoom,
                pitch: request.pitch,
                rotation: request.rotation,
                js_api_key: request.js_api_key.clone(),
                security_code: request.security_code.clone(),
                boundary: Vec::new(),
                purpose: MapPurpose::CampusBoundary,
                overlays: Vec::new(),
                feature_kind: None,
                english: request.english,
            };
            return map_html_request(&open_map, Some(&request.desk));
        }
        map_html_request(command, None)
    }

    fn foundation_review_html(request: &MapFoundationReviewDeskRequest) -> String {
        let desk = serde_json::to_string(&request.desk).unwrap();
        let campus = serde_json::to_string(&request.campus_name).unwrap();
        let key = request
            .js_api_key
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>();
        let security = serde_json::to_string(&request.security_code).unwrap();
        let english = if request.english { "true" } else { "false" };
        let boundary = serde_json::to_string(&request.boundary).unwrap();
        let template = r#"<!doctype html><html><head><meta charset="utf-8">
<style>
:root{--ink:#17251f;--forest:#2f765b;--forest2:#245d48;--paper:#fffdf7;--sand:#f1eadc;--line:#cfc3ae;--red:#a54836;--amber:#d08b32;--blue:#315f78}
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;font:13px/1.4 "Microsoft YaHei UI",sans-serif;color:var(--ink);background:var(--sand)}
button{border:1px solid var(--line);border-radius:8px;padding:8px 11px;background:var(--paper);color:var(--ink);cursor:pointer}button:hover:not(:disabled){border-color:var(--forest)}button:disabled{opacity:.5;cursor:not-allowed}.primary{background:var(--forest);border-color:var(--forest);color:white;font-weight:700}.danger{color:var(--red);border-color:#d5aba0;background:#fff8f5}.small{padding:6px 8px;font-size:11px}
#shell{height:100%;display:grid;grid-template-rows:58px 1fr 66px}.tabs{display:flex;align-items:center;gap:8px;padding:9px 16px;background:var(--paper);border-bottom:1px solid var(--line)}.tab{display:grid;grid-template-columns:1fr auto;gap:2px 8px;min-width:145px;text-align:left}.tab.active{background:var(--forest);color:white;border-color:var(--forest)}.tab small{grid-column:1/-1;opacity:.8}.route-title{font-weight:800;margin-right:8px}.spacer{flex:1}
.workspace{min-height:0;display:grid;grid-template-columns:310px minmax(420px,1fr) 340px;gap:12px;padding:12px}.panel{min-height:0;background:var(--paper);border:1px solid var(--line);border-radius:12px;overflow:auto}.queue{padding:12px}.toolbar{position:sticky;top:-12px;z-index:4;background:var(--paper);padding:0 0 9px;display:flex;align-items:center;gap:6px}.candidate{width:100%;margin:0 0 8px;padding:10px;text-align:left;display:grid;gap:5px}.candidate.selected{border:2px solid var(--forest)}.candidate-head{display:flex;gap:7px;align-items:center}.candidate input{margin:0}.state{font-size:10px;font-weight:800;padding:2px 6px;border-radius:99px;background:#eee4d3}.state.accepted{background:#dcecdf;color:var(--forest2)}.state.rejected{background:#f5ded8;color:var(--red)}.state.deferred{background:#fff0cf;color:#865a13}.state.supporting_evidence{background:#dbe8ef;color:var(--blue)}.muted{color:#637069}.tiny{font-size:10px}
.map-panel{position:relative;overflow:hidden}.map{width:100%;height:100%;min-height:430px}.map-badge{position:absolute;z-index:3;left:12px;top:12px;padding:7px 9px;background:rgba(23,37,31,.9);color:white;border-radius:8px}.legend{position:absolute;z-index:3;left:12px;bottom:12px;padding:7px 9px;background:rgba(255,253,247,.95);border:1px solid var(--line);border-radius:8px;font-size:10px}
.evidence{padding:14px}.evidence h2,.evidence h3{margin:0 0 7px}.card{padding:9px;margin:0 0 8px;background:#f5efe4;border-radius:8px}.assessment{display:grid;grid-template-columns:1fr 1fr;gap:7px}.assessment div{padding:8px;background:#f5efe4;border-radius:7px;font-size:10px}.assessment strong{display:block}.gap{border:1px solid #d7ad7c;background:#fff6df}.gap.ack{border-color:#8eaa8f;background:#eef5eb}.conflict{border:1px dashed var(--red);background:#fff6f2}.actions{display:grid;grid-template-columns:1fr 1fr;gap:7px;margin:10px 0}.actions .wide{grid-column:1/-1}.no-draw{padding:8px;border:1px dashed var(--line);border-radius:8px;color:#637069}
.footer{display:flex;align-items:center;gap:12px;padding:10px 16px;background:var(--paper);border-top:1px solid var(--line)}.progress{height:8px;flex:1;max-width:360px;background:#ded5c7;border-radius:99px;overflow:hidden}.progress span{display:block;height:100%;background:var(--forest)}.blocked{color:var(--red)}
@media(max-width:1000px){.workspace{grid-template-columns:280px 1fr}.evidence{display:none}.tab{min-width:115px}}
</style>
<script>const post=value=>window.ipc.postMessage(JSON.stringify(value));const english=__ENGLISH__;window._AMapSecurityConfig={securityJsCode:__SECURITY__};</script>
<script src="https://webapi.amap.com/maps?v=2.0&key=__KEY__" onerror="post({type:'error',message:'Failed to load Gaode Maps'})"></script>
</head><body><main id="shell"><header class="tabs" id="tabs"></header><section class="workspace"><aside class="panel queue"><div class="toolbar"><strong id="queue-title"></strong><span class="spacer"></span><button class="small" id="batch-accept">Batch accept</button><button class="small danger" id="batch-reject">Batch reject</button></div><div id="candidates"></div></aside><section class="panel map-panel"><div id="map" class="map"></div><div class="map-badge" id="map-badge"></div><div class="legend">Colour = category · line style = ledger state · map is review-only</div></section><aside class="panel evidence" id="evidence"></aside></section><footer class="footer"><div><strong id="footer-title"></strong><div class="tiny muted" id="footer-state"></div></div><div class="progress"><span id="progress"></span></div><button class="primary" id="complete">Complete category</button></footer></main>
<script>
const tx=(zh,en)=>english?en:zh;const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let desk=__DESK__,selected=new Set(),overlays=[],pending=false;
const map=new AMap.Map('map',{viewMode:'3D',zoom:__ZOOM__,pitch:__PITCH__,rotation:__ROTATION__,center:[__LNG__,__LAT__],showLabel:false});
const boundary=__BOUNDARY__.map(p=>[p.lng,p.lat]);if(boundary.length>=3){const outline=new AMap.Polygon({path:boundary,strokeColor:'#17251f',strokeWeight:3,fillOpacity:0,zIndex:2});map.add(outline)}
const activeTab=()=>desk.categories.find(c=>c.id===desk.activeCategory);const activeCandidate=()=>desk.candidates.find(c=>c.id===desk.selectedCandidateId)||desk.candidates[0];
function draw(){if(overlays.length)map.remove(overlays);overlays=[];desk.candidates.forEach((candidate,index)=>{const color=candidate.disposition==='accepted'?'#2f765b':candidate.disposition==='rejected'?'#a54836':candidate.disposition==='deferred'?'#d08b32':'#315f78';candidate.geometry.forEach(path=>{if(!path.length)return;const points=path.map(p=>[p.lng,p.lat]);let item;if(candidate.geometryForm==='point')item=new AMap.Marker({position:points[0],title:candidate.label,zIndex:candidate.id===desk.selectedCandidateId?120:20});else if(candidate.geometryForm==='centreline')item=new AMap.Polyline({path:points,strokeColor:color,strokeWeight:candidate.id===desk.selectedCandidateId?8:5,strokeStyle:candidate.disposition==='rejected'?'dashed':'solid',zIndex:candidate.id===desk.selectedCandidateId?120:20});else item=new AMap.Polygon({path:points,strokeColor:color,strokeWeight:candidate.id===desk.selectedCandidateId?7:4,strokeStyle:candidate.disposition==='rejected'?'dashed':'solid',fillColor:color,fillOpacity:candidate.disposition==='rejected'?.06:.18,zIndex:candidate.id===desk.selectedCandidateId?120:20});item.on('click',()=>selectCandidate(candidate.id));overlays.push(item)})});if(overlays.length)map.add(overlays)}
function selectCandidate(id){desk.selectedCandidateId=id;post({type:'mapFoundationReviewCandidateSelected',category:desk.activeCategory,subjectId:id});render()}
function decision(value){const item=activeCandidate();if(!item)return;pending=true;post({type:'mapFoundationReviewDecisionRequested',category:desk.activeCategory,subjectId:item.id,decision:value});render()}
function renderTabs(){document.getElementById('tabs').innerHTML=`<span class="route-title">${tx('Foundation 五类审核','Foundation five-category review')}</span>`+desk.categories.map(c=>`<button class="tab ${c.id===desk.activeCategory?'active':''}" data-category="${c.id}"><strong>${esc(c.label)}</strong><span>${c.complete?'✓':c.pending}</span><small>${esc(c.acquisitionState)} · ${c.disposed}/${c.total}</small></button>`).join('');document.querySelectorAll('[data-category]').forEach(button=>button.onclick=()=>{pending=true;selected.clear();post({type:'mapFoundationReviewCategorySelected',category:button.dataset.category})})}
function renderQueue(){const tab=activeTab();document.getElementById('queue-title').textContent=tx('候选队列','Candidate queue')+' · '+(tab?.total||0);document.getElementById('candidates').innerHTML=desk.candidates.map(c=>`<button class="candidate ${c.id===desk.selectedCandidateId?'selected':''}" data-id="${esc(c.id)}"><div class="candidate-head"><input type="checkbox" data-check="${esc(c.id)}" ${selected.has(c.id)?'checked':''}><span class="state ${esc(c.disposition)}">${esc(c.disposition)}</span><span class="spacer"></span><span class="tiny">${esc(c.priority)}</span></div><strong>${esc(c.label)}</strong><span class="tiny muted">${esc(c.sourceSummary)}</span></button>`).join('');document.querySelectorAll('.candidate[data-id]').forEach(button=>button.onclick=e=>{if(e.target.matches('input'))return;selectCandidate(button.dataset.id)});document.querySelectorAll('[data-check]').forEach(input=>input.onchange=()=>{input.checked?selected.add(input.dataset.check):selected.delete(input.dataset.check);renderQueue()});document.getElementById('batch-accept').disabled=!selected.size||pending;document.getElementById('batch-reject').disabled=!selected.size||pending}
function renderEvidence(){const c=activeCandidate();const providers=desk.providerOutcomes.map(p=>`<div class="card"><strong>${esc(p.provider)} · ${esc(p.state)}</strong><div class="tiny">${esc(p.tileId)} · ${esc(p.summary)}</div></div>`).join('');const gaps=desk.knownGaps.map(g=>`<div class="card gap ${g.acknowledged?'ack':''}"><strong>${esc(g.attemptedEvidence)}</strong><p class="tiny">${esc(g.generationImpact)}</p><button class="small" data-gap="${esc(g.id)}" data-ack="${!g.acknowledged}">${g.acknowledged?tx('重新打开缺口','Reopen gap'):tx('确认缺口','Acknowledge gap')}</button></div>`).join('');const conflicts=desk.conflicts.map(x=>`<div class="card conflict"><strong>${esc(x.kind)} ${x.resolved?'✓':''}</strong><p class="tiny">${esc(x.explanation)}</p>${x.resolved?'':`<button class="small" data-separate="${esc(x.id)}">${tx('保留为独立地物','Keep separate')}</button>${x.kind==='Containment'?`<button class="small" data-contain="${esc(x.id)}">${tx('记录容器关系','Record containment')}</button>`:''}${x.kind==='GeometryOverlap'?`<button class="small" data-group="${esc(x.id)}">${tx('主证据 + 支持证据','Primary + supporting')}</button><button class="small" data-repair="${esc(x.id)}">${tx('记录几何修复','Record geometry repair')}</button>`:''}${x.kind==='Naming'?`<button class="small" data-name="${esc(x.id)}">${tx('记录名称','Record name')}</button>`:''}${x.kind==='Attribute'?`<button class="small" data-attribute="${esc(x.id)}">${tx('记录属性','Record attribute')}</button>`:''}`}</div>`).join('');document.getElementById('evidence').innerHTML=c?`<h2>${esc(c.label)}</h2><div class="tiny muted">${esc(c.lineageSummary)}</div><div class="card tiny"><strong>${tx('审核几何','Review geometry')}</strong><br>${esc(c.geometryForm)}${c.subtype?' · '+esc(c.subtype):''}${c.widthSummary?' · '+esc(c.widthSummary):''}</div><div class="actions"><button class="primary" id="accept">${tx('接受','Accept')}</button><button class="danger" id="reject">${tx('拒绝','Reject')}</button><button id="defer">${tx('延后','Defer')}</button><button id="revoke">${tx('撤销决定','Revoke')}</button></div><h3>${tx('多维证据评估','Evidence assessment')}</h3><div class="assessment"><div><strong>${tx('几何','Geometry')}</strong>${esc(c.assessment.geometry)}</div><div><strong>${tx('分类','Semantics')}</strong>${esc(c.assessment.semantics)}</div><div><strong>${tx('实体','Entity')}</strong>${esc(c.assessment.entityMatch)}</div><div><strong>${tx('名称','Name')}</strong>${esc(c.assessment.nameMatch)}</div></div><p class="tiny"><strong>${tx('来源与许可','Provenance')}</strong><br>${esc(c.provenanceSummary)}</p><h3>${tx('来源状态','Provider state')}</h3>${providers}<h3>${tx('已知地物缺口','Known Feature Gaps')}</h3>${gaps||`<div class="card">${tx('无已知缺口','No known gaps')}</div>`}<h3>${tx('冲突','Conflicts')}</h3>${conflicts||`<div class="card">${tx('无未决冲突','No unresolved conflicts')}</div>`}<div class="no-draw">${tx('本工作流只审核来源证据；不提供五类空白绘制或截图恢复。','This workflow reviews source evidence only; no blank-canvas drawing or screenshot recovery is available.')}</div>`:`<h2>${tx('本类没有候选','No candidates in this category')}</h2>${providers}${gaps}`;document.getElementById('accept')?.addEventListener('click',()=>decision('accept'));document.getElementById('reject')?.addEventListener('click',()=>decision('reject'));document.getElementById('defer')?.addEventListener('click',()=>{const gap=desk.knownGaps.find(g=>g.acknowledged),reason=gap&&prompt(tx('输入结构化延后原因','Enter a structured deferral reason'));if(c&&gap&&reason){pending=true;post({type:'mapFoundationReviewDeferredRequested',category:desk.activeCategory,subjectId:c.id,structuredReason:reason,acknowledgedGapId:gap.id});render()}else if(!gap)alert(tx('请先确认一个已知地物缺口。','Acknowledge a Known Feature Gap first.'))});document.getElementById('revoke')?.addEventListener('click',()=>decision('revoke'));document.querySelectorAll('[data-gap]').forEach(b=>b.onclick=()=>post({type:'mapKnownFeatureGapAcknowledgementRequested',category:desk.activeCategory,gapId:b.dataset.gap,acknowledged:b.dataset.ack==='true'}));document.querySelectorAll('[data-separate]').forEach(b=>b.onclick=()=>post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:b.dataset.separate,resolution:{resolution:'keep_separate'}}));document.querySelectorAll('[data-contain]').forEach(b=>b.onclick=()=>{const x=desk.conflicts.find(v=>v.id===b.dataset.contain);post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:x.id,resolution:{resolution:'containment',containerId:x.subjectIds[0],memberId:x.subjectIds[1],containerGeneratesSurface:false}})});document.querySelectorAll('[data-group]').forEach(b=>b.onclick=()=>{const x=desk.conflicts.find(v=>v.id===b.dataset.group);post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:x.id,resolution:{resolution:'grouping',groupId:'group:'+x.id,primarySubjectId:x.subjectIds[0],supportingSubjectIds:x.subjectIds.slice(1)}})});document.querySelectorAll('[data-name]').forEach(b=>b.onclick=()=>{const x=desk.conflicts.find(v=>v.id===b.dataset.name),displayName=prompt(tx('输入证据支持的名称','Enter the evidence-backed name')),evidence=prompt(tx('输入证据 ID（逗号分隔）','Enter evidence IDs (comma-separated)'));if(displayName&&evidence)post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:x.id,resolution:{resolution:'naming',subjectId:x.subjectIds[0],displayName,evidenceIds:evidence.split(',').map(v=>v.trim()).filter(Boolean)}})});document.querySelectorAll('[data-repair]').forEach(b=>b.onclick=()=>{const x=desk.conflicts.find(v=>v.id===b.dataset.repair),digest=prompt(tx('输入审核几何 SHA-256','Enter review geometry SHA-256'));if(digest)post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:x.id,resolution:{resolution:'geometry_repair',subjectId:x.subjectIds[0],reviewGeometrySha256:digest}})});document.querySelectorAll('[data-attribute]').forEach(b=>b.onclick=()=>{const x=desk.conflicts.find(v=>v.id===b.dataset.attribute),attribute=prompt(tx('输入属性判定','Enter the attribute decision')),provenance=prompt(tx('输入来源 ID（逗号分隔）','Enter provenance IDs (comma-separated)'));if(attribute&&provenance)post({type:'mapFoundationConflictResolutionRequested',category:desk.activeCategory,conflictId:x.id,resolution:{resolution:'attribute',subjectId:x.subjectIds[0],attribute,provenanceIds:provenance.split(',').map(v=>v.trim()).filter(Boolean)}})})}
function renderFooter(){const tab=activeTab(),pct=tab&&tab.total?Math.round(tab.disposed/tab.total*100):(desk.completionBlockedReason?0:100);document.getElementById('footer-title').textContent=(tab?.label||desk.activeCategory)+' · '+(tab?.disposed||0)+'/'+(tab?.total||0);const state=document.getElementById('footer-state');state.textContent=desk.completionBlockedReason||tx('所有候选已有处置，可明确完成本类审核。','Every candidate has a disposition; this category can be explicitly completed.');state.className='tiny '+(desk.completionBlockedReason?'blocked':'muted');document.getElementById('progress').style.width=pct+'%';const complete=document.getElementById('complete');complete.disabled=!!desk.completionBlockedReason||!!tab?.complete||pending;complete.textContent=tab?.complete?tx('✓ 已完成','✓ Complete'):tx('完成本类审核','Complete category')}
function render(){renderTabs();renderQueue();renderEvidence();renderFooter();document.getElementById('map-badge').textContent=(activeTab()?.label||desk.activeCategory)+' · '+tx('地图仅供空间上下文','spatial context only');draw()}
function batch(decision){if(!selected.size)return;pending=true;post({type:'mapFoundationBatchReviewRequested',category:desk.activeCategory,exactSubjectIds:[...selected],basisToken:desk.basisToken,expectedLedgerSequence:desk.ledgerSequence,decision});render()}
document.getElementById('batch-accept').onclick=()=>batch('accept');document.getElementById('batch-reject').onclick=()=>batch('reject');document.getElementById('complete').onclick=()=>{pending=true;post({type:'mapFoundationCategoryCompletionRequested',category:desk.activeCategory});render()};
window.applyFoundationReviewDesk=next=>{desk=next;selected.clear();pending=false;render()};render();if(boundary.length>=3)map.setFitView(null,false,[80,360,90,330]);
</script></body></html>"#;
        template
            .replace("__ENGLISH__", english)
            .replace("__SECURITY__", &security)
            .replace("__KEY__", &key)
            .replace("__DESK__", &desk)
            .replace("__ZOOM__", &request.zoom.to_string())
            .replace("__PITCH__", &request.pitch.to_string())
            .replace("__ROTATION__", &request.rotation.to_string())
            .replace("__LNG__", &request.center_lng.to_string())
            .replace("__LAT__", &request.center_lat.to_string())
            .replace("__BOUNDARY__", &boundary)
            .replace("__CAMPUS__", &campus)
    }

    fn map_html_request(command: &ToolCommand, boundary_desk: Option<&MapBoundaryDesk>) -> String {
        let ToolCommand::OpenMap {
            campus_name,
            center_lng,
            center_lat,
            zoom,
            pitch,
            rotation,
            js_api_key,
            security_code,
            boundary,
            purpose,
            overlays,
            feature_kind: _,
            english,
        } = command
        else {
            return "<h1>Invalid map request</h1>".into();
        };
        let campus = serde_json::to_string(campus_name).unwrap();
        let key = js_api_key
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>();
        let security = serde_json::to_string(security_code).unwrap();
        let boundary = serde_json::to_string(boundary).unwrap();
        let overlays = serde_json::to_string(overlays).unwrap();
        let boundary_desk = serde_json::to_string(&boundary_desk).unwrap();
        let initial_points = boundary.clone();
        let (bar, editing_script) = match purpose {
            MapPurpose::CampusSelection => (
                if *english {
                    r#"<span class="task">1 · Select campus</span><input id="poi-query" aria-label="Campus name" placeholder="Search school or campus"><button id="poi-search">Search Gaode</button><select id="poi-results" aria-label="Search results" hidden></select><button id="poi-confirm" hidden>Use this campus</button>"#.to_string()
                } else {
                    r#"<span class="task">1 · 选择校区</span><input id="poi-query" aria-label="校园名称" placeholder="搜索学校或校区"><button id="poi-search">搜索高德</button><select id="poi-results" aria-label="搜索结果" hidden></select><button id="poi-confirm" hidden>使用此校区</button>"#.to_string()
                },
                r#"
const english=__ENGLISH__;
let poiSearch=null,poiCandidates=[];
AMap.plugin('AMap.PlaceSearch',()=>{poiSearch=new AMap.PlaceSearch({pageSize:12,pageIndex:1,extensions:'base'});});
document.getElementById('poi-query').value=__CAMPUS__;
document.getElementById('poi-search').onclick=()=>{
  const query=document.getElementById('poi-query').value.trim();
  if(!query||!poiSearch){post({type:'error',message:english?'Enter a campus name and wait for Gaode search to become ready':'请输入校园名称并等待高德搜索服务就绪'});return;}
  poiSearch.search(query,(status,result)=>{
    const pois=result&&result.poiList&&Array.isArray(result.poiList.pois)?result.poiList.pois:(result&&Array.isArray(result.pois)?result.pois:(result&&result.data&&Array.isArray(result.data.pois)?result.data.pois:[]));
    poiCandidates=status==='complete'?pois.filter(p=>p&&p.location):[];
    const select=document.getElementById('poi-results');select.replaceChildren();
    poiCandidates.forEach((poi,index)=>{const option=document.createElement('option');option.value=String(index);option.textContent=[poi.name,poi.address].filter(Boolean).join(' · ');select.appendChild(option);});
    select.hidden=poiCandidates.length===0;document.getElementById('poi-confirm').hidden=poiCandidates.length===0;
    if(poiCandidates.length){const p=poiCandidates[0];map.setZoomAndCenter(17,[p.location.lng,p.location.lat]);}
    else {const code=typeof result==='string'?result:String((result&&(result.info||result.message||result.code))||status||'UNKNOWN');post({type:'mapSearchFailed',code});}
  });
};
document.getElementById('poi-results').onchange=e=>{const p=poiCandidates[Number(e.target.value)];if(p)map.setZoomAndCenter(17,[p.location.lng,p.location.lat]);};
document.getElementById('poi-confirm').onclick=()=>{const index=Number(document.getElementById('poi-results').value),p=poiCandidates[index];if(!p)return;post({type:'mapCampusSelected',poiId:String(p.id||''),name:String(p.name||''),lng:p.location.lng,lat:p.location.lat});document.getElementById('bar').innerHTML='<span class="task">'+(english?'Campus selected · return to the project':'校区已选定 · 请返回项目')+'</span>';};"#
                    .replace("__ENGLISH__", if *english { "true" } else { "false" })
                    .replace("__CAMPUS__", &campus),
            ),
            MapPurpose::CampusBoundary => (
                if *english {
                    r#"<div class="boundary-shell"><aside class="boundary-left"><span class="task">2 · Automatic Campus Boundary</span><h2>Ranked candidates</h2><p class="hint">Invalid candidates remain diagnosable but cannot be edited.</p><div id="boundary-candidates"></div><div id="boundary-recovery"></div></aside><section class="boundary-tools"><strong id="boundary-mode-label">Review mode</strong><button id="boundary-adjust" class="secondary">Adjust boundary</button><button id="boundary-insert" class="secondary" disabled>Insert on selected edge</button><button id="boundary-delete" class="secondary" disabled>Delete selected vertex</button><button id="boundary-undo" class="secondary" disabled>Undo</button><button id="boundary-restore" class="secondary" disabled>Restore candidate original</button><span id="boundary-validity" class="hint"></span></section><aside class="boundary-right"><h2>Lineage &amp; coverage</h2><div id="boundary-evidence"></div></aside><footer class="boundary-confirmation"><div><strong>Next: acquire Buildings, Circulation, Water, Vegetation, and Sports</strong><br><span class="hint">Save the edited boundary and discovery snapshot, then reuse this Dataset Bundle.</span></div><button id="boundary-back" class="secondary">Return to Campus Target</button><button id="boundary-confirm" disabled>Confirm boundary and begin five-category acquisition</button></footer></div>"#.to_string()
                } else {
                    r#"<div class="boundary-shell"><aside class="boundary-left"><span class="task">2 · 自动 Campus Boundary</span><h2>排序候选</h2><p class="hint">无效候选保留诊断，但不可编辑或确认。</p><div id="boundary-candidates"></div><div id="boundary-recovery"></div></aside><section class="boundary-tools"><strong id="boundary-mode-label">审核模式</strong><button id="boundary-adjust" class="secondary">调整边界</button><button id="boundary-insert" class="secondary" disabled>在所选边插点</button><button id="boundary-delete" class="secondary" disabled>删除所选顶点</button><button id="boundary-undo" class="secondary" disabled>撤销</button><button id="boundary-restore" class="secondary" disabled>恢复候选原状</button><span id="boundary-validity" class="hint"></span></section><aside class="boundary-right"><h2>来源与覆盖</h2><div id="boundary-evidence"></div></aside><footer class="boundary-confirmation"><div><strong>下一步：获取建筑、交通、水域、植被和体育五类地物</strong><br><span class="hint">一起保存编辑后边界和发现快照，并沿用同一 Dataset Bundle。</span></div><button id="boundary-back" class="secondary">返回 Campus Target</button><button id="boundary-confirm" disabled>确认边界并开始五类采集</button></footer></div>"#.to_string()
                },
                r#"
const english=__ENGLISH__;
let desk=__BOUNDARY_DESK__;
let candidates=desk&&Array.isArray(desk.candidates)?desk.candidates:[];
let activeIndex=Math.max(0,candidates.findIndex(item=>item.id===(desk&&desk.selectedCandidateId)));
let adjustment=false,selectedVertex=null,selectedEdge=null,pending=false,markers=[],edgeHits=[];
document.getElementById('bar').classList.add('boundary-mode');
const tx=(zh,en)=>english?en:zh;
const esc=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const active=()=>candidates[activeIndex];
const blocked=()=>pending?tx('正在等待项目验证','Waiting for project validation'):((desk&&desk.confirmationBlockedReason)||(!active()?'No automatic Campus Boundary candidate is available':(!active().valid?(active().invalidReasons[0]||'The automatic Campus Boundary candidate is invalid'):'')));
const emit=operation=>{pending=true;post({type:'mapBoundaryOperation',candidateId:active().id,operation});renderDesk()};
const clearHandles=()=>{if(markers.length)map.remove(markers);if(edgeHits.length)map.remove(edgeHits);markers=[];edgeHits=[]};
const renderHandles=()=>{
  clearHandles();if(!adjustment||!active()||!active().valid)return;
  edgeHits=points.map((point,index)=>{const line=new AMap.Polyline({path:[point,points[(index+1)%points.length]],strokeColor:selectedEdge===index?'#e1a23a':'#a54836',strokeOpacity:selectedEdge===index?.9:.03,strokeWeight:selectedEdge===index?6:18,zIndex:120});line.on('click',()=>{selectedEdge=index;selectedVertex=null;post({type:'mapBoundaryHandleSelected',candidateId:active().id,selection:{type:'edge',edgeIndex:index}});renderDesk()});return line});
  markers=points.map((point,index)=>{const marker=new AMap.Marker({position:point,draggable:!pending&&selectedVertex===index,anchor:'center',content:`<div class="boundary-vertex ${selectedVertex===index?'selected':''}">${index+1}</div>`,zIndex:130});marker.on('click',()=>{selectedVertex=index;selectedEdge=null;post({type:'mapBoundaryHandleSelected',candidateId:active().id,selection:{type:'vertex',vertexIndex:index}});renderDesk()});marker.on('dragend',event=>{if(selectedVertex!==index)return;emit({type:'move_vertex',vertexIndex:index,coordinate:{lng:event.lnglat.lng,lat:event.lnglat.lat}})});return marker});
  map.add(edgeHits);map.add(markers);
};
const selectCandidate=index=>{activeIndex=index;adjustment=false;selectedVertex=null;selectedEdge=null;pending=true;post({type:'mapBoundaryCandidateSelected',candidateId:active().id});renderDesk()};
const renderDesk=()=>{
  const item=active();
  document.getElementById('boundary-candidates').innerHTML=candidates.map((candidate,index)=>`<button class="boundary-candidate ${index===activeIndex?'selected':''} ${candidate.valid?'':'invalid'}" data-candidate="${index}"><strong>${candidate.valid?'#'+candidate.rank:tx('无效','Invalid')}</strong><span>${esc(candidate.label)}</span><small>${esc(candidate.sourceSummary)}</small><small>${esc(candidate.valid?candidate.rankingSummary:(candidate.invalidReasons[0]||''))}</small></button>`).join('');
  document.querySelectorAll('[data-candidate]').forEach(button=>button.onclick=()=>selectCandidate(Number(button.dataset.candidate)));
  document.getElementById('boundary-evidence').innerHTML=item?`<p><strong>${esc(item.label)}</strong></p><p>${esc(item.lineageSummary)}</p><p><strong>Dataset Bundle</strong><br>${esc(desk.datasetBundleSummary)}</p><p><strong>Coverage</strong><br>${esc(desk.coverageSummary)}</p><p><strong>${tx('排名依据','Ranking evidence')}</strong><br>${esc(item.rankingSummary)}</p><p class="${item.valid?'':'invalid-copy'}">${item.valid?tx('几何有效，可进入调整或确认。','Geometry is valid and may be adjusted or confirmed.'):esc(item.invalidReasons.join(' · '))}</p>`:'';
  const recovery=document.getElementById('boundary-recovery'),message=(desk&&desk.recoveryMessage)||(!candidates.length?tx('没有可用边界证据；不会启用空白绘制。','No boundary evidence is available; blank-canvas drawing stays disabled.'):'');
  recovery.innerHTML=message?`<div class="boundary-recovery"><strong>${esc(message)}</strong><button id="boundary-retry">${tx('重试同一任务','Retry same job')}</button><button id="boundary-return">${tx('返回校区确认','Return to campus')}</button></div>`:'';
  document.getElementById('boundary-retry')?.addEventListener('click',()=>post({type:'mapBoundaryRetryRequested'}));
  document.getElementById('boundary-return')?.addEventListener('click',()=>post({type:'mapBoundaryReturnToCampusRequested'}));
  document.getElementById('boundary-mode-label').textContent=adjustment?tx('调整模式 · 先选择点或边','Adjustment mode · select a vertex or edge'):tx('审核模式 · 地图浏览不会修改边界','Review mode · map browsing cannot edit');
  const adjust=document.getElementById('boundary-adjust');adjust.disabled=!item||!item.valid;adjust.textContent=adjustment?tx('退出调整','Leave adjustment'):tx('调整边界','Adjust boundary');
  document.getElementById('boundary-insert').disabled=pending||!adjustment||selectedEdge===null;
  document.getElementById('boundary-delete').disabled=pending||!adjustment||selectedVertex===null||points.length<=3;
  document.getElementById('boundary-undo').disabled=pending||!(desk&&desk.canUndo);
  document.getElementById('boundary-restore').disabled=pending||!adjustment||!item;
  const reason=blocked(),confirm=document.getElementById('boundary-confirm');confirm.disabled=!!reason;confirm.title=reason;document.getElementById('boundary-validity').textContent=reason?tx('不可确认：','Blocked: ')+reason:tx('当前编辑几何有效','Edited geometry is valid');
  renderHandles();
};
window.applyBoundaryDesk=next=>{desk=next;candidates=desk&&Array.isArray(desk.candidates)?desk.candidates:[];activeIndex=Math.max(0,candidates.findIndex(item=>item.id===(desk&&desk.selectedCandidateId)));points=(desk&&Array.isArray(desk.workingPoints)?desk.workingPoints:[]).map(p=>[p.lng,p.lat]);pending=false;selectedVertex=null;selectedEdge=null;redraw();if(polygon)map.setFitView([polygon],false,[90,360,120,360]);renderDesk()};
points=(desk&&Array.isArray(desk.workingPoints)?desk.workingPoints:[]).map(p=>[p.lng,p.lat]);redraw();
document.getElementById('boundary-adjust').onclick=()=>{if(!active()||!active().valid)return;adjustment=!adjustment;selectedVertex=null;selectedEdge=null;post({type:'mapBoundaryAdjustmentChanged',candidateId:active().id,enabled:adjustment});renderDesk()};
document.getElementById('boundary-insert').onclick=()=>{if(selectedEdge===null)return;emit({type:'insert_vertex',edgeIndex:selectedEdge})};
document.getElementById('boundary-delete').onclick=()=>{if(selectedVertex===null||points.length<=3)return;emit({type:'delete_vertex',vertexIndex:selectedVertex})};
document.getElementById('boundary-undo').onclick=()=>emit({type:'undo'});
document.getElementById('boundary-restore').onclick=()=>{if(active())emit({type:'restore_candidate_original'})};
document.getElementById('boundary-back').onclick=()=>post({type:'mapBoundaryReturnToCampusRequested'});
document.getElementById('boundary-confirm').onclick=()=>{const reason=blocked();if(reason){post({type:'error',message:reason});return;}post({type:'mapBoundaryConfirmed',candidateId:active().id})};
renderDesk();"#
                    .replace("__ENGLISH__", if *english { "true" } else { "false" })
                    .replace("__BOUNDARY_DESK__", &boundary_desk),
            ),
            MapPurpose::FoundationReview => (
                if *english {
                    r#"<span class="task">3 · Review pinned Foundation evidence</span><span class="hint">Use the list-first review queue; this map has no drawing or screenshot-recovery actions.</span>"#.to_string()
                } else {
                    r#"<span class="task">3 · 审核已固定的 Foundation 证据</span><span class="hint">请使用列表优先审核队列；此地图不提供绘制或截图恢复操作。</span>"#.to_string()
                },
                String::new(),
            ),
            MapPurpose::BuildingEvidence => (
                if *english {
                    r#"<span class="hint">Green outline: reviewed open geodata · Gaode 3D: human visual evidence</span>"#.to_string()
                } else {
                    r#"<span class="hint">绿色轮廓：已审核开放地理数据 · 高德 3D：人工视觉证据</span>"#.to_string()
                },
                String::new(),
            ),
        };
        format!(
            r#"<!doctype html><html><head><meta charset="utf-8">
<style>html,body,#map{{margin:0;width:100%;height:100%;overflow:hidden;font-family:"Microsoft YaHei UI",sans-serif}}
#bar{{position:absolute;z-index:5;left:16px;top:16px;background:#f4f0e5;border:1px solid #23362e;padding:10px;display:flex;gap:8px;align-items:center;flex-wrap:wrap}}
button{{padding:8px 12px;background:#2f765b;color:white;border:1px solid #23362e}} button.secondary{{background:#eee6d6;color:#17251f}} button:disabled{{opacity:.5;cursor:not-allowed}} input,select{{min-width:170px;padding:8px;background:#fffdf7;color:#17251f;border:1px solid #6d786f}} span{{font-weight:700;color:#17251f}} span.hint{{font-weight:400;color:#506058}}
#bar.boundary-mode{{inset:0;background:transparent;border:0;padding:0;display:block;pointer-events:none}} #bar.boundary-mode>span{{display:none}}
.boundary-shell button,.boundary-shell aside,.boundary-shell section,.boundary-shell footer{{pointer-events:auto}} .boundary-left,.boundary-right{{position:absolute;top:16px;bottom:92px;width:286px;padding:16px;background:rgba(244,240,229,.97);border:1px solid #23362e;overflow:auto;box-sizing:border-box}} .boundary-left{{left:16px}} .boundary-right{{right:16px}} .boundary-tools{{position:absolute;top:16px;left:318px;right:318px;padding:10px;background:rgba(244,240,229,.96);border:1px solid #23362e;display:flex;gap:8px;align-items:center;flex-wrap:wrap}} .boundary-confirmation{{position:absolute;left:16px;right:16px;bottom:16px;min-height:58px;padding:10px 14px;background:rgba(244,240,229,.98);border:1px solid #23362e;display:flex;gap:12px;align-items:center}} .boundary-confirmation>div{{flex:1}}
.boundary-candidate{{width:100%;margin:6px 0;text-align:left;display:grid;gap:3px;background:#fffdf7;color:#17251f}} .boundary-candidate.selected{{border:3px solid #2f765b}} .boundary-candidate.invalid{{border-style:dashed;color:#7c2f25}} .boundary-candidate small{{display:block;color:#506058}} .boundary-recovery{{margin-top:12px;padding:10px;background:#f6ddd5;border:1px solid #a54836;display:grid;gap:8px}} .invalid-copy{{color:#a54836}} .boundary-vertex{{width:22px;height:22px;border-radius:50%;background:#fffdf7;border:3px solid #a54836;color:#17251f;text-align:center;line-height:22px;font-weight:700}} .boundary-vertex.selected{{background:#e1a23a}}</style>
<script>
const post=(value)=>window.ipc.postMessage(JSON.stringify(value));
const pageEnglish={english};
window.addEventListener('error',event=>post({{type:'error',message:(pageEnglish?'Map script error: ':'地图脚本错误：')+String(event.message||event.error||'unknown error')}}));
window.addEventListener('unhandledrejection',event=>post({{type:'error',message:(pageEnglish?'Map promise rejected: ':'地图异步任务失败：')+String(event.reason||'unknown rejection')}}));
</script>
<script>window._AMapSecurityConfig={{securityJsCode:{security}}};</script>
<script src="https://webapi.amap.com/maps?v=2.0&key={key}" onerror="post({{type:'error',message:pageEnglish?'Failed to load the Gaode Maps JavaScript API':'高德地图 JavaScript API 加载失败'}})"></script></head>
<body><div id="map"></div><div id="bar"><span>{campus}</span>{bar}</div>
<script>
const map=new AMap.Map('map',{{viewMode:'3D',zoom:{zoom},pitch:{pitch},rotation:{rotation},center:[{center_lng},{center_lat}],showLabel:false}});
let drawing=false;
let points={initial_points}.map(p=>[p.lng,p.lat]);
let polygon=null;
const redraw=()=>{{if(polygon)map.remove(polygon);polygon=points.length>=2?new AMap.Polygon({{path:points,strokeColor:'#a54836',strokeWeight:4,fillColor:'#a54836',fillOpacity:.14}}):null;if(polygon)map.add(polygon);}};
redraw();
const overlays={overlays};
overlays.forEach((overlay,index)=>{{const item=new AMap.Polygon({{path:overlay.points.map(p=>[p.lng,p.lat]),strokeColor:index%2===0?'#2f765b':'#a54836',strokeWeight:5,fillColor:index%2===0?'#2f765b':'#a54836',fillOpacity:.16}});map.add(item);}});
map.on('click',e=>{{if(drawing){{points.push([e.lnglat.lng,e.lnglat.lat]);redraw();}}else post({{type:'mapPointSelected',lng:e.lnglat.lng,lat:e.lnglat.lat}});}});
map.on('moveend',()=>{{const c=map.getCenter();post({{type:'mapCamera',centerLng:c.lng,centerLat:c.lat,zoom:map.getZoom(),pitch:map.getPitch(),rotation:map.getRotation()}})}});
{editing_script}
</script></body></html>"#,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::{full_window_bounds, map_html, MapPurpose, ToolCommand};
        use campus_tool_protocol::{
            MapBoundaryCandidate, MapBoundaryDesk, MapBoundaryDeskRequest, MapCoordinate,
            MapEvidenceAssessment, MapFoundationReviewCandidate, MapFoundationReviewCategory,
            MapFoundationReviewDesk, MapFoundationReviewDeskRequest, MapProviderOutcome,
        };

        fn map_command(purpose: MapPurpose) -> ToolCommand {
            ToolCommand::OpenMap {
                campus_name: "East China Normal University Putuo Campus".into(),
                center_lng: 0.0,
                center_lat: 0.0,
                zoom: 17.0,
                pitch: 45.0,
                rotation: 0.0,
                js_api_key: String::new(),
                security_code: String::new(),
                boundary: Vec::new(),
                purpose,
                overlays: Vec::new(),
                feature_kind: None,
                english: true,
            }
        }

        fn boundary_map_command() -> ToolCommand {
            ToolCommand::OpenBoundaryDesk {
                request: Box::new(MapBoundaryDeskRequest {
                    campus_name: "East China Normal University Putuo Campus".into(),
                    center_lng: 0.0,
                    center_lat: 0.0,
                    zoom: 17.0,
                    pitch: 45.0,
                    rotation: 0.0,
                    js_api_key: String::new(),
                    security_code: String::new(),
                    desk: MapBoundaryDesk {
                        candidates: vec![
                            MapBoundaryCandidate {
                                id: "boundary-osm-1".into(),
                                rank: 1,
                                label: "OSM education relation".into(),
                                valid: true,
                                invalid_reasons: Vec::new(),
                                points: vec![
                                    MapCoordinate { lng: 1.0, lat: 1.0 },
                                    MapCoordinate { lng: 2.0, lat: 1.0 },
                                    MapCoordinate { lng: 2.0, lat: 2.0 },
                                ],
                                source_summary: "OSM relation/1".into(),
                                ranking_summary: "name match; contains anchor".into(),
                                lineage_summary: "complete relation assembly".into(),
                            },
                            MapBoundaryCandidate {
                                id: "boundary-invalid-2".into(),
                                rank: 2,
                                label: "Incomplete relation".into(),
                                valid: false,
                                invalid_reasons: vec!["missing outer relation members".into()],
                                points: Vec::new(),
                                source_summary: "OSM relation/2".into(),
                                ranking_summary: "partial evidence".into(),
                                lineage_summary: "incomplete relation assembly".into(),
                            },
                        ],
                        selected_candidate_id: Some("boundary-osm-1".into()),
                        working_points: vec![
                            MapCoordinate { lng: 1.0, lat: 1.0 },
                            MapCoordinate { lng: 2.0, lat: 1.0 },
                            MapCoordinate { lng: 2.0, lat: 2.0 },
                        ],
                        can_undo: false,
                        dataset_bundle_summary: "osm-2026-06 + overture-2026-06-17.0".into(),
                        coverage_summary: "complete 12/12 tiles".into(),
                        confirmation_blocked_reason: None,
                        recovery_message: None,
                    },
                    english: true,
                }),
            }
        }

        fn review_desk_command() -> ToolCommand {
            ToolCommand::OpenFoundationReviewDesk {
                request: Box::new(MapFoundationReviewDeskRequest {
                    campus_name: "East China Normal University Putuo Campus".into(),
                    center_lng: 121.4,
                    center_lat: 31.2,
                    zoom: 17.0,
                    pitch: 45.0,
                    rotation: 0.0,
                    js_api_key: String::new(),
                    security_code: String::new(),
                    boundary: Vec::new(),
                    desk: MapFoundationReviewDesk {
                        categories: vec![MapFoundationReviewCategory {
                            id: "building".into(),
                            label: "Buildings".into(),
                            acquisition_state: "complete".into(),
                            disposed: 0,
                            total: 1,
                            pending: 1,
                            blockers: 1,
                            complete: false,
                        }],
                        active_category: "building".into(),
                        candidates: vec![MapFoundationReviewCandidate {
                            id: "building-1".into(),
                            label: "Library".into(),
                            disposition: "pending".into(),
                            priority: "normal".into(),
                            source_summary: "OSM relation/1".into(),
                            lineage_summary: "OSM 2026-06".into(),
                            provenance_summary: "ODbL-1.0".into(),
                            geometry_form: "area".into(),
                            subtype: Some("university".into()),
                            width_summary: None,
                            assessment: MapEvidenceAssessment {
                                geometry: "source geometry".into(),
                                semantics: "typed building".into(),
                                entity_match: "entity review".into(),
                                name_match: "unconfirmed".into(),
                            },
                            geometry: Vec::new(),
                        }],
                        selected_candidate_id: Some("building-1".into()),
                        provider_outcomes: vec![MapProviderOutcome {
                            provider: "osm".into(),
                            tile_id: "tile-1".into(),
                            state: "complete".into(),
                            summary: "1 record".into(),
                        }],
                        known_gaps: Vec::new(),
                        conflicts: Vec::new(),
                        basis_token: "{}".into(),
                        ledger_sequence: 0,
                        completion_blocked_reason: Some(
                            "1 pending candidate requires a disposition".into(),
                        ),
                    },
                    english: true,
                }),
            }
        }

        #[test]
        fn webview_bounds_fill_the_window_at_any_scale_factor() {
            let standard = full_window_bounds(1100, 760, 1.0);
            let standard_size = standard.size.to_logical::<f64>(1.0);
            assert_eq!(standard_size.width, 1100.0);
            assert_eq!(standard_size.height, 760.0);

            let scaled = full_window_bounds(1650, 1140, 1.5);
            let scaled_size = scaled.size.to_logical::<f64>(1.0);
            assert_eq!(scaled_size.width, 1100.0);
            assert_eq!(scaled_size.height, 760.0);
        }

        #[test]
        fn map_page_reports_script_failures_to_the_parent() {
            let html = map_html(&map_command(MapPurpose::CampusSelection));

            assert!(html.contains("window.addEventListener('error'"));
            assert!(html.contains("window.addEventListener('unhandledrejection'"));
            assert!(html.contains("Failed to load the Gaode Maps JavaScript API"));
        }

        #[test]
        fn campus_search_preserves_gaode_errors_and_accepts_supported_result_shapes() {
            let html = map_html(&map_command(MapPurpose::CampusSelection));

            assert!(html.contains("mapSearchFailed"));
            assert!(html.contains("result.data"));
            assert!(!html.contains("DEBUG-gaode-search"));
        }

        #[test]
        fn map_tasks_expose_only_controls_for_the_current_job() {
            let selection = map_html(&map_command(MapPurpose::CampusSelection));
            assert!(selection.contains("Select campus"));
            assert!(selection.contains("Search Gaode"));
            assert!(!selection.contains("Confirm campus boundary"));
            assert!(!selection.contains("Load open data for this view"));
            assert!(!selection.contains("Visual gap recovery"));

            let boundary = map_html(&boundary_map_command());
            assert!(boundary.contains("Automatic Campus Boundary"));
            assert!(boundary.contains("Confirm boundary"));
            assert!(boundary.contains("Ranked candidates"));
            assert!(boundary.contains("Lineage &amp; coverage"));
            assert!(boundary.contains("Adjustment mode"));
            assert!(boundary.contains("mapBoundaryOperation"));
            assert!(boundary.contains("move_vertex"));
            assert!(boundary.contains("restore_candidate_original"));
            assert!(boundary.contains("window.applyBoundaryDesk"));
            assert!(boundary.contains("Waiting for project validation"));
            assert!(!boundary.contains("geometryReason"));
            assert!(!boundary.contains("history.push"));
            assert!(!boundary.contains("operation,points:points.map"));
            assert!(boundary.contains("mapBoundaryRetryRequested"));
            assert!(boundary.contains("missing outer relation members"));
            assert!(!boundary.contains("Click the map to add boundary nodes"));
            assert!(!boundary.contains("id=\"clear\""));
            assert!(!boundary.contains("Search Gaode"));
            assert!(!boundary.contains("Visual gap recovery"));

            let review = map_html(&map_command(MapPurpose::FoundationReview));
            assert!(review.contains("Review pinned Foundation evidence"));
            assert!(review.contains("list-first review queue"));
            assert!(!review.contains("Load open data for this view"));
            assert!(!review.contains("Visual gap recovery"));
            assert!(!review.contains("mapCaptureRequested"));
            assert!(!review.contains("mapVisualCapture"));
            assert!(!review.contains("mapFeatureDrawn"));
            assert!(!review.contains("Search Gaode"));
            assert!(!review.contains("Confirm boundary"));

            let review_queue = map_html(&review_desk_command());
            assert!(review_queue.contains("Foundation five-category review"));
            assert!(review_queue.contains("Candidate queue"));
            assert!(review_queue.contains("Evidence assessment"));
            assert!(review_queue.contains("Known Feature Gaps"));
            assert!(review_queue.contains("mapFoundationBatchReviewRequested"));
            assert!(review_queue.contains("window.applyFoundationReviewDesk"));
            assert!(review_queue.contains("no blank-canvas drawing or screenshot recovery"));
            assert!(!review_queue.contains("Visual gap recovery"));
            assert!(!review_queue.contains("MapFeatureDrawn"));
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("{error}");
    }
}
