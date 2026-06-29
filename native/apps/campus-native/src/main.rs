#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arnis_core::{FootprintComponent, GenerateBuildingRequest, MaterialOverrides};
use campus_state::{
    ArnisStylePreset, DesktopApplicationState, DesktopMode, FeatureKind, FoundationStep, GeoPoint,
    MapCandidate, MapViewState, ReviewDecision,
};
use campus_tool_protocol::{
    read_message, write_message, MapCoordinate, ToolCommand, ToolEvent, ToolKind, PROTOCOL_VERSION,
};
use rand::Rng;
use slint::{ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
#[cfg(target_os = "windows")]
use tokio::net::windows::named_pipe::ServerOptions;

slint::include_modules!();

fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CampusReconstructionTool")
}

fn default_project_path() -> PathBuf {
    app_data_dir().join("projects").join("active.campus.json")
}

fn generated_model_dir() -> PathBuf {
    app_data_dir().join("generated")
}

fn project_file_dialog(save: bool) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("Campus Reconstruction Project", &["json"])
        .set_directory(app_data_dir());
    if save {
        dialog = dialog.set_file_name("campus-project.campus.json");
        dialog.save_file()
    } else {
        dialog.pick_file()
    }
}

fn schematic_file_dialog(default_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Sponge Schematic", &["schem"])
        .set_directory(
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(app_data_dir)
                .join("Downloads"),
        )
        .set_file_name(default_name)
        .save_file()
}

#[derive(Clone, Default)]
struct MapCredentials {
    js_api_key: String,
    security_code: String,
}

fn credential_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Campus Reconstruction Tool", account).map_err(|error| error.to_string())
}

fn load_map_credentials() -> MapCredentials {
    let from_store = |account: &str| {
        credential_entry(account)
            .and_then(|entry| entry.get_password().map_err(|error| error.to_string()))
            .unwrap_or_default()
    };
    let stored_key = from_store("gaode-js-api-key");
    let stored_security = from_store("gaode-security-code");
    MapCredentials {
        js_api_key: if stored_key.is_empty() {
            std::env::var("GAODE_JS_API_KEY")
                .or_else(|_| std::env::var("VITE_GAODE_JS_API_KEY"))
                .unwrap_or_default()
        } else {
            stored_key
        },
        security_code: if stored_security.is_empty() {
            std::env::var("GAODE_SECURITY_CODE")
                .or_else(|_| std::env::var("VITE_GAODE_SECURITY_CODE"))
                .unwrap_or_default()
        } else {
            stored_security
        },
    }
}

