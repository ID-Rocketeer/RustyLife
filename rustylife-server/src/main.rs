// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

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
    Request, Response, SimulationPresenter,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use rustylife_gui::{AppState, RustyLifeApp, UserActionHandler};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

#[derive(Parser, Debug)]
#[command(author, version, about = "RustyLife Server - High-performance Game of Life engine", long_about = None)]
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

    /// Initial seed pattern (glider, blinker, breeder 1)
    #[arg(short, long)]
    pub seed: Option<String>,

    /// Number of generations to run before exiting (for profiling)
    #[arg(long)]
    pub generations: Option<u64>,

    /// Automatically start the simulation after seeding
    #[arg(long)]
    pub autostart: bool,

    /// Prevent Windows from suspending the process when locked
    #[arg(long)]
    pub stay_awake: bool,

    /// Enable periodic status logging to the console
    #[arg(short, long)]
    pub log: bool,

    /// Frequency of periodic status logging in minutes (default 20)
    #[arg(long, default_value_t = 20.0)]
    pub log_interval: f64,
}

/// Broadcasts result when a snapshot is ready.
pub struct ServerEngineSubscriber {
    pub engine: Arc<SimulationEngine>,
    #[allow(clippy::type_complexity)]
    pub tx: broadcast::Sender<(Arc<Vec<((i128, i128), u8)>>, rustylife_core::Telemetry)>,
}

impl EngineSubscriber for ServerEngineSubscriber {
    fn on_snapshot_available(
        &self,
        data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let _ = self.tx.send((data, telemetry));
        true
    }
}

/// A subscriber that exits the process after reaching a target generation.
pub struct BenchmarkSubscriber {
    pub target_generation: u64,
}

impl EngineSubscriber for BenchmarkSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        if telemetry.generation >= self.target_generation {
            println!(
                "Reached target generation {}. Exiting...",
                telemetry.generation
            );
            std::process::exit(0);
        }
        true
    }
}

/// 2. Presenter Subscriber: Forwards simulation updates to a presenter (e.g. Server GUI or IPC)
struct PresenterSubscriber {
    presenter: Arc<Mutex<dyn SimulationPresenter>>,
}

impl EngineSubscriber for PresenterSubscriber {
    fn on_snapshot_available(
        &self,
        data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let packet_data =
            rustylife_core::encode_binary_packet(telemetry.generation, &data, telemetry);
        if let Ok(packet) = rustylife_core::decode_binary_packet(&packet_data) {
            let mut presenter = self.presenter.lock().unwrap();
            presenter.update_state(packet, telemetry);
        }
        true
    }
}

/// A subscriber that logs status updates to the console periodically.
pub struct LoggingSubscriber {
    last_log: Mutex<Option<std::time::Instant>>,
    last_snapshot_time: Mutex<Option<std::time::Instant>>,
    min_interval: Mutex<std::time::Duration>,
    max_interval: Mutex<std::time::Duration>,
    log_interval: std::time::Duration,
}

impl LoggingSubscriber {
    pub fn new(interval_mins: f64) -> Self {
        Self {
            last_log: Mutex::new(None),
            last_snapshot_time: Mutex::new(None),
            min_interval: Mutex::new(std::time::Duration::MAX),
            max_interval: Mutex::new(std::time::Duration::ZERO),
            log_interval: std::time::Duration::from_secs_f64(interval_mins * 60.0),
        }
    }
}

impl Default for LoggingSubscriber {
    fn default() -> Self {
        Self::new(20.0)
    }
}

impl EngineSubscriber for LoggingSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let now = std::time::Instant::now();

        // 1. Update Inter-generation Interval tracking
        if telemetry.is_running {
            let mut last_snap = self.last_snapshot_time.lock().unwrap();
            if let Some(prev) = *last_snap {
                let interval = now.duration_since(prev);

                let mut min = self.min_interval.lock().unwrap();
                let mut max = self.max_interval.lock().unwrap();
                *min = (*min).min(interval);
                *max = (*max).max(interval);
            }
            *last_snap = Some(now);
        } else {
            // Engine is idle. Reset interval tracking to avoid measuring the "stopped" period.
            *self.last_snapshot_time.lock().unwrap() = None;
        }

        // 2. Check for Periodic Logging (using the provided interval)
        let mut last_log = self.last_log.lock().unwrap();
        let should_log = match *last_log {
            None => true,
            Some(last) => now.duration_since(last) >= self.log_interval,
        };

        if should_log {
            let timestamp = chrono::Utc::now().format("[%H:%M:%S UTC]");
            let bounds_str = if let Some(((min_x, min_y), (max_x, max_y))) = telemetry.bounds {
                format!("({}, {}) to ({}, {})", min_x, min_y, max_x, max_y)
            } else {
                "None".to_string()
            };

            let min_max_str = {
                let min = self.min_interval.lock().unwrap();
                let max = self.max_interval.lock().unwrap();
                if *min == std::time::Duration::MAX {
                    "Min/Max: N/A".to_string()
                } else {
                    format!(
                        "Min/Max: {:.2} ms / {:.2} ms",
                        min.as_secs_f64() * 1000.0,
                        max.as_secs_f64() * 1000.0
                    )
                }
            };

            println!(
                "{} Gen: {}, Pop: {}, GPS: {}, Work Rate: {}, {}, Bounds: {}",
                timestamp,
                format_with_commas(telemetry.generation),
                format_with_commas(telemetry.population),
                format_si_rate(telemetry.gps),
                format_si_rate(telemetry.work_rate),
                min_max_str,
                bounds_str
            );

            // Reset tracking for next window
            *last_log = Some(now);
            *self.min_interval.lock().unwrap() = std::time::Duration::MAX;
            *self.max_interval.lock().unwrap() = std::time::Duration::ZERO;
            // Note: we do NOT reset last_snapshot_time here, as the user specified:
            // "When we log a message we do want to reset the min/max as previously described, but there should be no issue with the subsequent interval since we'll have a valid timestamp from the last update."
        }
        true
    }
}

