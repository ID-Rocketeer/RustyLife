use crate::state::AppState;
use crate::utils::{fmt_coord, fmt_num, format_si};
use crate::UserActionHandler;
use egui::{Color32, Vec2};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::broadcast;

pub struct RustyLifeApp {
    state: Arc<Mutex<AppState>>,
    handler: Box<dyn UserActionHandler>,
    cell_size: f32,
    view_offset: egui::Vec2,
    last_generation: u64,

    // Viewport Sync
    last_requested_viewport: Option<((i128, i128), (i128, i128))>,
    last_request_time: Instant,
    last_update_time: Option<Instant>,

    // Shutdown
    shutdown_rx: Option<broadcast::Receiver<()>>,

    // Styling
    first_frame: bool,
}

impl RustyLifeApp {
    pub fn new(
        state: Arc<Mutex<AppState>>,
        handler: Box<dyn UserActionHandler>,
        shutdown_rx: Option<broadcast::Receiver<()>>,
    ) -> Self {
        Self {
            state,
            handler,
            cell_size: 4.0,
            view_offset: egui::Vec2::ZERO,
            last_generation: u64::MAX,
            last_requested_viewport: None,
            last_request_time: Instant::now(),
            last_update_time: None,
            shutdown_rx,
            first_frame: true,
        }
    }
}

impl eframe::App for RustyLifeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_frame {
            crate::style::configure_style(ctx);

            // Custom Font Loading: Try to load Consolas on Windows for consistent value formatting
            let mut fonts = egui::FontDefinitions::default();

            // Attempt to load system font
            let font_path = "C:\\Windows\\Fonts\\consola.ttf";
            if let Ok(font_data) = std::fs::read(font_path) {
                fonts
                    .font_data
                    .insert("Consolas".to_owned(), egui::FontData::from_owned(font_data));

                // Set Consolas as the highest priority for Monospace
                if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    family.insert(0, "Consolas".to_owned());
                }

                // Also tweak the proportional font to be consistent if needed, but default is usually okay.
                // Web UI uses Sans-Serif (likely Segoe UI on Windows).
                // Let's stick to fixing Monospace first as requested ("dotted zeros").