fn save_map_credentials(credentials: &MapCredentials) -> Result<(), String> {
    credential_entry("gaode-js-api-key")?
        .set_password(&credentials.js_api_key)
        .map_err(|error| error.to_string())?;
    credential_entry("gaode-security-code")?
        .set_password(&credentials.security_code)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn autosave(state: &Rc<RefCell<DesktopApplicationState>>) -> Result<(), String> {
    let path = state
        .borrow()
        .project_path
        .clone()
        .unwrap_or_else(default_project_path);
    state.borrow_mut().save_to(path)
}

fn page_copy(step: FoundationStep) -> (&'static str, &'static str, &'static str) {
    match step {
        FoundationStep::Campus => (
            "选择学校与具体校区",
            "确认校区后再发现建筑、道路、水域、植被与体育设施。",
            "确认校区并继续",
        ),
        FoundationStep::Boundary => (
            "确认校园边界",
            "调整地图缩放与轮廓，边界只定义校区范围，不代替地基要素。",
            "确认边界并继续",
        ),
        FoundationStep::Orientation => (
            "确定 Minecraft 朝向",
            "选择一条校内主轴作为 Minecraft 水平轴，后续所有要素共用此方向。",
            "确认朝向并继续",
        ),
        FoundationStep::Building => (
            "审核建筑与补缺",
            "接受可靠建筑候选；缺失建筑通过当前地图视野或人工绘制补充。",
            "完成建筑审核",
        ),
        FoundationStep::Road => (
            "审核道路",
            "审核主路与步道，并调整道路宽度。",
            "完成道路审核",
        ),
        FoundationStep::Water => ("审核水域", "审核河道、池塘与景观水面。", "完成水域审核"),
        FoundationStep::Vegetation => {
            ("审核植被", "审核树林、草地与主要绿化区域。", "完成植被审核")
        }
        FoundationStep::Sports => (
            "审核体育设施",
            "审核操场、球场与其他体育区域。",
            "完成体育审核",
        ),
        FoundationStep::Export => (
            "导出校园地基",
            "检查范围、方块数量与建筑槽位后导出工程。",
            "完成 Foundation",
        ),
    }
}

fn sync_ui(ui: &AppWindow, state: &DesktopApplicationState) {
    let Some(project) = &state.project else {
        ui.set_save_status("未保存".into());
        return;
    };
    ui.set_project_name(project.name.clone().into());
    ui.set_campus_name(project.campus_name.clone().into());
    ui.set_detailed_active(project.mode == DesktopMode::Detailed);
    ui.set_active_step(
        FoundationStep::ALL
            .iter()
            .position(|step| *step == project.foundation_step)
            .unwrap_or(0) as i32,
    );
    let (title, help, primary) = if project.mode == DesktopMode::Detailed {
        (
            "精细建筑模板",
            "实测轮廓、高度与楼层保持不变；模板只控制方块、窗户与墙面质感。",
            "",
        )
    } else {
        page_copy(project.foundation_step)
    };
    ui.set_page_title(title.into());
    ui.set_page_help(help.into());
    ui.set_primary_label(primary.into());
    ui.set_save_status(
        if state.dirty {
            "待保存"
        } else {
            "已保存"
        }
        .into(),
    );
    ui.set_project_summary(
        format!(
            "{} 个候选 · {} 个已采用地物 · {} 个建筑槽位",
            project.candidates.len(),
            project.features.len(),
            project.building_slots.len()
        )
        .into(),
    );
    let current_kind = match project.foundation_step {
        FoundationStep::Building => Some(FeatureKind::Building),
        FoundationStep::Road => Some(FeatureKind::Road),
        FoundationStep::Water => Some(FeatureKind::Water),
        FoundationStep::Vegetation => Some(FeatureKind::Vegetation),
        FoundationStep::Sports => Some(FeatureKind::Sports),
        _ => None,
    };
    let candidates = project
        .candidates
        .iter()
        .filter(|candidate| current_kind.is_none() || current_kind == Some(candidate.kind))
        .map(|candidate| CandidateRow {
            id: candidate.id.clone().into(),
            name: candidate.name.clone().into(),
            meta: format!("{} · {}", candidate.source, candidate.confidence).into(),
            status: match candidate.review {
                ReviewDecision::Pending => "待审核",
                ReviewDecision::Accepted => "已接受",
                ReviewDecision::Rejected => "已拒绝",
            }
            .into(),
        })
        .collect::<Vec<_>>();
    ui.set_candidates(ModelRc::new(VecModel::from(candidates)));
    let slots = if project.building_slots.is_empty() {
        vec![SharedString::from("暂无已审核建筑")]
    } else {
        project
            .building_slots
            .iter()
            .map(|slot| SharedString::from(slot.name.as_str()))
            .collect()
    };
    ui.set_building_slots(ModelRc::new(VecModel::from(slots)));
    let selected_slot = project
        .detailed
        .selected_slot_id
        .as_deref()
        .and_then(|id| project.building_slots.iter().position(|slot| slot.id == id))
        .unwrap_or(0);
    ui.set_selected_slot(selected_slot as i32);
    let selected_measurements = project.building_slots.get(selected_slot);
    ui.set_measured_height(
        selected_measurements
            .and_then(|slot| slot.height_m)
            .map(|value| format!("{value:.2}"))
            .unwrap_or_default()
            .into(),
    );
    ui.set_measured_floors(
        selected_measurements
            .and_then(|slot| slot.floors)
            .map(|value| value.to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_measured_roof(
        selected_measurements
            .and_then(|slot| slot.roof_shape.clone())
            .unwrap_or_default()
            .into(),
    );
    ui.set_palette_summary(generated_palette_summary(project).into());
    let selected_style = ArnisStylePreset::ALL
        .iter()
        .position(|preset| *preset == project.detailed.style_preset)
        .unwrap_or(8);
    ui.set_selected_style(selected_style as i32);
    ui.set_window_density(project.detailed.window_density as f32);
    ui.set_wall_depth(project.detailed.wall_depth as f32);
    ui.set_orientation_degrees(project.orientation_degrees as f32);
    ui.set_blocks_per_meter(project.blocks_per_meter as f32);
    ui.set_can_undo(state.can_undo());
    ui.set_can_redo(state.can_redo());
}

fn save_and_sync(
    ui: &AppWindow,
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(), String> {
    autosave(state)?;
    sync_ui(ui, &state.borrow());
    Ok(())
}

fn set_error(ui: &AppWindow, message: impl AsRef<str>) {
    ui.set_save_status(format!("保存失败：{}", message.as_ref()).into());
}

fn generate_detailed_model(
    state: &Rc<RefCell<DesktopApplicationState>>,
) -> Result<(PathBuf, String), String> {
    let (slot, style, density, depth, scale) = {
        let borrowed = state.borrow();
        let project = borrowed.project.as_ref().ok_or("请先创建项目")?;
        let slot = project
            .detailed
            .selected_slot_id
            .as_deref()
            .and_then(|id| project.building_slots.iter().find(|slot| slot.id == id))
            .or_else(|| project.building_slots.first())
            .cloned()
            .ok_or("请先在地基模式接受至少一个建筑候选")?;
        (
            slot,
            project.detailed.style_preset,
            project.detailed.window_density,
            project.detailed.wall_depth,
            project.blocks_per_meter,
        )
    };
    let generated = arnis_core::generate_building(GenerateBuildingRequest {
        candidate_id: slot.id.clone(),
        source: "campus-project".into(),
        components: vec![FootprintComponent {
            exterior: slot
                .footprint
                .iter()
                .map(|point| arnis_core::GeoPoint {
                    lng: point.lng,
                    lat: point.lat,
                })
                .collect(),
            interior_rings: Vec::new(),
        }],
        height_m: slot.height_m,
        floors: slot.floors,
        roof_shape: slot.roof_shape.clone(),
        blocks_per_meter: scale,
        seed: 42,
        materials: MaterialOverrides::default(),
        correction_notes: vec![format!("V1 fixed Arnis preset: {}", style.slug())],
        parts: Vec::new(),
        style_preset: style.slug().into(),
        window_density: density,
        wall_depth: depth,
    })?;
    std::fs::create_dir_all(generated_model_dir()).map_err(|error| error.to_string())?;
    let safe_id = slot
        .id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let path = generated_model_dir().join(format!("{safe_id}.arnis.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    state.borrow_mut().mutate_project(|project| {
        project.detailed.selected_slot_id = Some(slot.id.clone());
        project.detailed.generated_path = Some(path.clone());
        if let Some(target) = project
            .building_slots
            .iter_mut()
            .find(|target| target.id == slot.id)
        {
            target.refined = true;
        }
    });
    Ok((path, slot.name))
}

fn generated_palette_summary(project: &campus_state::CampusProject) -> String {
    let Some(path) = &project.detailed.generated_path else {
        return "尚未生成模型".into();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return "生成文件已移动，请重新生成".into();
    };
    let Ok(generated) = serde_json::from_slice::<arnis_core::GeneratedBuilding>(&bytes) else {
        return "生成文件格式无效".into();
    };
    generated
        .palette
        .iter()
        .filter(|block| block.as_str() != "minecraft:air")
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn normalize_minecraft_block(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, ':' | '_' | '[' | ']' | '=' | ',')
        })
    {
        return Err("方块 ID 无效".into());
    }
    Ok(if value.contains(':') {
        value
    } else {
        format!("minecraft:{value}")
    })
}

fn replace_generated_block(path: &PathBuf, source: &str, target: &str) -> Result<usize, String> {
    let source = normalize_minecraft_block(source)?;
    let target = normalize_minecraft_block(target)?;
    if source == "minecraft:air" {
        return Err("V1 不允许把全部空气作为替换来源".into());
    }
    if source == target {
        return Err("原方块和新方块相同".into());
    }
    let mut generated: arnis_core::GeneratedBuilding =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let sources = generated
        .palette
        .iter()
        .enumerate()
        .filter(|(_, block)| **block == source)
        .map(|(index, _)| index as u16)
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Err(format!("模型中没有 {source}"));
    }
    let target_index = generated
        .palette
        .iter()
        .position(|block| *block == target)
        .unwrap_or_else(|| {
            generated.palette.push(target);
            generated.palette.len() - 1
        }) as u16;
    let mut replaced = 0usize;
    for run in &mut generated.block_runs {
        if sources.contains(&run.palette_index) {
            replaced += run.run_length as usize;
            run.palette_index = target_index;
        }
    }
    let mut merged: Vec<arnis_core::BlockRun> = Vec::new();
    for run in generated.block_runs {
        if let Some(previous) = merged.last_mut() {
            if previous.palette_index == run.palette_index {
                previous.run_length += run.run_length;
                continue;
            }
        }
        merged.push(run);
    }
    generated.block_runs = merged;
    generated.report.correction_notes.push(format!(
        "batch replacement: {source} -> {}",
        generated.palette[target_index as usize]
    ));
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&generated).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(replaced)
}

