//! # RustyLife Client
//!
//! The `rustylife-client` binary is a native visualization tool for the
//! RustyLife simulation. It connects to the server via IPC (TCP) and
//! implements a high-performance rendering loop with 4-state lifecycle tracking.

use eframe::egui;
use rustylife_core::{BinaryPacket, Request, SimulationPresenter};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

struct RustyLifeClientState {
    viewport_cells: Vec<((i128, i128), u8)>,
    generation: u64,
    is_running: bool,
    target_viewport: Option<((i128, i128), (i128, i128))>,
}

impl SimulationPresenter for RustyLifeClientState {
    fn update_state(&mut self, packet: BinaryPacket) {
        self.viewport_cells = packet.cells;
        self.generation = packet.generation;
        self.is_running = packet.is_running;
    }

    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        self.target_viewport
    }
}

struct RustyLifeClientApp {
    state: Arc<Mutex<RustyLifeClientState>>,
    tx: mpsc::Sender<Request>,
    cell_size: f32,
    view_offset: egui::Vec2,
    last_generation: u64,
}

impl RustyLifeClientApp {
    fn new(state: Arc<Mutex<RustyLifeClientState>>, tx: mpsc::Sender<Request>) -> Self {
        Self {
            state,
            tx,
            cell_size: 10.0,
            view_offset: egui::Vec2::ZERO,
            last_generation: u64::MAX,
        }
    }
}

impl eframe::App for RustyLifeClientApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let generation = {
            let s = self.state.lock().unwrap();
            s.generation
        };

        if generation == 0 && self.last_generation != 0 && self.view_offset != egui::Vec2::ZERO {
            self.view_offset = egui::Vec2::ZERO;
        }
        self.last_generation = generation;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("RustyLife Standalone Client (Gen {})", generation));

            let (is_running, cells) = {
                let s = self.state.lock().unwrap();
                (s.is_running, s.viewport_cells.clone())
            };

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!is_running, egui::Button::new("Run"))
                    .clicked()
                {
                    let _ = self.tx.try_send(Request::Start);
                }
                if ui
                    .add_enabled(!is_running, egui::Button::new("Step"))
                    .clicked()
                {
                    let _ = self.tx.try_send(Request::NextStep);
                }
                if ui
                    .add_enabled(is_running, egui::Button::new("Stop"))
                    .clicked()
                {
                    let _ = self.tx.try_send(Request::Stop);
                }
                if ui
                    .add_enabled(!is_running, egui::Button::new("Reset"))
                    .clicked()
                {
                    let _ = self.tx.try_send(Request::Reset);
                }

                ui.separator();

                ui.add_enabled_ui(!is_running, |ui| {
                    egui::ComboBox::from_label("Patterns")
                        .selected_text("Select Pattern...")
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(false, "glider").clicked() {
                                let _ = self.tx.try_send(Request::Seed("glider".to_string()));
                            }
                            if ui.selectable_label(false, "r-pentomino").clicked() {
                                let _ = self.tx.try_send(Request::Seed("r-pentomino".to_string()));
                            }
                            if ui.selectable_label(false, "glider gun").clicked() {
                                let _ = self.tx.try_send(Request::Seed("glider gun".to_string()));
                            }
                            if ui.selectable_label(false, "spaceship").clicked() {
                                let _ = self.tx.try_send(Request::Seed("spaceship".to_string()));
                            }
                            if ui.selectable_label(false, "blinker").clicked() {
                                let _ = self.tx.try_send(Request::Seed("blinker".to_string()));
                            }
                            if ui.selectable_label(false, "block").clicked() {
                                let _ = self.tx.try_send(Request::Seed("block".to_string()));
                            }
                            if ui.selectable_label(false, "beehive").clicked() {
                                let _ = self.tx.try_send(Request::Seed("beehive".to_string()));
                            }
                            if ui.selectable_label(false, "breeder 1").clicked() {
                                let _ = self.tx.try_send(Request::Seed("breeder 1".to_string()));
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

                    // Handle Panning
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

                    // Calculate visible viewport
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client_state = Arc::new(Mutex::new(RustyLifeClientState {
        viewport_cells: Vec::new(),
        generation: 0,
        is_running: false,
        target_viewport: None,
    }));
    let (tx, mut rx) = mpsc::channel::<Request>(10);

    let state_clone = client_state.clone();

    // Background task for TCP communication
    tokio::spawn(async move {
        loop {
            println!("Connecting to server...");
            if let Ok(stream) = TcpStream::connect("127.0.0.1:9001").await {
                println!("Connected!");
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                loop {
                    let mut tag_buf = [0u8; 1];
                    tokio::select! {
                        result = reader.read_exact(&mut tag_buf) => {
                            if result.is_err() { break; }
                            match tag_buf[0] {
                                0x01 => {
                                    let mut gen_buf = [0u8; 8];
                                    if reader.read_exact(&mut gen_buf).await.is_err() { break; }
                                    let generation = u64::from_le_bytes(gen_buf);

                                    let viewport = {
                                        state_clone.lock().unwrap().target_viewport
                                    };
                                    let req = Request::GetState { generation, viewport };
                                    let _ = writer.write_all(&req.to_bytes()).await;
                                }
                                0x02 => {
                                    let mut len_buf = [0u8; 8];
                                    if reader.read_exact(&mut len_buf).await.is_err() { break; }
                                    let len = u64::from_le_bytes(len_buf) as usize;

                                    let mut payload = vec![0u8; len];
                                    if reader.read_exact(&mut payload).await.is_err() { break; }

                                    if let Ok(packet) = rustylife_core::decode_binary_packet(&payload) {
                                        let mut s = state_clone.lock().unwrap();
                                        s.update_state(packet);
                                    }
                                }
                                _ => {}
                            }
                        }
                        Some(req) = rx.recv() => {
                            if writer.write_all(&req.to_bytes()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
            println!("Disconnected, retrying in 2s...");
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "RustyLife Client",
        options,
        Box::new(|_cc| Ok(Box::new(RustyLifeClientApp::new(client_state, tx)))),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