struct AppStateEnv {
    engine: Arc<SimulationEngine>,
    #[allow(clippy::type_complexity)]
    tx: broadcast::Sender<(Arc<Vec<((i128, i128), u8)>>, rustylife_core::Telemetry)>,
    shutdown_tx: broadcast::Sender<()>,
    cores: usize,
}

#[derive(Clone, Debug)]
enum ClientType {
    MetricsOnly,
    FullSnapshot {
        viewport: Option<((i128, i128), (i128, i128))>,
    },
}

struct ServerActionHandler {
    engine: Arc<SimulationEngine>,
    state: Arc<Mutex<AppState>>,
}

impl UserActionHandler for ServerActionHandler {
    fn start(&mut self) {
        self.engine.start();
    }
    fn stop(&mut self) {
        self.engine.stop();
    }
    fn step(&mut self) {
        self.engine.step();
    }
    fn reset(&mut self) {
        self.engine.reset();
    }
    fn seed(&mut self, pattern: String) {
        self.engine.seed(pattern);
    }
    fn request_state(&mut self, _gen: u64, viewport: Option<((i128, i128), (i128, i128))>) {
        // Always sync running state — is_running must not be gated on viewport availability
        // because egui may not call this with a viewport if the view hasn't changed.
        {
            let mut s = self.state.lock().unwrap();
            s.is_running = !self.engine.is_stopped();
        }

        // Expensive viewport cell fetch only when caller provides a viewport
        if let Some(viewport) = viewport {
            let cells = self.engine.get_cells_in_rect(viewport.0, viewport.1);
            let mut s = self.state.lock().unwrap();
            s.viewport_cells = cells;
            s.generation = self.engine.generation();
            s.population = self
                .engine
                .living_count
                .load(std::sync::atomic::Ordering::Relaxed);
        }
    }
    fn shutdown(&mut self) {
        self.engine.stop();
        // Trigger system exit
        std::process::exit(0);
    }
}

static ENGINE_REF: OnceLock<Weak<SimulationEngine>> = OnceLock::new();

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn format_si_rate(val: f64) -> String {
    let units = ["", "K", "M", "G", "T"];
    let mut v = val.abs();
    let mut u = 0;
    while v >= 999.995 && u < units.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if units[u].is_empty() {
        format!("{:.2} /s", v)
    } else {
        format!("{:.2} {}/s", v, units[u])
    }
}

struct OomTelemetryAllocator;