enum ToolUpdate {
    Status(String),
    MapCamera {
        center: GeoPoint,
        zoom: f64,
        pitch: f64,
        rotation: f64,
    },
    MapPoint(GeoPoint),
    MapBoundary(Vec<GeoPoint>),
    MapCapture {
        south_west: GeoPoint,
        north_east: GeoPoint,
        candidates: Result<Vec<MapCandidate>, String>,
    },
}

#[derive(Clone)]
struct ToolSupervisor {
    children: Arc<Mutex<Vec<Child>>>,
    updates: mpsc::Sender<ToolUpdate>,
}

impl ToolSupervisor {
    fn tool_executable(name: &str) -> Result<PathBuf, String> {
        let directory = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or("native executable has no parent")?
            .to_path_buf();
        let executable = directory.join(format!("{name}.exe"));
        executable
            .exists()
            .then_some(executable)
            .ok_or_else(|| format!("{name}.exe is not installed beside the main application"))
    }

    #[cfg(target_os = "windows")]
    fn launch_map(
        &self,
        ui: slint::Weak<AppWindow>,
        campus_name: String,
        view: MapViewState,
        boundary: Vec<GeoPoint>,
        js_api_key: String,
        security_code: String,
    ) -> Result<(), String> {
        let executable = Self::tool_executable("campus-map")?;
        let random: u128 = rand::rng().random();
        let pipe = format!(r"\\.\pipe\campus-reconstruction-{random:032x}");
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        self.children
            .lock()
            .map_err(|_| "tool child lock poisoned")?
            .push(child);
        let updates = self.updates.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("map supervisor runtime");
            let error_ui = ui.clone();
            let result: Result<(), String> = runtime.block_on(async move {
                let mut server = server;
                server.connect().await.map_err(|error| error.to_string())?;
                let hello: ToolCommand = read_message(&mut server).await?;
                match hello {
                    ToolCommand::Hello {
                        protocol_version,
                        session_token,
                        tool: ToolKind::Map,
                    } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                    _ => return Err("map tool handshake rejected".into()),
                }
                write_message(
                    &mut server,
                    &ToolCommand::OpenMap {
                        campus_name,
                        center_lng: view.center.lng,
                        center_lat: view.center.lat,
                        zoom: view.zoom,
                        pitch: view.pitch,
                        rotation: view.rotation,
                        js_api_key,
                        security_code,
                        boundary: boundary
                            .into_iter()
                            .map(|point| MapCoordinate {
                                lng: point.lng,
                                lat: point.lat,
                            })
                            .collect(),
                    },
                )
                .await?;
                loop {
                    let event: ToolEvent = read_message(&mut server).await?;
                    let message = match event {
                        ToolEvent::Ready { .. } => "高德工具已连接".to_string(),
                        ToolEvent::MapCamera {
                            center_lng,
                            center_lat,
                            zoom,
                            pitch,
                            rotation,
                        } => {
                            let _ = updates.send(ToolUpdate::MapCamera {
                                center: GeoPoint {
                                    lng: center_lng,
                                    lat: center_lat,
                                },
                                zoom,
                                pitch,
                                rotation,
                            });
                            format!("地图视角已记录 · zoom {zoom:.1} · pitch {pitch:.0}")
                        }
                        ToolEvent::MapPointSelected { lng, lat } => {
                            let _ = updates.send(ToolUpdate::MapPoint(GeoPoint { lng, lat }));
                            format!("已选择位置 {lng:.6}, {lat:.6}")
                        }
                        ToolEvent::MapBoundaryChanged { points } => {
                            let count = points.len();
                            let _ = updates.send(ToolUpdate::MapBoundary(
                                points
                                    .into_iter()
                                    .map(|point| GeoPoint {
                                        lng: point.lng,
                                        lat: point.lat,
                                    })
                                    .collect(),
                            ));
                            format!("校区边界已保存 · {count} 个节点")
                        }
                        ToolEvent::MapCaptureRequested {
                            south_west_lng,
                            south_west_lat,
                            north_east_lng,
                            north_east_lat,
                        } => {
                            let south_west = GeoPoint {
                                lng: south_west_lng,
                                lat: south_west_lat,
                            };
                            let north_east = GeoPoint {
                                lng: north_east_lng,
                                lat: north_east_lat,
                            };
                            let _ = updates.send(ToolUpdate::Status(
                                "已锁定当前视野，正在识别校区地物…".into(),
                            ));
                            let bounds = campus_services::GeoBounds {
                                west: south_west_lng,
                                south: south_west_lat,
                                east: north_east_lng,
                                north: north_east_lat,
                            };
                            let overture_endpoint = std::env::var("CAMPUS_DATA_SERVICE_URL")
                                .or_else(|_| std::env::var("OVERTURE_BUILDING_ENDPOINT"))
                                .ok();
                            let candidates = campus_services::query_campus_data(
                                bounds,
                                overture_endpoint.as_deref(),
                            )
                            .await;
                            let _ = updates.send(ToolUpdate::MapCapture {
                                south_west,
                                north_east,
                                candidates,
                            });
                            "已接收当前视野范围".to_string()
                        }
                        ToolEvent::Error { message } => format!("地图错误：{message}"),
                        ToolEvent::Closed { .. } => "高德工具已关闭".to_string(),
                        _ => continue,
                    };
                    let weak = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_save_status(message.into());
                        }
                    });
                }
            });
            if let Err(error) = result {
                let weak = error_ui;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_save_status(format!("地图连接失败：{error}").into());
                    }
                });
            }
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn launch_map(
        &self,
        _ui: slint::Weak<AppWindow>,
        _campus_name: String,
        _view: MapViewState,
        _boundary: Vec<GeoPoint>,
        _js_api_key: String,
        _security_code: String,
    ) -> Result<(), String> {
        Err("map tool is supported only on Windows".into())
    }

    #[cfg(target_os = "windows")]
    fn launch_preview(
        &self,
        ui: slint::Weak<AppWindow>,
        model_path: PathBuf,
        title: String,
    ) -> Result<(), String> {
        let executable = Self::tool_executable("campus-preview")?;
        let random: u128 = rand::rng().random();
        let pipe = format!(r"\\.\pipe\campus-reconstruction-preview-{random:032x}");
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        self.children
            .lock()
            .map_err(|_| "tool child lock poisoned")?
            .push(child);
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("preview supervisor runtime");
            let error_ui = ui.clone();
            let result: Result<(), String> = runtime.block_on(async move {
                let mut server = server;
                server.connect().await.map_err(|error| error.to_string())?;
                let hello: ToolCommand = read_message(&mut server).await?;
                match hello {
                    ToolCommand::Hello {
                        protocol_version,
                        session_token,
                        tool: ToolKind::Preview,
                    } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                    _ => return Err("preview tool handshake rejected".into()),
                }
                write_message(
                    &mut server,
                    &ToolCommand::OpenPreview {
                        model_path: model_path.to_string_lossy().into_owned(),
                        title,
                    },
                )
                .await?;
                loop {
                    let event: ToolEvent = read_message(&mut server).await?;
                    let message = match event {
                        ToolEvent::Ready { .. } => "原生 3D 预览已连接".to_string(),
                        ToolEvent::PreviewBlockSelected { x, y, z, block } => {
                            format!("方块 {block} · {x}, {y}, {z}")
                        }
                        ToolEvent::Error { message } => format!("预览错误：{message}"),
                        ToolEvent::Closed { .. } => "原生预览已关闭".to_string(),
                        _ => continue,
                    };
                    let weak = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_save_status(message.into());
                        }
                    });
                }
            });
            if let Err(error) = result {
                let weak = error_ui;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_save_status(format!("预览连接失败：{error}").into());
                    }
                });
            }
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn launch_preview(
        &self,
        _ui: slint::Weak<AppWindow>,
        _model_path: PathBuf,
        _title: String,
    ) -> Result<(), String> {
        Err("preview tool is supported only on Windows".into())
    }
}

