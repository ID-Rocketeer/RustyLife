//! # RustyLife Server
//!
//! The `rustylife-server` binary provides a central simulation host that
//! orchestrates the cell grid and exposes interfaces for various clients.
//!
//! Features:
//! - **IPC (TCP)**: High-speed binary protocol for native clients.
//! - **Web (HTTP/WS)**: Real-time dashboard with binary WebSocket updates.
//! - **GUI**: Optional integrated visualization.

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use clap::Parser;
use rustylife_core::{
    BinaryPacket, Request, Response, SimulationPresenter,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Enable the integrated native GUI
    #[arg(short, long)]
    pub gui: bool,

    /// Port for the web interface
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    /// Port for the native client (IPC)
    #[arg(short, long, default_value_t = 9001)]
    pub ipc_port: u16,

    /// Initial seed pattern (glider, blinker)
    #[arg(short, long)]
    pub seed: Option<String>,
}

/// Broadcasts only the generation number when a snapshot is ready.
pub struct ServerEngineSubscriber {
    pub engine: Arc<SimulationEngine>,
    pub tx: broadcast::Sender<u64>,
}

impl EngineSubscriber for ServerEngineSubscriber {
    fn on_snapshot_available(&self, path: std::path::PathBuf) -> bool {
        if path.exists() {
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                if file_name.starts_with("gen_") && file_name.ends_with(".bin") {
                    if let Ok(generation_count) = file_name[4..file_name.len() - 4].parse::<u64>() {
                        let _ = self.tx.send(generation_count);
                    }
                }
            }
        }
        true
    }
}

/// A subscriber that drives an abstract presenter using file-based snapshots.
pub struct PresenterSubscriber {
    pub presenter: Arc<Mutex<dyn SimulationPresenter>>,
}

impl EngineSubscriber for PresenterSubscriber {
    fn on_snapshot_available(&self, path: std::path::PathBuf) -> bool {
        if let Ok(buf) = std::fs::read(&path) {
            if let Ok(mut packet) = rustylife_core::decode_binary_packet(&buf) {
                let mut presenter = self.presenter.lock().unwrap();

                // Filter based on Presenter's viewport
                if let Some(((min_x, min_y), (max_x, max_y))) = presenter.get_viewport() {
                    let filtered: Vec<_> = packet
                        .cells
                        .into_iter()
                        .filter(|((x, y), _)| {
                            *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y
                        })
                        .collect();
                    packet.cells = filtered;
                    packet.record_count = packet.cells.len() as u64;
                }

                presenter.update_state(packet);
            }
        }
        true
    }
}

struct AppStateEnv {
    engine: Arc<SimulationEngine>,
    tx: broadcast::Sender<u64>,
}

struct RustyLifeGuiState {
    viewport_cells: Vec<((i128, i128), u8)>,
    generation: u64,
    is_running: bool,
    target_viewport: Option<((i128, i128), (i128, i128))>,
}

impl SimulationPresenter for RustyLifeGuiState {
    fn update_state(&mut self, packet: BinaryPacket) {
        self.viewport_cells = packet.cells;
        self.generation = packet.generation;
        self.is_running = packet.is_running;
    }

    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        self.target_viewport
    }
}

struct RustyLifeGui {
    state: Arc<Mutex<RustyLifeGuiState>>,
    engine: Arc<SimulationEngine>,
    cell_size: f32,
    view_offset: egui::Vec2,
    last_generation: u64,
}

impl RustyLifeGui {
    fn new(engine: Arc<SimulationEngine>, state: Arc<Mutex<RustyLifeGuiState>>) -> Self {
        Self {
            state,
            engine,
            cell_size: 10.0,
            view_offset: egui::Vec2::ZERO,
            last_generation: u64::MAX, // Force initial update
        }
    }
}