unsafe impl GlobalAlloc for OomTelemetryAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if ptr.is_null() {
            oom_crash_report(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if ptr.is_null() {
            oom_crash_report(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if new_ptr.is_null() {
            oom_crash_report(new_size);
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: OomTelemetryAllocator = OomTelemetryAllocator;

fn oom_crash_report(size: usize) {
    // Attempt to print the telemetry securely without triggering further panics.
    // We avoid large standard formatting macros because we are out of memory.
    eprintln!("\n==================================================");
    eprintln!("        RUSTYLIFE OUT OF MEMORY CRASH             ");
    eprintln!("==================================================");
    eprintln!("FATAL: Failed to allocate {} bytes.", size);

    // We attempt to get the engine and print its state.
    // By invoking `print_crash_telemetry`, we rely entirely on `eprintln!` which avoids
    // dynamic heap allocations like `format!()` or `String::new()`. This is critical
    // because any heap allocation during an OOM handler could deadlock or crash immediately.
    if let Some(engine_weak) = ENGINE_REF.get()
        && let Some(engine) = engine_weak.upgrade()
    {
        let _ = write_crash_telemetry(&mut std::io::stderr(), Some(&engine));
    }
    eprintln!("==================================================\n");
    std::process::abort();
}

pub fn write_crash_telemetry(
    mut out: impl std::io::Write,
    engine: Option<&SimulationEngine>,
) -> std::io::Result<()> {
    writeln!(out, "\n==================================================")?;
    writeln!(out, "              RUSTYLIFE CRASH REPORT              ")?;
    writeln!(out, "==================================================")?;
    writeln!(out, "{}", chrono::Utc::now().format("[%H:%M:%S UTC]"))?;
    writeln!(out)?;

    if let Some(engine) = engine {
        writeln!(out, "Simulation State at Crash:")?;
        writeln!(
            out,
            "  Generation: {}",
            format_with_commas(engine.generation())
        )?;
        writeln!(
            out,
            "  Population: {}",
            format_with_commas(engine.living_count.load(Ordering::Relaxed))
        )?;

        write!(out, "  Bounds:     ")?;
        if let Ok(bounds_lock) = engine.current_generation_bounds.try_lock() {
            if let Some(((min_x, min_y), (max_x, max_y))) = *bounds_lock {
                writeln!(out, "({}, {}) to ({}, {})", min_x, min_y, max_x, max_y)?;
            } else {
                writeln!(out, "None")?;
            }
        } else {
            writeln!(out, "Locked")?;
        }

        if let Ok(tel) = engine.telemetry.try_lock() {
            writeln!(out, "  GPS:        {}", format_si_rate(tel.gps))?;
            writeln!(out, "  Work Rate:  {}", format_si_rate(tel.work_rate_ema))?;
            writeln!(out, "  Net Rate:   {}", format_si_rate(tel.net_rate_ema))?;
        } else {
            writeln!(out, "  GPS:        Locked")?;
            writeln!(out, "  Work Rate:  Locked")?;
            writeln!(out, "  Net Rate:   Locked")?;
        }
    } else {
        writeln!(
            out,
            "Simulation State at Crash: UNKNOWN (Engine not running or inaccessible)"
        )?;
    }
    Ok(())
}

/// Prevents the system from entering sleep mode while the simulation is running.
/// Only effective on Windows.
#[cfg(windows)]
fn prevent_sleep(enable: bool) {
    if !enable {
        return;
    }

    // Windows API Constants
    const ES_CONTINUOUS: u32 = 0x80000000;
    const ES_SYSTEM_REQUIRED: u32 = 0x00000001;

    unsafe extern "system" {
        fn SetThreadExecutionState(esFlags: u32) -> u32;
    }

    unsafe {
        // Set the state to continuous + system required
        // This tells Windows "don't sleep the CPU, but you can turn off the monitor".
        let res = SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        if res == 0 {
            eprintln!("Warning: Failed to set thread execution state (Windows sleep prevention).");
        } else {
            println!("Windows sleep prevention enabled.");
        }
    }
}

fn setup_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let engine = ENGINE_REF.get().and_then(|w| w.upgrade());
        let _ = write_crash_telemetry(&mut std::io::stderr(), engine.as_deref());
        eprintln!("--------------------------------------------------");
        eprintln!("Panic Details:");
        default_hook(panic_info);
        eprintln!("==================================================\n");
    }));
}

fn main() {
    let args = rustylife_core::cli::init_cli::<Args>();

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
        "Initializing Simulation Engine with {} workers (+1 I/O thread)...",
        pool_size
    );
    let engine = SimulationEngine::new(space.clone(), pool_size);

    // Set up global crash reporting
    let _ = ENGINE_REF.set(Arc::downgrade(&engine));
    setup_panic_hook();

    // Windows sleep prevention
    #[cfg(windows)]
    prevent_sleep(args.stay_awake);

    // Load dynamic patterns from "patterns/" directory
    println!("Loading patterns from ./patterns directory...");
    load_dynamic_patterns(&engine);

    let (tx, _rx) =
        broadcast::channel::<(Arc<Vec<((i128, i128), u8)>>, rustylife_core::Telemetry)>(100);

    // Register subscriber for real-time broadcasts
    let subscriber = Arc::new(ServerEngineSubscriber {
        engine: engine.clone(),
        tx: tx.clone(),
    });
    engine.add_subscriber(subscriber);

    if args.log {
        engine.add_subscriber(Arc::new(LoggingSubscriber::new(args.log_interval)));
    }

    let gui_state = if args.gui {
        let state = rustylife_gui::AppState {
            cores: pool_size,
            is_connected: true, // Native GUI is always "connected" to the internal engine
            patterns: engine.get_catalog(),
            ..Default::default()
        };
        let state = Arc::new(Mutex::new(state));

        let gui_presenter = Arc::clone(&state) as Arc<Mutex<dyn SimulationPresenter>>;
        engine.add_subscriber(Arc::new(PresenterSubscriber {
            presenter: gui_presenter,
        }));
        Some(state)
    } else {
        None
    };

    // Initial Seeding and Benchmarking
    if let Some(target) = args.generations {
        println!("Profiling mode: Running for {} generations.", target);
        engine.add_subscriber(Arc::new(BenchmarkSubscriber {
            target_generation: target,
        }));
    }

    if let Some(pattern) = args.seed {
        if args.generations.is_some() || args.autostart {
            engine.seed_and_start(pattern, args.generations);
        } else {
            engine.seed(pattern);
        }
    } else if args.autostart {
        // No seed provided, just start (e.g. continuing or default state)
        if let Some(target) = args.generations {
            engine.set_target_generation(target);
        }
        engine.start();
    }

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

    let shared_state = Arc::new(AppStateEnv {
        engine: engine.clone(),
        tx: tx.clone(),
        shutdown_tx: shutdown_tx.clone(),
        cores: pool_size,
    });