impl Drop for ToolSupervisor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.children) != 1 {
            return;
        }
        if let Ok(mut children) = self.children.lock() {
            for child in children.iter_mut() {
                let _ = child.kill();
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_step_labels(ModelRc::new(VecModel::from(
        FoundationStep::ALL
            .iter()
            .map(|step| SharedString::from(step.label()))
            .collect::<Vec<_>>(),
    )));
    ui.set_arnis_styles(ModelRc::new(VecModel::from(
        ArnisStylePreset::ALL
            .iter()
            .map(|preset| SharedString::from(preset.label()))
            .collect::<Vec<_>>(),
    )));

    let state = Rc::new(RefCell::new(DesktopApplicationState::default()));
    let map_credentials = Rc::new(RefCell::new(load_map_credentials()));
    ui.set_gaode_key(map_credentials.borrow().js_api_key.clone().into());
    ui.set_gaode_security(map_credentials.borrow().security_code.clone().into());
    let (tool_update_tx, tool_update_rx) = mpsc::channel();
    let tools = ToolSupervisor {
        children: Arc::new(Mutex::new(Vec::new())),
        updates: tool_update_tx,
    };
    let tool_update_rx = Rc::new(RefCell::new(tool_update_rx));
    if default_project_path().exists() {
        let _ = state.borrow_mut().open(default_project_path());
    }
    if state.borrow().project.is_none() {
        state
            .borrow_mut()
            .new_project("未命名项目", "华东师范大学普陀校区");
    }
    sync_ui(&ui, &state.borrow());

    let tool_timer = Timer::default();
    {
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_save_map_settings(move |key, security| {
            let updated = MapCredentials {
                js_api_key: key.to_string(),
                security_code: security.to_string(),
            };
            if let Some(ui) = weak.upgrade() {
                match save_map_credentials(&updated) {
                    Ok(()) => {
                        *map_credentials.borrow_mut() = updated;
                        ui.set_save_status("地图密钥已安全保存".into());
                    }
                    Err(error) => {
                        ui.set_save_status(format!("地图密钥保存失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        let receiver = tool_update_rx.clone();
        tool_timer.start(
            TimerMode::Repeated,
            std::time::Duration::from_millis(120),
            move || {
                let mut changed = false;
                while let Ok(update) = receiver.borrow_mut().try_recv() {
                    match update {
                        ToolUpdate::Status(message) => {
                            if let Some(ui) = weak.upgrade() {
                                ui.set_save_status(message.into());
                            }
                        }
                        ToolUpdate::MapCamera {
                            center,
                            zoom,
                            pitch,
                            rotation,
                        } => {
                            state.borrow_mut().mutate_project(|project| {
                                project.map_view.center = center;
                                project.map_view.zoom = zoom;
                                project.map_view.pitch = pitch;
                                project.map_view.rotation = rotation;
                            });
                            changed = true;
                        }
                        ToolUpdate::MapPoint(point) => {
                            state.borrow_mut().mutate_project(|project| {
                                project.map_view.center = point;
                            });
                            changed = true;
                        }
                        ToolUpdate::MapBoundary(points) => {
                            state.borrow_mut().mutate_project(|project| {
                                project.boundary = points;
                            });
                            changed = true;
                        }
                        ToolUpdate::MapCapture {
                            south_west,
                            north_east,
                            candidates,
                        } => match candidates {
                            Ok(mut discovered) => {
                                let count = discovered.len();
                                state.borrow_mut().mutate_project(|project| {
                                    project.map_view.capture_bounds =
                                        Some([south_west, north_east]);
                                    if project.boundary.is_empty() {
                                        project.boundary = vec![
                                            south_west,
                                            GeoPoint {
                                                lng: north_east.lng,
                                                lat: south_west.lat,
                                            },
                                            north_east,
                                            GeoPoint {
                                                lng: south_west.lng,
                                                lat: north_east.lat,
                                            },
                                        ];
                                    }
                                    for candidate in &mut discovered {
                                        if let Some(previous) = project
                                            .candidates
                                            .iter()
                                            .find(|previous| previous.id == candidate.id)
                                        {
                                            candidate.review = previous.review;
                                        }
                                    }
                                    project.candidates.retain(|candidate| {
                                        candidate.review != ReviewDecision::Pending
                                            || candidate.source != "OpenStreetMap / Overpass"
                                    });
                                    project.candidates.extend(discovered);
                                });
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_save_status(
                                        format!("识别完成：发现 {count} 个校区候选").into(),
                                    );
                                }
                                changed = true;
                            }
                            Err(error) => {
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_save_status(format!("识别失败：{error}").into());
                                }
                            }
                        },
                    }
                }
                if changed {
                    if let Some(ui) = weak.upgrade() {
                        if let Err(error) = save_and_sync(&ui, &state) {
                            set_error(&ui, error);
                        }
                    }
                }
            },
        );
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_undo(move || {
            if state.borrow_mut().undo() {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = save_and_sync(&ui, &state) {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_redo(move || {
            if state.borrow_mut().redo() {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = save_and_sync(&ui, &state) {
                        set_error(&ui, error);
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_set_foundation_metrics(move |orientation, scale| {
            state.borrow_mut().mutate_project(|project| {
                project.orientation_degrees = orientation.clamp(-180.0, 180.0) as f64;
                project.blocks_per_meter = scale.clamp(0.25, 4.0) as f64;
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                } else {
                    ui.set_save_status("朝向与比例已应用".into());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_create_project(move |name, campus| {
            state
                .borrow_mut()
                .new_project(name.as_str(), campus.as_str());
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open_project(move || {
            let Some(path) = project_file_dialog(false) else {
                return;
            };
            let result = state.borrow_mut().open(path);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => sync_ui(&ui, &state.borrow()),
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_save_project(move || {
            let result = autosave(&state);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => sync_ui(&ui, &state.borrow()),
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_save_project_as(move || {
            let Some(path) = project_file_dialog(true) else {
                return;
            };
            let result = state.borrow_mut().save_to(path);
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(()) => sync_ui(&ui, &state.borrow()),
                    Err(error) => set_error(&ui, error),
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_switch_mode(move |detailed| {
            state.borrow_mut().set_mode(if detailed {
                DesktopMode::Detailed
            } else {
                DesktopMode::Foundation
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
                ui.window().request_redraw();
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_select_step(move |index| {
            let Some(step) = FoundationStep::ALL.get(index.max(0) as usize).copied() else {
                return;
            };
            state.borrow_mut().mutate_project(|project| {
                project.foundation_step = step;
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_confirm_step(move || {
            state
                .borrow_mut()
                .mutate_project(|project| project.confirm_step());
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_review_candidate(move |id, accepted| {
            state.borrow_mut().mutate_project(|project| {
                if accepted {
                    project.accept_candidate(id.as_str());
                } else {
                    project.reject_candidate(id.as_str());
                }
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_update_building_measurements(move |height, floors, roof| {
            let parsed_height = height
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0);
            let parsed_floors = floors.trim().parse::<u32>().ok().filter(|value| *value > 0);
            let parsed_roof = match roof.trim().to_ascii_lowercase().as_str() {
                "" => None,
                "flat" | "gabled" | "hipped" | "skillion" | "pyramidal" | "dome" | "cone"
                | "onion" => Some(roof.trim().to_ascii_lowercase()),
                _ => {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_save_status("屋顶形状无效".into());
                    }
                    return;
                }
            };
            state.borrow_mut().mutate_project(|project| {
                let selected_id = project.detailed.selected_slot_id.clone();
                if let Some(slot) = project.building_slots.iter_mut().find(|slot| {
                    selected_id
                        .as_deref()
                        .map(|id| slot.id == id)
                        .unwrap_or(true)
                }) {
                    slot.height_m = parsed_height;
                    slot.floors = parsed_floors;
                    slot.roof_shape = parsed_roof;
                }
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                } else {
                    ui.set_save_status("实测几何已保存，模板不会修改这些数值".into());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_slot(move |index| {
            state.borrow_mut().mutate_project(|project| {
                project.detailed.selected_slot_id = project
                    .building_slots
                    .get(index.max(0) as usize)
                    .map(|slot| slot.id.clone());
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_choose_style(move |index, density, depth| {
            let preset = ArnisStylePreset::ALL
                .get(index.max(0) as usize)
                .copied()
                .unwrap_or_default();
            state.borrow_mut().mutate_project(|project| {
                project.detailed.style_preset = preset;
                project.detailed.window_density = density.clamp(0.0, 100.0) as u8;
                project.detailed.wall_depth = depth.clamp(0.0, 100.0) as u8;
            });
            if let Some(ui) = weak.upgrade() {
                if let Err(error) = save_and_sync(&ui, &state) {
                    set_error(&ui, error);
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let map_credentials = map_credentials.clone();
        let weak = ui.as_weak();
        ui.on_open_map(move || {
            let campus_name = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.campus_name.clone())
                .unwrap_or_else(|| "未命名校区".into());
            let view = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.map_view.clone())
                .unwrap_or_default();
            let boundary = state
                .borrow()
                .project
                .as_ref()
                .map(|project| project.boundary.clone())
                .unwrap_or_default();
            let credentials = map_credentials.borrow().clone();
            if credentials.js_api_key.trim().is_empty() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_visible(true);
                    ui.set_save_status("请先配置高德 Web JS API 密钥".into());
                }
                return;
            }
            if let Err(error) = tools.launch_map(
                weak.clone(),
                campus_name,
                view,
                boundary,
                credentials.js_api_key,
                credentials.security_code,
            ) {
                if let Some(ui) = weak.upgrade() {
                    ui.set_save_status(format!("地图启动失败：{error}").into());
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_open_preview(move || {
            let model = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| project.detailed.generated_path.clone());
            let Some(model) = model else {
                if let Some(ui) = weak.upgrade() {
                    ui.set_save_status("请先生成精细建筑".into());
                }
                return;
            };
            let title = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| {
                    project
                        .detailed
                        .selected_slot_id
                        .as_deref()
                        .and_then(|id| project.building_slots.iter().find(|slot| slot.id == id))
                        .map(|slot| slot.name.clone())
                })
                .unwrap_or_else(|| "精细建筑".into());
            if let Err(error) = tools.launch_preview(weak.clone(), model, title) {
                if let Some(ui) = weak.upgrade() {
                    ui.set_save_status(format!("预览启动失败：{error}").into());
                }
            }
        });
    }
    {
        let tools = tools.clone();
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_generate_building(move || match generate_detailed_model(&state) {
            Ok((path, title)) => {
                if let Some(ui) = weak.upgrade() {
                    if let Err(error) = autosave(&state) {
                        set_error(&ui, error);
                        return;
                    }
                    sync_ui(&ui, &state.borrow());
                }
                if let Err(error) = tools.launch_preview(weak.clone(), path, title) {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_save_status(format!("生成成功，预览启动失败：{error}").into());
                    }
                }
            }
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_save_status(format!("生成失败：{error}").into());
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_replace_generated_block(move |source, target| {
            let path = state
                .borrow()
                .project
                .as_ref()
                .and_then(|project| project.detailed.generated_path.clone());
            let result = path
                .as_ref()
                .ok_or_else(|| "请先生成精细建筑".to_string())
                .and_then(|path| replace_generated_block(path, source.as_str(), target.as_str()));
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(count) => {
                        sync_ui(&ui, &state.borrow());
                        ui.set_save_status(format!("已替换 {count} 个方块，请重新打开预览").into());
                    }
                    Err(error) => {
                        ui.set_save_status(format!("替换失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_building(move || {
            let result = (|| -> Result<PathBuf, String> {
                let generated_path = state
                    .borrow()
                    .project
                    .as_ref()
                    .and_then(|project| project.detailed.generated_path.clone())
                    .ok_or("请先生成当前精细建筑")?;
                let generated: arnis_core::GeneratedBuilding = serde_json::from_slice(
                    &std::fs::read(&generated_path).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let default_name = format!("{}.schem", generated.report.candidate_id);
                let path = schematic_file_dialog(&default_name).ok_or("已取消导出")?;
                let model = campus_export::model_from_runs(
                    generated.width,
                    generated.height,
                    generated.length,
                    generated.palette,
                    generated
                        .block_runs
                        .into_iter()
                        .map(|run| (run.palette_index, run.run_length)),
                )?;
                campus_export::write_schematic(&path, &generated.report.candidate_id, &model)?;
                Ok(path)
            })();
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(path) => {
                        ui.set_save_status(format!("精细建筑已导出：{}", path.display()).into());
                    }
                    Err(error) if error != "已取消导出" => {
                        ui.set_save_status(format!("导出失败：{error}").into());
                    }
                    Err(_) => {}
                }
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        ui.on_export_project(move || {
            let result = (|| -> Result<PathBuf, String> {
                let project = state.borrow().project.clone().ok_or("请先创建项目")?;
                let default_name = format!("{}.schem", project.name);
                let path = schematic_file_dialog(&default_name).ok_or("已取消导出")?;
                let model = campus_export::foundation_model(&project)?;
                campus_export::write_schematic(&path, &project.name, &model)?;
                let project_path = path.with_extension("campus.json");
                std::fs::write(
                    &project_path,
                    serde_json::to_vec_pretty(&project).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                Ok(path)
            })();
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(path) => {
                        ui.set_save_status(format!("Foundation 已导出：{}", path.display()).into());
                    }
                    Err(error) if error != "已取消导出" => {
                        ui.set_save_status(format!("导出失败：{error}").into());
                    }
                    Err(_) => {}
                }
            }
        });
    }

    ui.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_fixture_generates_preview_model() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/v1-demo.campus.json");
        let mut loaded = DesktopApplicationState::default();
        loaded.open(fixture).unwrap();
        let state = Rc::new(RefCell::new(loaded));
        let (path, title) = generate_detailed_model(&state).unwrap();
        let generated: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(title, "验收图书馆");
        assert!(generated.report.non_air_blocks > 1_000);
        assert!(generated
            .palette
            .iter()
            .any(|block| block.contains("glass")));
        let source = generated
            .palette
            .iter()
            .find(|block| block.as_str() != "minecraft:air")
            .unwrap()
            .clone();
        let replaced =
            replace_generated_block(&path, &source, "minecraft:purple_concrete").unwrap();
        assert!(replaced > 0);
        let updated: arnis_core::GeneratedBuilding =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(updated
            .palette
            .iter()
            .any(|block| block == "minecraft:purple_concrete"));
        let _ = std::fs::remove_file(path);
    }
}