impl eframe::App for RustyLifeGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (cells, generation) = {
            let s = self.state.lock().unwrap();
            (s.viewport_cells.clone(), s.generation)
        };

        if generation == 0 && self.last_generation != 0 && self.view_offset != egui::Vec2::ZERO {
            self.view_offset = egui::Vec2::ZERO;
        }
        self.last_generation = generation;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!(
                "RustyLife Native GUI (Integrated - Gen {})",
                generation
            ));

            let is_running = !self.engine.is_stopped();

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!is_running, egui::Button::new("Run"))
                    .clicked()
                {
                    self.engine.start();
                }
                if ui.button("Step").clicked() {
                    self.engine.step();
                }
                if ui
                    .add_enabled(is_running, egui::Button::new("Stop"))
                    .clicked()
                {
                    self.engine.stop();
                }
                if ui
                    .add_enabled(!is_running, egui::Button::new("Reset"))
                    .clicked()
                {
                    self.engine.reset();
                }

                ui.separator();

                ui.add_enabled_ui(!is_running, |ui| {
                    egui::ComboBox::from_label("Patterns")
                        .selected_text("Select Pattern...")
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(false, "glider").clicked() {
                                self.engine.seed("glider".to_string());
                            }
                            if ui.selectable_label(false, "r-pentomino").clicked() {
                                self.engine.seed("r-pentomino".to_string());
                            }
                            if ui.selectable_label(false, "glider gun").clicked() {
                                self.engine.seed("glider gun".to_string());
                            }
                            if ui.selectable_label(false, "spaceship").clicked() {
                                self.engine.seed("spaceship".to_string());
                            }
                            if ui.selectable_label(false, "blinker").clicked() {
                                self.engine.seed("blinker".to_string());
                            }
                            if ui.selectable_label(false, "block").clicked() {
                                self.engine.seed("block".to_string());
                            }
                            if ui.selectable_label(false, "beehive").clicked() {
                                self.engine.seed("beehive".to_string());
                            }
                            if ui.selectable_label(false, "breeder 1").clicked() {
                                self.engine.seed("breeder 1".to_string());
                            }
                        });
                });
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Zoom:");
                if ui.button("In (+)").clicked() || ui.input(|i| i.key_pressed(egui::Key::Plus)) {
                    self.cell_size = (self.cell_size + 1.0).min(32.0);
                }
                if ui.button("Out (-)").clicked() || ui.input(|i| i.key_pressed(egui::Key::Minus)) {
                    self.cell_size = (self.cell_size - 1.0).max(1.0);
                }
                ui.label(format!("{:.0}px", self.cell_size));
            });

            let born = cells.iter().filter(|(_, s)| *s == 0b10).count();
            let stable = cells.iter().filter(|(_, s)| *s == 0b11).count();
            let dying = cells.iter().filter(|(_, s)| *s == 0b01).count();

            ui.label(format!(
                "Alive: {}, NewlyBorn: {}, Dying: {}",
                stable, born, dying
            ));

            ui.separator();

            // Simulation Rendering
            egui::Frame::canvas(ui.style())
                .fill(egui::Color32::BLACK) // Universe background
                .show(ui, |ui| {
                    let (response, painter) =
                        ui.allocate_painter(ui.available_size(), egui::Sense::drag());
                    let rect = response.rect;

                    if response.dragged() {
                        self.view_offset += response.drag_delta();
                    }

                    // Handle Zooming (Mouse Wheel)
                    if response.hovered() {
                        let zoom_delta = ui.input(|i| i.raw_scroll_delta.y);
                        if zoom_delta != 0.0 {
                            let pointer_pos =
                                ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                            let current_center = rect.center() + self.view_offset;

                            // Calculate world coordinate under local pointer
                            let offset_from_center = pointer_pos - current_center;
                            let world_x = offset_from_center.x / self.cell_size;
                            let world_y = offset_from_center.y / self.cell_size;

                            let delta = if zoom_delta > 0.0 { 1.0 } else { -1.0 };
                            let new_cell_size = (self.cell_size + delta).clamp(1.0, 32.0);

                            if new_cell_size != self.cell_size {
                                self.view_offset = pointer_pos
                                    - rect.center()
                                    - egui::vec2(world_x * new_cell_size, world_y * new_cell_size);
                                self.cell_size = new_cell_size;
                            }
                        }
                    }

                    let center = rect.center() + self.view_offset;

                    // Calculate visible bounds
                    let min_x = ((rect.min.x - center.x) / self.cell_size).floor() as i128;
                    let max_x = ((rect.max.x - center.x) / self.cell_size).ceil() as i128;
                    let min_y = ((rect.min.y - center.y) / self.cell_size).floor() as i128;
                    let max_y = ((rect.max.y - center.y) / self.cell_size).ceil() as i128;

                    {
                        let mut s = self.state.lock().unwrap();
                        s.target_viewport = Some(((min_x, min_y), (max_x, max_y)));
                    }

                    for ((x, y), state) in cells {
                        let color = match state {
                            0b11 => egui::Color32::from_rgb(59, 130, 246), // Alive (Blue)
                            0b10 => egui::Color32::from_rgb(16, 185, 129), // Born (Green)
                            0b01 => egui::Color32::from_rgb(239, 68, 68),  // Dying (Red)
                            _ => continue,
                        };

                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(
                                    center.x + (x as f32 * self.cell_size),
                                    center.y + (y as f32 * self.cell_size),
                                ),
                                egui::vec2(
                                    if self.cell_size <= 1.0 {
                                        self.cell_size
                                    } else {
                                        self.cell_size - 1.0
                                    },
                                    if self.cell_size <= 1.0 {
                                        self.cell_size
                                    } else {
                                        self.cell_size - 1.0
                                    },
                                ),
                            ),
                            0.0,
                            color,
                        );
                    }
                });
        });

        ctx.request_repaint();
    }
}