    // Handle Ctrl-C to trigger clean shutdown
    let shutdown_tx_clone = shutdown_tx.clone();
    rt.spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            println!("\r\nCtrl-C received. Initiating shutdown...");
            let _ = shutdown_tx_clone.send(());
        }
    });

    let mut hard_shutdown_rx = shutdown_tx.subscribe();
    rt.spawn(async move {
        let _ = hard_shutdown_rx.recv().await;
        println!("\nShutdown signal received. Forcing process exit.");
        std::process::exit(0);
    });

    // Spawn the server stack in the background
    let shared_state_clone = shared_state.clone();
    let port = args.port;
    let ipc_port = args.ipc_port;

    rt.spawn(async move {
        let app = Router::new()
            .route("/", get(index))
            .route("/dashboard.js", get(dashboard_js))
            .route("/utils.js", get(utils_js))
            .route("/protocol.js", get(protocol_js))
            .route("/ws", get(ws_handler))
            .with_state(shared_state_clone.clone());

        // Telemetry Server for Realtime Graphing (Port 8086)
        let telemetry_state = shared_state_clone.clone();
        tokio::spawn(async move {
            let telemetry_app = Router::new()
                .route("/", get(telemetry_html))
                .route("/telemetry.js", get(telemetry_js))
                .route("/protocol.js", get(protocol_js))
                .route("/ws", get(ws_handler))
                .with_state(telemetry_state);

            let listener = tokio::net::TcpListener::bind("0.0.0.0:8086").await.unwrap();
            println!("Telemetry Server running on http://localhost:8086");
            axum::serve(listener, telemetry_app).await.unwrap();
        });

        // IPC/TCP Server for Native Clients
        let ipc_state = shared_state_clone.clone();
        tokio::spawn(async move {
            let listener = TcpListener::bind(format!("0.0.0.0:{}", ipc_port))
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
        // Subscribe to shutdown channel for GUI
        let gui_shutdown_rx = shutdown_tx.subscribe();

        // Create Handler
        let handler = Box::new(ServerActionHandler {
            engine: engine.clone(),
            state: gui_state.clone(),
        });

        eframe::run_native(
            "RustyLife",
            options,
            Box::new(|_cc| {
                Ok(Box::new(RustyLifeApp::new(
                    gui_state,
                    handler,
                    Some(gui_shutdown_rx),
                )))
            }),
        )
        .unwrap();
    } else {
        // Just wait for the background runtime
        println!("Running in headless mode. Press Ctrl+C to stop.");
        let _ = rt.block_on(async { shutdown_rx.recv().await });
        println!("Shutdown requested. Exiting...");
    }
}

fn load_dynamic_patterns(engine: &Arc<SimulationEngine>) {
    // Look for patterns relative to the executable location (target/debug/patterns)
    // This ensures it works for both deployment (copy exe+dir) and cargo run (if copied to target).
    let patterns_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("patterns")))
        .unwrap_or_else(|| std::path::PathBuf::from("patterns")); // Fallback to CWD

    if patterns_dir.exists() && patterns_dir.is_dir() {
        println!(
            "Loading patterns from: {:?}",
            patterns_dir
                .canonicalize()
                .unwrap_or(patterns_dir.to_path_buf())
        );
        if let Ok(entries) = std::fs::read_dir(patterns_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                #[allow(clippy::collapsible_if)]
                if path.extension().and_then(|s| s.to_str()) == Some("rle") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut name = path.file_stem().unwrap().to_string_lossy().to_string();
                        let mut description = "User loaded pattern".to_string();

                        // Simple metadata parsing
                        for line in content.lines() {
                            if let Some(stripped) = line.strip_prefix("#N") {
                                name = stripped.trim().to_string();
                            } else if let Some(stripped) = line.strip_prefix("#C") {
                                let comment = stripped.trim();
                                if description == "User loaded pattern" {
                                    description = comment.to_string();
                                } else {
                                    description.push(' ');
                                    description.push_str(comment);
                                }
                            }
                        }

                        engine.register_pattern(rustylife_core::PatternInfo {
                            name: name.clone(),
                            description,
                            rle: content,
                        });
                        println!("Loaded dynamic pattern: {}", name);
                    }
                }
            }
        }
    } else {
        println!(
            "Warning: No local './patterns' directory found. Ensure build script is running or patterns are deployed."
        );
        // We do NOT create the directory anymore, as it is a build artifact.
    }
}

fn make_current_state_payload(
    engine: &SimulationEngine,
    client_type: &Option<ClientType>,
) -> Option<Vec<u8>> {
    match client_type {
        Some(ClientType::MetricsOnly) => {
            let telemetry = engine.capture_metrics_only();
            let resp = Response::TelemetryBundle {
                telemetry: vec![telemetry],
            };
            Some(resp.to_bytes())
        }
        Some(ClientType::FullSnapshot { viewport }) => {
            let (data, telemetry) = engine.capture_current_state();
            let filtered_cells: Vec<_> = if let Some(((min_x, min_y), (max_x, max_y))) = viewport {
                data.iter()
                    .filter(|((x, y), _)| {
                        *x >= *min_x && *x <= *max_x && *y >= *min_y && *y <= *max_y
                    })
                    .copied()
                    .collect()
            } else {
                data
            };
            Some(rustylife_core::encode_binary_packet(
                telemetry.generation,
                &filtered_cells,
                telemetry,
            ))
        }
        None => None,
    }
}

