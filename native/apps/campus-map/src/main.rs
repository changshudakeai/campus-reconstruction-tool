#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("campus-map is currently supported only on Windows");
}

#[cfg(target_os = "windows")]
mod windows {
    use campus_tool_protocol::{
        read_message, write_message, MapPurpose, ToolCommand, ToolEvent, ToolKind, PROTOCOL_VERSION,
    };
    use std::sync::mpsc::{self, Sender};
    use std::thread;
    use tokio::net::windows::named_pipe::ClientOptions;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::{Window, WindowId};
    use wry::{WebView, WebViewBuilder};

    pub fn run() -> Result<(), String> {
        let pipe = std::env::args()
            .nth(1)
            .ok_or("missing named pipe argument")?;
        let token = std::env::args().nth(2).ok_or("missing session token")?;
        if std::env::var_os("CAMPUS_MAP_HEADLESS").is_some() {
            return run_headless(pipe, token);
        }
        let (config, event_tx) = connect(pipe, token)?;
        let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
        let mut app = MapApplication {
            window: None,
            webview: None,
            config,
            event_tx,
        };
        event_loop
            .run_app(&mut app)
            .map_err(|error| error.to_string())
    }

    fn connect(pipe: String, token: String) -> Result<(ToolCommand, Sender<ToolEvent>), String> {
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
        let (tx, rx) = mpsc::channel::<ToolEvent>();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("map pipe runtime");
            runtime.block_on(async move {
                while let Ok(event) = rx.recv() {
                    if write_message(&mut client, &event).await.is_err() {
                        break;
                    }
                }
            });
        });
        Ok((config, tx))
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
            if !matches!(command, ToolCommand::OpenMap { .. }) {
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
    }

    impl ApplicationHandler for MapApplication {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("高德 3D 校区工具")
                        .with_inner_size(winit::dpi::LogicalSize::new(1100, 760)),
                )
                .expect("create map window");
            let html = map_html(&self.config);
            let tx = self.event_tx.clone();
            let webview = WebViewBuilder::new()
                .with_html(html)
                .with_ipc_handler(move |request| {
                    if let Ok(event) = serde_json::from_str::<ToolEvent>(request.body()) {
                        let _ = tx.send(event);
                    }
                })
                .build_as_child(&window)
                .expect("create map webview");
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
            if event == WindowEvent::CloseRequested {
                let _ = self.event_tx.send(ToolEvent::Closed {
                    tool: ToolKind::Map,
                });
                event_loop.exit();
            }
        }
    }

    fn map_html(command: &ToolCommand) -> String {
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
        let (bar, editing_script) = if *purpose == MapPurpose::CampusReview {
            (
                r#"<button id="draw" class="secondary">绘制边界</button><button id="clear" class="secondary">清空边界</button><button id="save" class="secondary">保存边界</button><button id="capture">截取并识别当前视野</button>"#,
                r#"
document.getElementById('draw').onclick=()=>{drawing=!drawing;document.getElementById('draw').textContent=drawing?'完成点选':'绘制边界';};
document.getElementById('clear').onclick=()=>{points=[];redraw();};
document.getElementById('save').onclick=()=>{if(points.length>=3)post({type:'mapBoundaryChanged',points:points.map(p=>({lng:p[0],lat:p[1]}))});};
document.getElementById('capture').onclick=()=>{const b=map.getBounds();const sw=b.getSouthWest(),ne=b.getNorthEast();post({type:'mapCaptureRequested',southWestLng:sw.lng,southWestLat:sw.lat,northEastLng:ne.lng,northEastLat:ne.lat})};"#,
            )
        } else {
            (
                r#"<span class="hint">绿色轮廓：已审核开放地理数据 · 高德 3D：人工视觉证据</span>"#,
                "",
            )
        };
        format!(
            r#"<!doctype html><html><head><meta charset="utf-8">
<style>html,body,#map{{margin:0;width:100%;height:100%;overflow:hidden;font-family:"Microsoft YaHei UI",sans-serif}}
#bar{{position:absolute;z-index:5;left:16px;top:16px;background:#f4f0e5;border:1px solid #23362e;padding:10px;display:flex;gap:8px;align-items:center;flex-wrap:wrap}}
button{{padding:8px 12px;background:#2f765b;color:white;border:1px solid #23362e}} button.secondary{{background:#eee6d6;color:#17251f}} span{{font-weight:700;color:#17251f}} span.hint{{font-weight:400;color:#506058}}</style>
<script>window._AMapSecurityConfig={{securityJsCode:{security}}};</script>
<script src="https://webapi.amap.com/maps?v=2.0&key={key}"></script></head>
<body><div id="map"></div><div id="bar"><span>{campus}</span>{bar}</div>
<script>
const post=(value)=>window.ipc.postMessage(JSON.stringify(value));
const map=new AMap.Map('map',{{viewMode:'3D',zoom:{zoom},pitch:{pitch},rotation:{rotation},center:[{center_lng},{center_lat}],showLabel:false}});
let drawing=false;
let points={boundary}.map(p=>[p.lng,p.lat]);
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
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("{error}");
    }
}
