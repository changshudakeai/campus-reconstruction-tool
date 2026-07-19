#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("campus-map is currently supported only on Windows");
}

#[cfg(target_os = "windows")]
mod windows {
    use campus_tool_protocol::{
        forward_tool_events, read_message, write_message, MapBoundaryDesk, MapPurpose, ToolCommand,
        ToolEvent, ToolKind, PROTOCOL_VERSION,
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
                ToolCommand::OpenMap { .. } | ToolCommand::OpenBoundaryDesk { .. }
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
            feature_kind,
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
        let initial_points = if *purpose == MapPurpose::FoundationFeatureDrawing {
            "[]".to_string()
        } else {
            boundary.clone()
        };
        let feature_kind =
            serde_json::to_string(feature_kind.as_deref().unwrap_or("building")).unwrap();
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
                    r#"<span class="task">3 · Review foundation data</span><button id="query">Load open data for this view</button><button id="capture" class="secondary">Visual gap recovery</button>"#.to_string()
                } else {
                    r#"<span class="task">3 · 审核地基数据</span><button id="query">加载当前视野开放数据</button><button id="capture" class="secondary">视觉补缺</button>"#.to_string()
                },
                r#"
const english=__ENGLISH__;
document.getElementById('query').onclick=()=>{const b=map.getBounds();const sw=b.getSouthWest(),ne=b.getNorthEast();post({type:'mapCaptureRequested',southWestLng:sw.lng,southWestLat:sw.lat,northEastLng:ne.lng,northEastLat:ne.lat})};
document.getElementById('capture').onclick=()=>{
  const source=[...document.querySelectorAll('#map canvas')].sort((a,b)=>b.width*b.height-a.width*a.height)[0];
  if(!source){post({type:'error',message:english?'No map canvas is available to capture':'当前地图没有可截取的画布'});return;}
  const maxSide=800,scale=Math.min(1,maxSide/Math.max(source.width,source.height));
  const target=document.createElement('canvas');
  target.width=Math.max(1,Math.round(source.width*scale));target.height=Math.max(1,Math.round(source.height*scale));
  const context=target.getContext('2d',{alpha:false});context.drawImage(source,0,0,target.width,target.height);
  const b=map.getBounds(),sw=b.getSouthWest(),ne=b.getNorthEast();
  post({type:'mapVisualCapture',imageDataUrl:target.toDataURL('image/png'),southWestLng:sw.lng,southWestLat:sw.lat,northEastLng:ne.lng,northEastLat:ne.lat});
};"#
                    .replace("__ENGLISH__", if *english { "true" } else { "false" }),
            ),
            MapPurpose::FoundationFeatureDrawing => (
                if *english {
                    r#"<button id="draw" class="secondary">Start points</button><button id="clear" class="secondary">Clear</button><button id="save">Save manual feature</button><span class="hint">Click to add nodes; roads need 2+, areas need 3+</span>"#.to_string()
                } else {
                    r#"<button id="draw" class="secondary">开始点选</button><button id="clear" class="secondary">清空</button><button id="save">保存手绘地物</button><span class="hint">单击依次添加节点；道路至少 2 点，区域至少 3 点</span>"#.to_string()
                },
                format!(
                    r#"
const featureKind={feature_kind};
const english={english};
drawing=true;
document.getElementById('draw').onclick=()=>{{drawing=!drawing;document.getElementById('draw').textContent=drawing?(english?'Finish points':'完成点选'):(english?'Continue points':'继续点选');}};
document.getElementById('clear').onclick=()=>{{points=[];redraw();}};
document.getElementById('save').onclick=()=>{{const minimum=featureKind==='road'?2:3;if(points.length>=minimum)post({{type:'mapFeatureDrawn',kind:featureKind,points:points.map(p=>({{lng:p[0],lat:p[1]}}))}});}};"#,
                    english = english
                ),
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
            assert!(review.contains("Review foundation data"));
            assert!(review.contains("Load open data for this view"));
            assert!(review.contains("Visual gap recovery"));
            assert!(!review.contains("Search Gaode"));
            assert!(!review.contains("Confirm boundary"));
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("{error}");
    }
}