// Handlers
async fn index() -> impl IntoResponse {
    let html = include_str!("../static/index.html");

    // Runtime injection of tooltip constants from rustylife-gui
    // This ensures Single Source of Truth for UI text.
    let html = html
        .replace("{{TOOLTIP_ZOOM}}", rustylife_gui::text::TOOLTIP_ZOOM)
        .replace("{{TOOLTIP_EXTENT}}", rustylife_gui::text::TOOLTIP_EXTENT)
        .replace("{{TOOLTIP_CENTER}}", rustylife_gui::text::TOOLTIP_CENTER)
        .replace("{{TOOLTIP_WORK}}", rustylife_gui::text::TOOLTIP_WORK)
        .replace("{{TOOLTIP_NET}}", rustylife_gui::text::TOOLTIP_NET)
        .replace("{{TOOLTIP_GPS}}", rustylife_gui::text::TOOLTIP_GPS)
        .replace("{{TOOLTIP_CORES}}", rustylife_gui::text::TOOLTIP_CORES);

    axum::response::Html(html)
}

async fn dashboard_js() -> impl IntoResponse {
    axum::response::Response::builder()
        .header("Content-Type", "application/javascript")
        .body(include_str!("../static/dashboard.js").to_owned())
        .unwrap()
}

async fn utils_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("../static/utils.js"),
    )
}

async fn telemetry_html() -> impl IntoResponse {
    axum::response::Html(include_str!("../static/telemetry.html"))
}

async fn telemetry_js() -> impl IntoResponse {
    axum::response::Response::builder()
        .header("Content-Type", "application/javascript")
        .body(include_str!("../static/telemetry.js").to_owned())
        .unwrap()
}