                ctx.set_fonts(fonts);
            } else {
                eprintln!("Failed to load Consolas font from {}", font_path);
            }

            // Configure text styles *after* setting fonts
            let mut style = (*ctx.style()).clone();
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                egui::FontId::new(12.0, egui::FontFamily::Monospace),
            );
            ctx.set_style(style);
            self.first_frame = false;
        }

        // Check for shutdown signal
        if let Some(rx) = &mut self.shutdown_rx {
            if let Ok(_) = rx.try_recv() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }

        let (
            cells,
            generation,
            population,
            gps,
            work_rate,
            net_rate,
            is_running,
            is_connected,
            cores,
            bounds,
            expanse,
        ) = {
            let s = self.state.lock().unwrap();
            (
                s.viewport_cells.clone(),
                s.generation,
                s.population,
                s.gps,
                s.work_rate,
                s.net_rate,
                s.is_running,
                s.is_connected,
                s.cores,
                s.bounds,
                s.expanse(),
            )
        };

        if generation == 0 && self.last_generation != 0 && self.view_offset != egui::Vec2::ZERO {
            self.view_offset = egui::Vec2::ZERO;
            self.last_update_time = None;
        }
        self.last_generation = generation;
        self.last_update_time = Some(Instant::now());

        // --- Header ---
        egui::TopBottomPanel::top("header")
            .min_height(48.0)
            .show(ctx, |ui| {
                ui.columns(3, |columns| {
                    // --- COLUMN 1: LEFT (Title & Generation) ---
                    columns[0].vertical(|ui| {
                        ui.set_height(48.0);
                        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.style_mut().spacing.item_spacing.x = 0.0;
                                let text = "RustyLife";
                                let start = crate::style::COLOR_TITLE_START;
                                let end = crate::style::COLOR_TITLE_END;
                                for (i, c) in text.chars().enumerate() {
                                    let t = i as f32 / (text.len() as f32 - 1.0);
                                    let r = (start.r() as f32
                                        + (end.r() as f32 - start.r() as f32) * t)
                                        as u8;
                                    let g = (start.g() as f32
                                        + (end.g() as f32 - start.g() as f32) * t)
                                        as u8;
                                    let b = (start.b() as f32
                                        + (end.b() as f32 - start.b() as f32) * t)
                                        as u8;
                                    ui.label(
                                        egui::RichText::new(c.to_string())
                                            .size(22.0)
                                            .strong()
                                            .color(egui::Color32::from_rgb(r, g, b)),
                                    );
                                }
                            });
                            ui.label(
                                egui::RichText::new(format!("Gen: {}", generation))
                                    .size(13.0)
                                    .strong()
                                    .monospace()
                                    .color(ui.visuals().weak_text_color()),
                            );
                        });
                    });

                    // --- COLUMN 2: CENTER (Simulation Controls) ---
                    columns[1].vertical_centered(|ui| {
                        ui.set_height(48.0);
                        ui.add_space(7.0);
                        ui.horizontal_centered(|ui| {
                            let btn_h = 34.0;
                            ui.spacing_mut().item_spacing.x = 8.0;

                            // Media Controls (Consistently bright)
                            let play_icon = if is_running { "⏸" } else { "▶" };
                            if ui
                                .add(
                                    MediaButton::new(play_icon, true, Color32::WHITE)
                                        .with_size(Vec2::new(34.0, btn_h)),
                                )
                                .on_hover_text("Play / Pause")
                                .clicked()
                            {
                                if is_running {
                                    self.handler.stop();
                                } else {
                                    self.handler.start();
                                }
                            }

                            if ui
                                .add_enabled(
                                    !is_running,
                                    MediaButton::new("⏯", true, Color32::WHITE)
                                        .with_size(Vec2::new(34.0, btn_h)),
                                )
                                .on_hover_text("Step Generation")
                                .clicked()
                            {
                                self.handler.step();
                            }

                            if ui
                                .add_enabled(
                                    !is_running,
                                    MediaButton::new("⏮", true, Color32::WHITE)
                                        .with_size(Vec2::new(34.0, btn_h)),
                                )
                                .on_hover_text("Reset Simulation")
                                .clicked()
                            {
                                self.handler.reset();
                            }

                            ui.add_space(4.0);

                            // Navigation Controls
                            let nav_style = |text: &str| {
                                egui::Button::new(egui::RichText::new(text).size(15.0).strong())
                                    .min_size(Vec2::new(0.0, btn_h))
                            };

                            if ui.add_sized([54.0, btn_h], nav_style("Origin")).clicked() {
                                self.view_offset = Vec2::ZERO;
                            }

                            ui.menu_button(
                                egui::RichText::new("Patterns").size(15.0).strong(),
                                |ui| {
                                    ui.set_min_width(120.0);
                                    let patterns = self.state.lock().unwrap().patterns.clone();
                                    for p in patterns {
                                        if ui
                                            .button(p.name.clone())
                                            .on_hover_text(p.description)
                                            .clicked()
                                        {
                                            self.handler.seed(p.name);
                                            ui.close_menu();
                                        }
                                    }
                                },
                            )
                            .response
                            .on_hover_text("Load a pattern");

                            if ui.add_sized([40.0, btn_h], nav_style("+")).clicked() {
                                let old = self.cell_size;
                                self.cell_size = (self.cell_size + 1.0).min(32.0);
                                self.view_offset *= self.cell_size / old;
                            }
                            if ui.add_sized([40.0, btn_h], nav_style("-")).clicked() {
                                let old = self.cell_size;
                                self.cell_size = (self.cell_size - 1.0).max(1.0);
                                self.view_offset *= self.cell_size / old;
                            }
                        });
                    });

                    // --- COLUMN 3: RIGHT (Metrics & Exit) ---
                    columns[2].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_height(48.0);
                            ui.add_space(10.0);

                            // Quit Button
                            if ui
                                .add_sized(
                                    [64.0, 34.0],
                                    egui::Button::new(
                                        egui::RichText::new("Quit")
                                            .size(15.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(180, 0, 0)),
                                )
                                .clicked()
                            {
                                self.handler.shutdown();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }

                            ui.add_space(15.0);

                            // Population Stats
                            ui.vertical(|ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(
                                        egui::RichText::new("Population")
                                            .size(10.0)
                                            .strong()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                    ui.label(
                                        egui::RichText::new(fmt_num(population as i64, 0, false))
                                            .size(16.0)
                                            .strong()
                                            .monospace(),
                                    );
                                });
                            });
                        },
                    );
                });
                ui.add_space(4.0);
            });

        // Footer
        egui::TopBottomPanel::bottom("footer")
            .frame(
                egui::Frame::side_top_panel(ctx.style().as_ref())
                    .fill(crate::style::COLOR_BG)
                    .stroke(egui::Stroke::new(0.0, egui::Color32::TRANSPARENT)) // Explicitly transparent
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0)),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                // Footer Layout: Two Rows (Stack) to match Web UI
                // Row 1: Metrics (Scrollable)
                // Row 2: Connection Status (Right Aligned)
                ui.vertical(|ui| {
                    // Define Styles
                    let connected_color = egui::Color32::from_hex("#10b981").unwrap();
                    let disconnected_color = egui::Color32::from_hex("#ef4444").unwrap();

                    let (status_text, color, text_color, glow_color) = if is_connected {
                        (
                            crate::text::STATUS_CONNECTED,
                            connected_color,
                            crate::style::COLOR_TEXT, // Match body text color (White)
                            Some(connected_color.linear_multiply(0.3)),
                        )
                    } else {
                        (
                            crate::text::STATUS_DISCONNECTED,
                            disconnected_color,
                            crate::style::COLOR_TEXT, // Match body text color (White)
                            None,
                        )
                    };

                    // --- Row 1: Metrics (Scrollable) ---
                    ui.horizontal(|ui| {
                        egui::ScrollArea::horizontal()
                            .hscroll(true)
                            .vscroll(false)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.style_mut().spacing.item_spacing.x = 2.0;

                                    // Zoom
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("ZOOM:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_ZOOM);
                                    ui.monospace(
                                        egui::RichText::new(format!(
                                            "[ {:0>5.2}X ]",
                                            self.cell_size
                                        ))
                                        .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // Extent
                                    let viewport_rect = ctx.input(|i| i.screen_rect);
                                    let width_cells =
                                        (viewport_rect.width() / self.cell_size) as i64;
                                    let height_cells =
                                        (viewport_rect.height() / self.cell_size) as i64;
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("EXTENT:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_EXTENT);
                                    ui.monospace(
                                        egui::RichText::new(format!(
                                            "[ {} \u{00D7} {} ]",
                                            fmt_num(width_cells, 5, false),
                                            fmt_num(height_cells, 5, false)
                                        ))
                                        .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // Center
                                    // Use last_offset to update Center with actual view?
                                    // Center is defined as view_offset relative to (0,0)?
                                    let center_x = (-self.view_offset.x / self.cell_size) as i64;
                                    let center_y = (self.view_offset.y / self.cell_size) as i64;
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("CENTER:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_CENTER);
                                    ui.monospace(
                                        egui::RichText::new(format!(
                                            "[ {} , {} ]",
                                            fmt_num(center_x, 9, true),
                                            fmt_num(center_y, 9, true)
                                        ))
                                        .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // Work
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("WORK:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_WORK);
                                    ui.monospace(
                                        egui::RichText::new(format_si(work_rate, 3, false))
                                            .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // Net
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("NET:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_NET);
                                    ui.monospace(
                                        egui::RichText::new(format_si(net_rate, 3, true))
                                            .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // GPS
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("GPS:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text(crate::text::TOOLTIP_GPS);
                                    ui.monospace(
                                        egui::RichText::new(format_si(gps, 3, false))
                                            .color(crate::style::COLOR_DATA),
                                    );
                                    ui.add_space(20.0);

                                    // Cores
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("CORES:")
                                                .color(crate::style::COLOR_LABEL)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong()
                                                .underline(),
                                        )
                                        .sense(egui::Sense::hover()),
                                    )
                                    .on_hover_text("Number of worker threads");
                                    ui.monospace(
                                        egui::RichText::new(format!("[ {:0>2} ]", cores))
                                            .color(crate::style::COLOR_DATA),
                                    );
                                });
                            });
                    });

                    ui.add_space(4.0); // Spacing between rows

                    // --- Row 2: Bounds, Expanse, Status ---
                    ui.horizontal(|ui| {
                        ui.style_mut().spacing.item_spacing.x = 0.0; // Tighten label-to-data spacing

                        // Bounds (always display - already in Cartesian from server)
                        let ((bx1, by1), (bx2, by2)) = bounds.unwrap_or(((0, 0), (0, 0)));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("BOUNDS:")
                                    .color(crate::style::COLOR_LABEL)
                                    .font(egui::FontId::proportional(11.0))
                                    .strong()
                                    .underline(),
                            )
                            .sense(egui::Sense::hover()),
                        )
                        .on_hover_text("Bounding box of all living cells");
                        ui.monospace(
                            egui::RichText::new(format!(
                                "[ ({}, {}) -> ({}, {}) ]",
                                fmt_coord(bx1, 9, true),
                                fmt_coord(by1, 9, true),
                                fmt_coord(bx2, 9, true),
                                fmt_coord(by2, 9, true)
                            ))
                            .color(crate::style::COLOR_DATA),
                        );
                        ui.add_space(20.0);

                        // Expanse (always display)
                        let (exp_w, exp_h) = expanse;
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("EXPANSE:")
                                    .color(crate::style::COLOR_LABEL)
                                    .font(egui::FontId::proportional(11.0))
                                    .strong()
                                    .underline(),
                            )
                            .sense(egui::Sense::hover()),
                        )
                        .on_hover_text("Width x Height of the bounding box");
                        ui.monospace(
                            egui::RichText::new(format!(
                                "[ {} \u{00D7} {} ]",
                                fmt_num(exp_w as i64, 9, false),
                                fmt_num(exp_h as i64, 9, false)
                            ))
                            .color(crate::style::COLOR_DATA),
                        );
                        ui.add_space(20.0);

                        // Status (Right Aligned)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.style_mut().spacing.item_spacing.x = 0.0; // Remove default spacing for precise control
                            ui.add_space(5.0); // Right padding
                            ui.label(
                                egui::RichText::new(status_text)
                                    .color(text_color)
                                    .size(16.0), // Increased to 16.0 (Default Body Size)
                            );
                            ui.add_space(4.0); // Exact match for 0.25rem (4px) from CSS

                            let (dot_rect, _) =
                                ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover()); // Exact 6px size
                            if let Some(glow) = glow_color {
                                ui.painter().circle_filled(dot_rect.center(), 6.0, glow);
                            }
                            ui.painter().circle_filled(dot_rect.center(), 3.0, color);
                        });
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::canvas(ui.style())
                .fill(crate::style::COLOR_BG) // Universe background
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

                    // Update Viewport Target in State (Optimistic)
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

                    // Viewport Synchronization (Debounced)
                    // Request data from handler
                    let current_viewport = ((min_x, min_y), (max_x, max_y));
                    let now = Instant::now();
                    let time_since_last = now.duration_since(self.last_request_time).as_millis();
                    let changed = self.last_requested_viewport != Some(current_viewport);

                    if changed && time_since_last > 100 {
                        self.handler
                            .request_state(generation, Some(current_viewport));
                        self.last_requested_viewport = Some(current_viewport);
                        self.last_request_time = now;
                    }
                });
        });

        ctx.request_repaint();
    }
}