fn main() {
    let args = Args::parse();

    // We must run the GUI on the main thread for Windows/Cross-platform compatibility.
    // So we'll run use a manual Tokio runtime on a background thread.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let pool_size = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    println!(
        "Initializing Simulation Engine with {} threads...",
        pool_size
    );
    let engine = SimulationEngine::new(space.clone(), pool_size);
    let (tx, _rx) = broadcast::channel::<u64>(100);

    // Register subscriber for real-time broadcasts
    let subscriber = Arc::new(ServerEngineSubscriber {
        engine: engine.clone(),
        tx: tx.clone(),
    });
    engine.add_subscriber(subscriber);

    let gui_state = if args.gui {
        let state = Arc::new(Mutex::new(RustyLifeGuiState {
            viewport_cells: Vec::new(),
            generation: 0,
            is_running: false,
            target_viewport: None,
        }));

        let gui_presenter = Arc::clone(&state) as Arc<Mutex<dyn SimulationPresenter>>;
        engine.add_subscriber(Arc::new(PresenterSubscriber {
            presenter: gui_presenter,
        }));
        Some(state)
    } else {
        None
    };

    // Initial Seeding
    if let Some(pattern) = args.seed {
        engine.seed(pattern);
    }

    let shared_state = Arc::new(AppStateEnv {
        engine: engine.clone(),
        tx: tx.clone(),
    });

    // Spawn the server stack in the background
    let shared_state_clone = shared_state.clone();
    let port = args.port;
    let ipc_port = args.ipc_port;

    rt.spawn(async move {
        let app = Router::new()
            .route("/", get(index))
            .route("/ws", get(ws_handler))
            .with_state(shared_state_clone.clone());

        // IPC/TCP Server for Native Clients
        let ipc_state = shared_state_clone.clone();
        tokio::spawn(async move {
            let listener = TcpListener::bind(format!("127.0.0.1:{}", ipc_port))
                .await
                .unwrap();
            println!("IPC (TCP) Server listening on port {}", ipc_port);

            loop {
                if let Ok((stream, _)) = listener.accept().await {
                    let state = ipc_state.clone();
                    tokio::spawn(handle_ipc(stream, state));
                }
            }
        });

        println!("Web Server running on http://localhost:{}", port);
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
            .await
            .unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    if args.gui {
        let options = eframe::NativeOptions::default();
        let gui_state = gui_state.unwrap();
        eframe::run_native(
            "RustyLife Server",
            options,
            Box::new(|_cc| Ok(Box::new(RustyLifeGui::new(engine, gui_state)))),
        )
        .unwrap();
    } else {
        // Just wait for the background runtime
        println!("Running in headless mode. Press Ctrl+C to stop.");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
}

async fn index() -> impl IntoResponse {
    axum::response::Html(include_str!("../static/index.html"))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppStateEnv>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppStateEnv>) {
    let mut rx = state.tx.subscribe();

    // Send the current generation immediately so the client can sync up
    let current_gen = state.engine.generation();
    let resp = Response::SnapshotAvailable(current_gen);
    let _ = socket.send(Message::Binary(resp.to_bytes().into())).await;

    loop {
        tokio::select! {
            result = socket.recv() => {
                if let Some(Ok(Message::Binary(bin))) = result {
                    if let Ok(req) = Request::from_bytes(&bin) {
                        match req {
                            Request::NextStep => {
                                state.engine.step();
                            }
                            Request::Reset => {
                                state.engine.reset();
                            }
                            Request::GetState { generation, viewport } => {
                                let resp = handle_get_state(&state, generation, viewport).await;
                                let _ = socket.send(Message::Binary(resp.to_bytes().into())).await;
                            }
                            Request::Start => {
                                state.engine.start();
                            }
                            Request::Stop => {
                                state.engine.stop();
                            }
                            Request::Seed(pattern) => {
                                state.engine.seed(pattern);
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            result = rx.recv() => {
                if let Ok(generation) = result {
                    let resp = Response::SnapshotAvailable(generation);
                    if socket.send(Message::Binary(resp.to_bytes().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_ipc(stream: TcpStream, state: Arc<AppStateEnv>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut rx = state.tx.subscribe();

    // Send the current generation immediately so the client can sync up
    let current_gen = state.engine.generation();
    let resp = Response::SnapshotAvailable(current_gen);
    let bytes = resp.to_bytes();
    let _ = writer.write_all(&bytes).await;

    loop {
        let mut tag_buf = [0u8; 1];
        tokio::select! {
            result = reader.read_exact(&mut tag_buf) => {
                if result.is_err() { break; }
                let tag = tag_buf[0];
                let req = match tag {
                    0x01 => Some(Request::NextStep),
                    0x02 => Some(Request::Reset),
                    0x03 => {
                        let mut gen_buf = [0u8; 8];
                        if reader.read_exact(&mut gen_buf).await.is_err() { break; }
                        let generation = u64::from_le_bytes(gen_buf);

                        let mut has_vp_buf = [0u8; 1];
                        if reader.read_exact(&mut has_vp_buf).await.is_err() { break; }

                        let viewport = if has_vp_buf[0] == 1 {
                            let mut vp_buf = [0u8; 64];
                            if reader.read_exact(&mut vp_buf).await.is_err() { break; }
                            let x1 = i128::from_le_bytes(vp_buf[0..16].try_into().unwrap());
                            let y1 = i128::from_le_bytes(vp_buf[16..32].try_into().unwrap());
                            let x2 = i128::from_le_bytes(vp_buf[32..48].try_into().unwrap());
                            let y2 = i128::from_le_bytes(vp_buf[48..64].try_into().unwrap());
                            Some(((x1, y1), (x2, y2)))
                        } else {
                            None
                        };
                        Some(Request::GetState { generation, viewport })
                    }
                    0x04 => Some(Request::Start),
                    0x05 => Some(Request::Stop),
                    0x06 => {
                        let mut len_buf = [0u8; 4];
                        if reader.read_exact(&mut len_buf).await.is_err() { break; }
                        let len = u32::from_le_bytes(len_buf) as usize;
                        let mut payload = vec![0u8; len];
                        if reader.read_exact(&mut payload).await.is_err() { break; }
                        let pattern = String::from_utf8_lossy(&payload).to_string();
                        Some(Request::Seed(pattern))
                    }
                    _ => None,
                };

                if let Some(req) = req {
                    match req {
                        Request::NextStep => { state.engine.step(); }
                        Request::Reset => { state.engine.reset(); }
                        Request::GetState { generation, viewport } => {
                            let resp = handle_get_state(&state, generation, viewport).await;
                            let bytes = resp.to_bytes();
                            let _ = writer.write_all(&bytes).await;
                        }
                        Request::Start => { state.engine.start(); }
                        Request::Stop => { state.engine.stop(); }
                        Request::Seed(pattern) => { state.engine.seed(pattern); }
                    }
                }
            }
            result = rx.recv() => {
                if let Ok(generation) = result {
                    let resp = Response::SnapshotAvailable(generation);
                    let bytes = resp.to_bytes();
                    if writer.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_get_state(
    state: &Arc<AppStateEnv>,
    generation: u64,
    viewport: Option<((i128, i128), (i128, i128))>,
) -> Response {
    let path = state
        .engine
        .staging_dir
        .join(format!("gen_{}.bin", generation));
    if !path.exists() {
        return Response::Error(format!("Snapshot for generation {} not found", generation));
    }

    match std::fs::read(&path) {
        Ok(mut buf) => {
            if viewport.is_none() {
                // OPTIMIZATION: Zero-Copy Path
                // If the client wants the full universe (no viewport), we can skip the expensive
                // Decode -> Filter -> Encode cycle. We just need to patch the 'is_running' byte
                // (offset 16) and recompute the CRC.
                if buf.len() > 16 + 4 {
                    let is_running = !state.engine.is_stopped();
                    buf[16] = if is_running { 1 } else { 0 };

                    // Recalculate CRC for the patched buffer
                    let len = buf.len();
                    let payload = &buf[0..len - 4];
                    let new_crc = crc32fast::hash(payload);
                    let crc_bytes = new_crc.to_le_bytes();

                    // Update CRC at the end
                    buf[len - 4] = crc_bytes[0];
                    buf[len - 3] = crc_bytes[1];
                    buf[len - 2] = crc_bytes[2];
                    buf[len - 1] = crc_bytes[3];
                }
                Response::BinaryState(buf)
            } else {
                // Legacy Path: Viewport Filtering
                // We must decode, filter the cells, and re-encode.
                match rustylife_core::decode_binary_packet(&buf) {
                    Ok(packet) => {
                        let filtered_cells: Vec<_> =
                            if let Some(((min_x, min_y), (max_x, max_y))) = viewport {
                                packet
                                    .cells
                                    .into_iter()
                                    .filter(|((x, y), _)| {
                                        *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y
                                    })
                                    .collect()
                            } else {
                                packet.cells
                            };

                        let is_running = !state.engine.is_stopped();
                        let response_packet = rustylife_core::encode_binary_packet(
                            packet.generation,
                            packet.total_cells,
                            is_running,
                            &filtered_cells,
                        );
                        Response::BinaryState(response_packet)
                    }
                    Err(e) => Response::Error(format!("Failed to decode snapshot: {}", e)),
                }
            }
        }
        Err(e) => Response::Error(format!("Failed to read snapshot file: {}", e)),
    }
}