async fn protocol_js() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        include_str!("../static/protocol.js"),
    )
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppStateEnv>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppStateEnv>) {
    let mut rx = state.tx.subscribe();
    let mut shutdown_rx = state.shutdown_tx.subscribe();

    let mut is_ready_for_next_frame = false;
    let mut client_type: Option<ClientType> = None;
    let mut last_sent_generation = 0;

    let mut metrics_buffer: Vec<rustylife_core::Telemetry> = Vec::new();

    // Send Welcome message
    let welcome = Response::Welcome {
        cores: state.cores,
        patterns: state.engine.get_catalog(),
    };
    let _ = socket.send(Message::Binary(welcome.to_bytes())).await;

    loop {
        tokio::select! {
            result = socket.recv() => {
                let bin = match result {
                    Some(Ok(Message::Binary(bin))) => Some(bin),
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) | Some(Ok(Message::Text(_))) => None,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                };

                if let Some(bin) = bin {
                    // Try parsing as Hybrid (Prefix) first, then fallback to Raw JSON
                    let req = Request::from_bytes(&bin).ok()
                        .or_else(|| serde_json::from_slice::<Request>(&bin).ok());

                    if let Some(req) = req {
                        match req {
                            Request::HandshakeMetricsOnly => {
                                client_type = Some(ClientType::MetricsOnly);
                                if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                    let _ = socket.send(Message::Binary(payload)).await;
                                    last_sent_generation = state.engine.generation();
                                }
                                is_ready_for_next_frame = false;
                            }
                            Request::HandshakeFullSnapshot { viewport } => {
                                client_type = Some(ClientType::FullSnapshot { viewport });
                                if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                    let _ = socket.send(Message::Binary(payload)).await;
                                    last_sent_generation = state.engine.generation();
                                }
                                is_ready_for_next_frame = false;
                            }
                            Request::AckPreviousFrame => {
                                match &client_type {
                                    Some(ClientType::MetricsOnly) => {
                                        if !metrics_buffer.is_empty() {
                                            let last_gen = metrics_buffer.last().unwrap().generation;
                                            let resp = Response::TelemetryBundle { telemetry: std::mem::take(&mut metrics_buffer) };
                                            let _ = socket.send(Message::Binary(resp.to_bytes())).await;
                                            last_sent_generation = last_gen;
                                            is_ready_for_next_frame = false;
                                        } else {
                                            is_ready_for_next_frame = true;
                                        }
                                    }
                                    _ => {
                                        let engine_gen = state.engine.generation();
                                        if engine_gen > last_sent_generation {
                                            if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                                let _ = socket.send(Message::Binary(payload)).await;
                                                last_sent_generation = engine_gen;
                                                is_ready_for_next_frame = false;
                                            } else {
                                                is_ready_for_next_frame = true;
                                            }
                                        } else {
                                            is_ready_for_next_frame = true;
                                        }
                                    }
                                }
                            }
                            Request::UpdateViewport { viewport } => {
                                if let Some(ClientType::FullSnapshot { viewport: ref mut vp }) = client_type {
                                    *vp = Some(viewport);
                                    if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                        let _ = socket.send(Message::Binary(payload)).await;
                                        last_sent_generation = state.engine.generation();
                                    }
                                    is_ready_for_next_frame = false;
                                }
                            }
                            Request::NextStep => {
                                state.engine.step();
                            }
                            Request::Reset => {
                                state.engine.reset();
                            }

                            Request::Start => {
                                state.engine.start();
                            }
                            Request::Stop => {
                                let engine = state.engine.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    engine.stop();
                                    // Wait for worker threads to drain so the telemetry correctly reports is_running = false
                                    engine.wait_for_quiescence(std::time::Duration::from_millis(50));
                                })
                                .await;

                                // Force a UI update so the client knows we stopped
                                if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                    let _ = socket.send(Message::Binary(payload)).await;
                                    last_sent_generation = state.engine.generation();
                                }
                            }
                            Request::Seed(pattern) => {
                                state.engine.seed(pattern);
                            }
                            Request::Shutdown => {
                                println!("Server Shutdown requested via WebSocket");
                                let _ = state.shutdown_tx.send(());
                            }
                        }
                    } else {
                        // Log failure only for non-empty packets to avoid spam
                        if !bin.is_empty() {
                            println!("WS: Failed to parse request (len={}): {:?}", bin.len(), bin);
                        }
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok((data, telemetry)) => {
                        if let Some(ClientType::MetricsOnly) = client_type {
                            metrics_buffer.push(telemetry);
                        }

                        // Client requested a frame and engine provided a new snapshot
                        if is_ready_for_next_frame {
                            match client_type {
                                Some(ClientType::MetricsOnly) => {
                                    let last_gen = metrics_buffer.last().unwrap().generation;
                                    let resp = Response::TelemetryBundle { telemetry: std::mem::take(&mut metrics_buffer) };
                                    if socket.send(Message::Binary(resp.to_bytes())).await.is_err() {
                                        break;
                                    }
                                    last_sent_generation = last_gen;
                                    is_ready_for_next_frame = false;
                                }
                                Some(ClientType::FullSnapshot { viewport }) => {
                                    let filtered_cells: Vec<_> = if let Some(((min_x, min_y), (max_x, max_y))) = viewport {
                                        data.iter()
                                            .filter(|((x, y), _)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
                                            .copied()
                                            .collect()
                                    } else {
                                        data.iter().copied().collect()
                                    };

                                    let payload = rustylife_core::encode_binary_packet(telemetry.generation, &filtered_cells, telemetry);
                                    if socket.send(Message::Binary(payload)).await.is_err() {
                                        break;
                                    }
                                    last_sent_generation = telemetry.generation;
                                    is_ready_for_next_frame = false;
                                }
                                None => {}
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // The engine is producing frames faster than this client can pull them
                        // from the broadcast channel. We just skip this warning and the next
                        // loop iteration will pull the very latest frame.
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
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
    let mut shutdown_rx = state.shutdown_tx.subscribe();

    let mut is_ready_for_next_frame = false;
    let mut client_type: Option<ClientType> = None;
    let mut last_sent_generation = 0;

    // Send Welcome message
    let welcome = Response::Welcome {
        cores: state.cores,
        patterns: state.engine.get_catalog(),
    };
    let bytes = welcome.to_bytes();
    let _ = writer.write_all(&bytes).await;

    loop {
        let mut tag_buf = [0u8; 1];
        tokio::select! {
            result = reader.read_exact(&mut tag_buf) => {
                if result.is_err() { break; }

                // Read Length Prefix (first byte read above, need 3 more)
                let mut length_bytes = [0u8; 4];
                length_bytes[0] = tag_buf[0];
                if reader.read_exact(&mut length_bytes[1..]).await.is_err() { break; }

                let len = u32::from_le_bytes(length_bytes) as usize;
                let mut payload = vec![0u8; len];
                if reader.read_exact(&mut payload).await.is_err() { break; }

                let req = match serde_json::from_slice::<Request>(&payload) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        println!("IPC: Failed to deserialize request: {}", e);
                        None
                    }
                };

                if let Some(req) = req {
                    match req {
                        Request::HandshakeMetricsOnly => {
                            client_type = Some(ClientType::MetricsOnly);
                            if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                let _ = writer.write_all(&payload).await;
                                last_sent_generation = state.engine.generation();
                            }
                            is_ready_for_next_frame = false;
                        }
                        Request::HandshakeFullSnapshot { viewport } => {
                            client_type = Some(ClientType::FullSnapshot { viewport });
                            if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                let _ = writer.write_all(&payload).await;
                                last_sent_generation = state.engine.generation();
                            }
                            is_ready_for_next_frame = false;
                        }
                        Request::AckPreviousFrame => {
                            let engine_gen = state.engine.generation();
                            if engine_gen > last_sent_generation {
                                if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                    let _ = writer.write_all(&payload).await;
                                    last_sent_generation = engine_gen;
                                    is_ready_for_next_frame = false;
                                } else {
                                    is_ready_for_next_frame = true;
                                }
                            } else {
                                is_ready_for_next_frame = true;
                            }
                        }
                        Request::UpdateViewport { viewport } => {
                            if let Some(ClientType::FullSnapshot { viewport: ref mut vp }) = client_type {
                                *vp = Some(viewport);
                                if let Some(payload) = make_current_state_payload(&state.engine, &client_type) {
                                    let _ = writer.write_all(&payload).await;
                                    last_sent_generation = state.engine.generation();
                                }
                                is_ready_for_next_frame = false;
                            }
                        }
                        Request::NextStep => { state.engine.step(); }
                        Request::Reset => { state.engine.reset(); }

                        Request::Start => {
                            println!("IPC: Received Start Request");
                            state.engine.start();
                        }
                        Request::Stop => {
                            println!("IPC: Received Stop Request");
                            state.engine.stop();
                        }
                        Request::Seed(pattern) => {
                            println!("IPC: Seeding pattern: {}", pattern);
                            state.engine.seed(pattern);
                        }
                        Request::Shutdown => {
                            let _ = state.shutdown_tx.send(());
                        }
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok((data, telemetry)) => {
                        if is_ready_for_next_frame {
                            match client_type {
                                Some(ClientType::MetricsOnly) => {
                                    let resp = Response::SnapshotAvailable { telemetry };
                                    let bytes = resp.to_bytes();
                                    if writer.write_all(&bytes).await.is_err() {
                                        break;
                                    }
                                    last_sent_generation = telemetry.generation;
                                    is_ready_for_next_frame = false;
                                }
                                Some(ClientType::FullSnapshot { viewport }) => {
                                    let filtered_cells: Vec<_> = if let Some(((min_x, min_y), (max_x, max_y))) = viewport {
                                        data.iter()
                                            .filter(|((x, y), _)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
                                            .copied()
                                            .collect()
                                    } else {
                                        data.iter().copied().collect()
                                    };

                                    let payload = rustylife_core::encode_binary_packet(telemetry.generation, &filtered_cells, telemetry);
                                    if writer.write_all(&payload).await.is_err() {
                                        break;
                                    }
                                    last_sent_generation = telemetry.generation;
                                    is_ready_for_next_frame = false;
                                }
                                None => {}
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod crash_telemetry_tests {
    use super::*;
    use rustylife_core::space::SimulationSpace;

    // Helper macro to easily capture the engine state into a String via our std::io::Write trait proxy
    fn capture_report(engine: Option<&SimulationEngine>) -> String {
        let mut buffer = Vec::new();
        write_crash_telemetry(&mut buffer, engine).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn test_format_crash_telemetry_none() {
        let report = capture_report(None);
        println!("{}", report);
        assert!(report.contains("RUSTYLIFE CRASH REPORT"));
        assert!(report.contains("UNKNOWN"));
        assert!(report.contains(" UTC]"));
    }

    #[test]
    fn test_format_crash_telemetry_some() {
        let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
        let engine = SimulationEngine::new(space.clone(), 1);

        // Manipulate engine state
        engine.generation.store(1234567, Ordering::SeqCst);
        engine.living_count.store(9876543, Ordering::SeqCst);
        *engine.current_generation_bounds.lock().unwrap() = Some(((-10, -5), (10, 5)));

        {
            let mut tel = engine.telemetry.lock().unwrap();
            tel.gps = 12.34;
            tel.work_rate_ema = 56789.01;
            tel.net_rate_ema = 90.12;
        }

        let report = capture_report(Some(&engine));
        println!("{}", report);
        assert!(report.contains("Generation: 1,234,567"));
        assert!(report.contains("Population: 9,876,543"));
        assert!(report.contains("Bounds:     (-10, -5) to (10, 5)"));
        assert!(report.contains("GPS:        12.34 /s"));
        assert!(report.contains("Work Rate:  56.79 K/s"));
        assert!(report.contains("Net Rate:   90.12 /s"));
        assert!(report.contains(" UTC]"));

        engine.shutdown();
    }

    #[test]
    fn test_format_helpers() {
        assert_eq!(format_with_commas(1234567), "1,234,567");
        assert_eq!(format_si_rate(1234.56), "1.23 K/s");
        assert_eq!(format_si_rate(0.79), "0.79 /s");
        assert_eq!(format_si_rate(1000000.0), "1.00 M/s");
    }

    #[test]
    fn test_format_crash_telemetry_locked() {
        let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
        let engine = SimulationEngine::new(space.clone(), 1);

        engine.generation.store(42, Ordering::SeqCst);
        engine.living_count.store(100, Ordering::SeqCst);

        // Explicitly lock the mutexes and keep them locked during the formatter call!
        let _tel_lock = engine.telemetry.lock().unwrap();
        let _bounds_lock = engine.current_generation_bounds.lock().unwrap();

        let report = capture_report(Some(&engine));
        println!("{}", report);
        assert!(report.contains("Generation: 42"));
        assert!(report.contains("Population: 100"));
        assert!(report.contains("Bounds:     Locked"));
        assert!(report.contains("GPS:        Locked"));
        assert!(report.contains("Work Rate:  Locked"));
        assert!(report.contains("Net Rate:   Locked"));

        drop(_bounds_lock);
        engine.shutdown();
    }

    #[test]
    fn test_logging_subscriber_performance_tracking() {
        let sub = LoggingSubscriber::new(20.0);
        let telemetry = rustylife_core::Telemetry {
            generation: 1,
            timestamp: 0,
            population: 100,
            is_running: true,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            bounds: None,
        };

        // 1st snapshot: Sets last_snapshot_time, but no interval yet
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), telemetry);
        assert_eq!(*sub.min_interval.lock().unwrap(), std::time::Duration::MAX);

        // Simulated delay
        std::thread::sleep(std::time::Duration::from_millis(10));

        // 2nd snapshot: Calculates interval, updates min/max IMMEDIATELY (two snapshots = one interval)
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), telemetry);
        let min = *sub.min_interval.lock().unwrap();
        let max = *sub.max_interval.lock().unwrap();
        assert!(min > std::time::Duration::ZERO);
        assert!(min < std::time::Duration::from_millis(50));
        assert_eq!(min, max);

        // 3rd snapshot: Update min/max again
        std::thread::sleep(std::time::Duration::from_millis(20));
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), telemetry);
        let min2 = *sub.min_interval.lock().unwrap();
        let max2 = *sub.max_interval.lock().unwrap();
        assert_eq!(min2, min); // Previous min was ~10ms
        assert!(max2 > max); // New max is ~20ms

        // Test Idle Reset: If is_running = false, last_snapshot_time should be cleared
        let mut idle_telemetry = telemetry;
        idle_telemetry.is_running = false;
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), idle_telemetry);
        assert!(sub.last_snapshot_time.lock().unwrap().is_none());

        // Next start: Should NOT compute a delta from the pre-idle time
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut running_telemetry = telemetry;
        running_telemetry.is_running = true;
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), running_telemetry);
        // last_snapshot_time is now Some(now), but no interval recorded yet
        assert_eq!(*sub.max_interval.lock().unwrap(), max2); // Max hasn't changed from last update
    }

    #[test]
    fn test_logging_subscriber_reset_after_log() {
        // Use an interval of 0 to trigger logging on every call for testing resets
        let sub = LoggingSubscriber::new(0.0);
        let telemetry = rustylife_core::Telemetry {
            generation: 1,
            timestamp: 0,
            population: 100,
            is_running: true,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            bounds: None,
        };

        // 1. First snapshot - establishes baseline
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), telemetry);

        // 2. Second snapshot - records interval AND triggers log (due to 0 interval)
        std::thread::sleep(std::time::Duration::from_millis(10));
        sub.on_snapshot_available(std::sync::Arc::new(vec![]), telemetry);

        // The stats should have been reset AFTER the log trigger
        assert_eq!(*sub.min_interval.lock().unwrap(), std::time::Duration::MAX);
        assert_eq!(*sub.max_interval.lock().unwrap(), std::time::Duration::ZERO);

        // However, last_snapshot_time should NOT have been reset (to allow clean interval to next snap)
        assert!(sub.last_snapshot_time.lock().unwrap().is_some());

        // 3. Test fractional interval: 0.1 minutes = 6 seconds
        let _sub_fractional = LoggingSubscriber::new(0.0001);
    }

    #[test]
    fn test_args_log_interval() {
        use clap::Parser;

        // Default value
        let args = Args::parse_from(["rustylife-server"]);
        assert_eq!(args.log_interval, 20.0);

        // Custom integer value
        let args = Args::parse_from(["rustylife-server", "--log-interval", "5"]);
        assert_eq!(args.log_interval, 5.0);

        // Fractional value
        let args_result = Args::try_parse_from(["rustylife-server", "--log-interval", "1.5"]);
        assert!(
            args_result.is_ok(),
            "Failed to parse fractional interval: {:?}",
            args_result.err()
        );
        assert_eq!(args_result.unwrap().log_interval, 1.5);
    }

    #[test]
    fn test_make_current_state_payload_metrics_only_performance() {
        use rustylife_core::engine::SimulationEngine;
        use rustylife_core::space::SimulationSpace;
        use std::sync::Arc;
        use std::time::Instant;

        // Create a massive simulation space
        let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
        let engine = SimulationEngine::new(space.clone(), 4);

        // Seed massive block of 250,000 cells directly
        for i in 0..500 {
            for j in 0..500 {
                engine.place_cell(i * 10, j * 10);
            }
        }

        // Wait for workers to place the cells
        while engine.work_queue_in_flight() > 0 {
            std::thread::yield_now();
        }

        // Step once to capture generation 1 and telemetry
        engine.step();
        while engine.work_queue_in_flight() > 0
            || engine
                .phase_counter
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0
        {
            std::thread::yield_now();
        }
        // Force sync engine stopping flag since it might be marked for stop
        engine.wait_for_quiescence(std::time::Duration::from_millis(50));

        let client_type = Some(crate::ClientType::MetricsOnly);

        // Measure execution time
        let start = Instant::now();
        let payload = crate::make_current_state_payload(&engine, &client_type);
        let elapsed = start.elapsed();

        assert!(payload.is_some());

        // In O(1), this is ~0ms.
        // In O(N) traversing 250k cells, this takes >> 5ms on modern CPUs.
        assert!(
            elapsed.as_millis() < 5,
            "make_current_state_payload for MetricsOnly took {}ms! This indicates an O(N) traversal bug dropping telemetry frames.",
            elapsed.as_millis()
        );
    }
}