struct MediaButton {
    icon: String,
    enabled: bool,
    color: Color32,
    size: Option<Vec2>,
}

impl MediaButton {
    pub fn new(icon: impl Into<String>, enabled: bool, color: Color32) -> Self {
        Self {
            icon: icon.into(),
            enabled,
            color,
            size: None,
        }
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = Some(size);
        self
    }
}

impl egui::Widget for MediaButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = self.size.unwrap_or(Vec2::splat(18.0));
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            let painter = ui.painter();

            // Always draw background frame to match standard buttons
            painter.rect_filled(
                rect.expand(visuals.expansion),
                visuals.rounding,
                visuals.bg_fill,
            );
            if response.hovered() || response.clicked() {
                painter.rect_stroke(
                    rect.expand(visuals.expansion),
                    visuals.rounding,
                    visuals.fg_stroke,
                );
            }

            let paint_rect = rect.shrink(8.0); // Balanced padding
            let icon_color = if self.enabled {
                self.color
            } else {
                ui.visuals().extreme_bg_color
            };

            use egui::{Rect, Shape, Stroke};
            match self.icon.as_str() {
                "⏸" => {
                    let w = paint_rect.width();
                    let h = paint_rect.height();
                    let bar_w = w * 0.35;
                    let gap = w * 0.3;
                    painter.rect_filled(
                        Rect::from_min_size(paint_rect.min, Vec2::new(bar_w, h)),
                        0.0,
                        icon_color,
                    );
                    painter.rect_filled(
                        Rect::from_min_size(
                            paint_rect.min + Vec2::new(bar_w + gap, 0.0),
                            Vec2::new(bar_w, h),
                        ),
                        0.0,
                        icon_color,
                    );
                }
                "⏯" => {
                    let w = paint_rect.width();
                    let h = paint_rect.height();
                    let tri_w = w * 0.55;
                    let bar_w = w * 0.15;
                    let gap = w * 0.15;

                    painter.add(Shape::convex_polygon(
                        vec![
                            paint_rect.min,
                            paint_rect.min + Vec2::new(tri_w, h / 2.0),
                            paint_rect.min + Vec2::new(0.0, h),
                        ],
                        icon_color,
                        Stroke::NONE,
                    ));

                    let bar_start = tri_w + gap;
                    painter.rect_filled(
                        Rect::from_min_size(
                            paint_rect.min + Vec2::new(bar_start, 0.0),
                            Vec2::new(bar_w, h),
                        ),
                        0.0,
                        icon_color,
                    );
                    painter.rect_filled(
                        Rect::from_min_size(
                            paint_rect.min + Vec2::new(bar_start + bar_w + gap, 0.0),
                            Vec2::new(bar_w, h),
                        ),
                        0.0,
                        icon_color,
                    );
                }
                "⏮" => {
                    let w = paint_rect.width();
                    let h = paint_rect.height();
                    let bar_w = w * 0.15;
                    let tri_w = w * 0.4;
                    let gap = w * 0.1;

                    painter.rect_filled(
                        Rect::from_min_size(paint_rect.min, Vec2::new(bar_w, h)),
                        0.0,
                        icon_color,
                    );

                    let tri1_start = bar_w + gap;
                    painter.add(Shape::convex_polygon(
                        vec![
                            paint_rect.min + Vec2::new(tri1_start, h / 2.0),
                            paint_rect.min + Vec2::new(tri1_start + tri_w, 0.0),
                            paint_rect.min + Vec2::new(tri1_start + tri_w, h),
                        ],
                        icon_color,
                        Stroke::NONE,
                    ));

                    let tri2_start = tri1_start + tri_w + gap;
                    painter.add(Shape::convex_polygon(
                        vec![
                            paint_rect.min + Vec2::new(tri2_start, h / 2.0),
                            paint_rect.min + Vec2::new(tri2_start + tri_w, 0.0),
                            paint_rect.min + Vec2::new(tri2_start + tri_w, h),
                        ],
                        icon_color,
                        Stroke::NONE,
                    ));
                }
                _ => {
                    painter.text(
                        paint_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &self.icon,
                        egui::FontId::proportional(14.0),
                        icon_color,
                    );
                }
            }
        }
        response
    }
}
