#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("campus-preview is currently supported only on Windows");
}

#[cfg(target_os = "windows")]
mod windows {
    use campus_tool_protocol::{
        forward_tool_events, read_message, write_message, ToolCommand, ToolEvent, ToolKind,
        PROTOCOL_VERSION,
    };
    use pixels::{Pixels, SurfaceTexture};
    use serde::Deserialize;
    use std::fs;
    use std::sync::mpsc::{self, Sender};
    use std::sync::Arc;
    use std::thread;
    use tokio::net::windows::named_pipe::ClientOptions;
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseButton, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::window::{Window, WindowId};

    const WIDTH: u32 = 1100;
    const HEIGHT: u32 = 760;
    type PipeThread = thread::JoinHandle<Result<(), String>>;
    type ToolConnection = (ToolCommand, Sender<ToolEvent>, PipeThread);

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BlockRun {
        palette_index: u16,
        run_length: u32,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Model {
        width: usize,
        height: usize,
        length: usize,
        palette: Vec<String>,
        block_runs: Vec<BlockRun>,
    }

    #[derive(Clone)]
    struct Voxel {
        x: f32,
        y: f32,
        z: f32,
        color: [u8; 3],
        block: String,
    }

    pub fn run() -> Result<(), String> {
        let pipe = std::env::args()
            .nth(1)
            .ok_or("missing named pipe argument")?;
        let token = std::env::args().nth(2).ok_or("missing session token")?;
        if std::env::var_os("CAMPUS_PREVIEW_HEADLESS").is_some() {
            return run_headless(pipe, token);
        }
        let (command, event_tx, pipe_thread) = connect(pipe, token)?;
        let ToolCommand::OpenPreview {
            model_path,
            title,
            english,
        } = command
        else {
            return Err("invalid preview request".into());
        };
        let model: Model = serde_json::from_slice(
            &fs::read(&model_path).map_err(|error| format!("read preview model: {error}"))?,
        )
        .map_err(|error| format!("parse preview model: {error}"))?;
        let voxels = decode(&model);
        let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
        let mut app = PreviewApplication {
            window: None,
            pixels: None,
            title,
            english,
            voxels,
            model_size: [model.width as f32, model.height as f32, model.length as f32],
            yaw: -0.72,
            pitch: 0.55,
            zoom: 1.0,
            dragging: false,
            cursor: None,
            press_cursor: None,
            last_projected: Vec::new(),
            event_tx,
            pipe_thread: Some(pipe_thread),
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
                tool: ToolKind::Preview,
            },
        ))?;
        let command = runtime.block_on(read_message(&mut client))?;
        let (tx, rx) = mpsc::channel::<ToolEvent>();
        let pipe_thread = thread::spawn(move || runtime.block_on(forward_tool_events(client, rx)));
        Ok((command, tx, pipe_thread))
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
                    tool: ToolKind::Preview,
                },
            )
            .await?;
            let command: ToolCommand = read_message(&mut client).await?;
            let ToolCommand::OpenPreview { model_path, .. } = command else {
                return Err("invalid preview request".into());
            };
            let _: Model =
                serde_json::from_slice(&fs::read(model_path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            write_message(
                &mut client,
                &ToolEvent::Ready {
                    protocol_version: PROTOCOL_VERSION,
                    tool: ToolKind::Preview,
                },
            )
            .await?;
            write_message(
                &mut client,
                &ToolEvent::Closed {
                    tool: ToolKind::Preview,
                },
            )
            .await
        })
    }

    fn decode(model: &Model) -> Vec<Voxel> {
        let mut result = Vec::new();
        let mut index = 0usize;
        let total = model.width * model.height * model.length;
        let stride = (total / 220_000).max(1);
        for run in &model.block_runs {
            let block = model
                .palette
                .get(run.palette_index as usize)
                .map(String::as_str)
                .unwrap_or("minecraft:air");
            let color = block_color(block);
            for _ in 0..run.run_length {
                if block != "minecraft:air" && index.is_multiple_of(stride) {
                    let x = index % model.width;
                    let z = (index / model.width) % model.length;
                    let y = index / (model.width * model.length);
                    result.push(Voxel {
                        x: x as f32,
                        y: y as f32,
                        z: z as f32,
                        color,
                        block: block.to_string(),
                    });
                }
                index += 1;
            }
        }
        result
    }

    fn block_color(block: &str) -> [u8; 3] {
        if block.contains("glass") {
            [126, 181, 190]
        } else if block.contains("brick") {
            [151, 72, 55]
        } else if block.contains("quartz") || block.contains("white") {
            [222, 216, 197]
        } else if block.contains("deepslate") || block.contains("black") {
            [55, 62, 60]
        } else if block.contains("copper") {
            [102, 145, 112]
        } else if block.contains("wood") || block.contains("plank") || block.contains("oak") {
            [144, 111, 71]
        } else if block.contains("green") || block.contains("grass") {
            [76, 124, 79]
        } else {
            [154, 154, 145]
        }
    }

    struct PreviewApplication {
        window: Option<Arc<Window>>,
        pixels: Option<Pixels<'static>>,
        title: String,
        english: bool,
        voxels: Vec<Voxel>,
        model_size: [f32; 3],
        yaw: f32,
        pitch: f32,
        zoom: f32,
        dragging: bool,
        cursor: Option<(f64, f64)>,
        press_cursor: Option<(f64, f64)>,
        last_projected: Vec<(i32, i32, usize)>,
        event_tx: Sender<ToolEvent>,
        pipe_thread: Option<thread::JoinHandle<Result<(), String>>>,
    }

    impl PreviewApplication {
        fn finish(&mut self, event_loop: &ActiveEventLoop, error: Option<String>) {
            if let Some(message) = error {
                let _ = self.event_tx.send(ToolEvent::Error { message });
            }
            let _ = self.event_tx.send(ToolEvent::Closed {
                tool: ToolKind::Preview,
            });
            if let Some(pipe_thread) = self.pipe_thread.take() {
                let _ = pipe_thread.join();
            }
            event_loop.exit();
        }
    }

    impl ApplicationHandler for PreviewApplication {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = match event_loop.create_window(
                Window::default_attributes()
                    .with_title(format!(
                        "{} · {}",
                        if self.english {
                            "Native Block Preview"
                        } else {
                            "原生方块预览"
                        },
                        self.title
                    ))
                    .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT)),
            ) {
                Ok(window) => Arc::new(window),
                Err(error) => {
                    self.finish(
                        event_loop,
                        Some(format!("create preview window failed: {error}")),
                    );
                    return;
                }
            };
            let size = window.inner_size();
            let texture = SurfaceTexture::new(size.width, size.height, window.clone());
            let pixels = match Pixels::new(WIDTH, HEIGHT, texture) {
                Ok(pixels) => pixels,
                Err(error) => {
                    self.finish(
                        event_loop,
                        Some(format!("create preview pixel surface failed: {error}")),
                    );
                    return;
                }
            };
            let _ = self.event_tx.send(ToolEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
                tool: ToolKind::Preview,
            });
            self.pixels = Some(pixels);
            self.window = Some(window);
            self.redraw();
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => {
                    self.finish(event_loop, None);
                }
                WindowEvent::RedrawRequested => {
                    self.render();
                    let render_error = self
                        .pixels
                        .as_mut()
                        .and_then(|pixels| pixels.render().err())
                        .map(|error| format!("render preview failed: {error}"));
                    if render_error.is_some() {
                        self.finish(event_loop, render_error);
                    }
                }
                WindowEvent::Resized(size) => {
                    if size.width > 0 && size.height > 0 {
                        let resize_error = self
                            .pixels
                            .as_mut()
                            .and_then(|pixels| pixels.resize_surface(size.width, size.height).err())
                            .map(|error| format!("resize preview surface failed: {error}"));
                        if resize_error.is_some() {
                            self.finish(event_loop, resize_error);
                        }
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    if state == ElementState::Pressed {
                        self.dragging = true;
                        self.press_cursor = self.cursor;
                    } else {
                        self.dragging = false;
                        if let (Some(start), Some(end)) = (self.press_cursor.take(), self.cursor) {
                            if (start.0 - end.0).hypot(start.1 - end.1) < 5.0 {
                                self.inspect_at(end);
                            }
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    if let Some((x, y)) = self.cursor {
                        if self.dragging {
                            self.yaw += (position.x - x) as f32 * 0.008;
                            self.pitch =
                                (self.pitch + (position.y - y) as f32 * 0.006).clamp(-1.2, 1.2);
                            self.redraw();
                        }
                    }
                    self.cursor = Some((position.x, position.y));
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let amount = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                        winit::event::MouseScrollDelta::PixelDelta(value) => value.y as f32 / 40.0,
                    };
                    self.zoom = (self.zoom * (1.0 + amount * 0.08)).clamp(0.25, 5.0);
                    self.redraw();
                }
                _ => {}
            }
        }
    }

    impl PreviewApplication {
        fn redraw(&self) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        fn render(&mut self) {
            let Some(pixels) = &mut self.pixels else {
                return;
            };
            let frame = pixels.frame_mut();
            for pixel in frame.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[244, 240, 229, 255]);
            }
            let center = [
                self.model_size[0] * 0.5,
                self.model_size[1] * 0.45,
                self.model_size[2] * 0.5,
            ];
            let scale = (620.0 / self.model_size[0].max(self.model_size[2]).max(12.0)) * self.zoom;
            let (sy, cy) = self.yaw.sin_cos();
            let (sp, cp) = self.pitch.sin_cos();
            let mut projected = self
                .voxels
                .iter()
                .enumerate()
                .map(|(index, voxel)| {
                    let x = voxel.x - center[0];
                    let y = voxel.y - center[1];
                    let z = voxel.z - center[2];
                    let rx = x * cy - z * sy;
                    let rz = x * sy + z * cy;
                    let ry = y * cp - rz * sp;
                    let depth = y * sp + rz * cp;
                    (
                        depth,
                        (WIDTH as f32 * 0.5 + rx * scale) as i32,
                        (HEIGHT as f32 * 0.55 - ry * scale) as i32,
                        voxel.color,
                        index,
                    )
                })
                .collect::<Vec<_>>();
            projected.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            self.last_projected.clear();
            let radius = (scale * 0.58).clamp(1.0, 5.0) as i32;
            for (_, x, y, color, voxel_index) in projected {
                self.last_projected.push((x, y, voxel_index));
                for py in y - radius..=y + radius {
                    for px in x - radius..=x + radius {
                        if px >= 0 && py >= 0 && px < WIDTH as i32 && py < HEIGHT as i32 {
                            let index = (py as usize * WIDTH as usize + px as usize) * 4;
                            frame[index..index + 4]
                                .copy_from_slice(&[color[0], color[1], color[2], 255]);
                        }
                    }
                }
            }
        }

        fn inspect_at(&self, cursor: (f64, f64)) {
            let Some(window) = &self.window else {
                return;
            };
            let size = window.inner_size();
            let target_x = (cursor.0 * WIDTH as f64 / size.width.max(1) as f64) as i32;
            let target_y = (cursor.1 * HEIGHT as f64 / size.height.max(1) as f64) as i32;
            let selected = self
                .last_projected
                .iter()
                .rev()
                .filter_map(|(x, y, index)| {
                    let distance = (target_x - *x).pow(2) + (target_y - *y).pow(2);
                    (distance <= 144).then_some((distance, *index))
                })
                .min_by_key(|(distance, _)| *distance);
            if let Some((_, index)) = selected {
                if let Some(voxel) = self.voxels.get(index) {
                    let _ = self.event_tx.send(ToolEvent::PreviewBlockSelected {
                        x: voxel.x as i32,
                        y: voxel.y as i32,
                        z: voxel.z as i32,
                        block: voxel.block.clone(),
                    });
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows::run() {
        eprintln!("{error}");
    }
}
