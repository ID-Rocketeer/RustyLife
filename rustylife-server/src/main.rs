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
    Request, Response, SimulationPresenter, Telemetry,
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

    /// Initial seed pattern (glider, blinker, breeder 1)
    #[arg(short, long)]
    pub seed: Option<String>,

    /// Number of generations to run before exiting (for profiling)
    #[arg(long)]
    pub generations: Option<u64>,

    /// Automatically start the simulation after seeding
    #[arg(long)]
    pub autostart: bool,
}

fn to_cartesian_bounds(
    bounds: Option<((i128, i128), (i128, i128))>,
) -> Option<((i128, i128), (i128, i128))> {
    bounds.map(|((min_x, min_y), (max_x, max_y))| {
        // Negate Y to convert from top-down to Cartesian (bottom-up)
        // Swap to maintain bottom-left to top-right ordering
        ((min_x, -max_y), (max_x, -min_y))
    })
}

/// Broadcasts result when a snapshot is ready.
pub struct ServerEngineSubscriber {
    pub engine: Arc<SimulationEngine>,
    pub tx: broadcast::Sender<Response>,
}

impl EngineSubscriber for ServerEngineSubscriber {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<u8>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        let resp = Response::SnapshotAvailable { telemetry };
        let _ = self.tx.send(resp);
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
        _data: Arc<Vec<u8>>,
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
        data: Arc<Vec<u8>>,
        telemetry: rustylife_core::Telemetry,
    ) -> bool {
        if let Ok(packet) = rustylife_core::decode_binary_packet(&data) {
            let mut presenter = self.presenter.lock().unwrap();
            presenter.update_state(packet, telemetry);
        }
        true
    }
}

struct AppStateEnv {
    engine: Arc<SimulationEngine>,
    tx: broadcast::Sender<Response>,
    shutdown_tx: broadcast::Sender<()>,
    cores: usize,
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
        // Create an optimized fetch for visual updates
        if let Some(viewport) = viewport {
            let cells = self.engine.get_cells_in_rect(viewport.0, viewport.1);
            let mut s = self.state.lock().unwrap();
            s.viewport_cells = cells;

            // Also grab atomic counters to keep UI responsive even if snapshots lag
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

    if let Some(engine) = engine {
        writeln!(out, "Simulation State at Crash:")?;
        writeln!(out, "  Generation: {}", engine.generation())?;
        writeln!(
            out,
            "  Population: {}",
            engine.living_count.load(Ordering::Relaxed)
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
            writeln!(out, "  GPS:        {:.2} / s", tel.gps)?;
            writeln!(out, "  Work Rate:  {:.2} / s", tel.work_rate_ema)?;
            writeln!(out, "  Net Rate:   {:.2} / s", tel.net_rate_ema)?;
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
    let args = Args::parse();

    // We must run the GUI on the main thread for Windows/Cross-platform compatibility.
    // So we'll run use a manual Tokio runtime on a background thread.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let pool_size = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1)) // Reserve 1 core for the I/O thread
        .unwrap_or(8);
    println!(
        "Initializing Simulation Engine with {} workers (+1 I/O thread)...",
        pool_size
    );
    let engine = SimulationEngine::new(space.clone(), pool_size);

    // Set up global crash reporting
    let _ = ENGINE_REF.set(Arc::downgrade(&engine));
    setup_panic_hook();

    // Load dynamic patterns from "patterns/" directory
    println!("Loading patterns from ./patterns directory...");
    load_dynamic_patterns(&engine);

    let (tx, _rx) = broadcast::channel::<Response>(100);

    // Register subscriber for real-time broadcasts
    let subscriber = Arc::new(ServerEngineSubscriber {
        engine: engine.clone(),
        tx: tx.clone(),
    });
    engine.add_subscriber(subscriber);

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

fn make_snapshot_response(engine: &SimulationEngine) -> Response {
    let (gps, work, net) = {
        let t = engine.telemetry.lock().unwrap();
        (t.gps, t.work_rate_ema, t.net_rate_ema)
    };
    // Transform bounds to Cartesian coordinates
    let bounds = to_cartesian_bounds(engine.space.bounds());

    let telemetry = Telemetry {
        generation: engine.generation(),
        population: engine.living_count.load(Ordering::Relaxed),
        is_running: !engine.stopping.load(Ordering::Relaxed),
        gps,
        work_rate: work,
        net_rate: net,
        bounds,
    };

    Response::SnapshotAvailable { telemetry }
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

    // Send Welcome message
    let welcome = Response::Welcome {
        cores: state.cores,
        patterns: state.engine.get_catalog(),
    };
    let _ = socket.send(Message::Binary(welcome.to_bytes())).await;

    let resp = make_snapshot_response(&state.engine);
    let _ = socket.send(Message::Binary(resp.to_bytes())).await;

    loop {
        tokio::select! {
            result = socket.recv() => {
                if let Some(Ok(Message::Binary(bin))) = result {
                    // Try parsing as Hybrid (Prefix) first, then fallback to Raw JSON
                    let req = Request::from_bytes(&bin).ok()
                        .or_else(|| serde_json::from_slice::<Request>(&bin).ok());

                    if let Some(req) = req {
                        match req {
                            Request::NextStep => {
                                state.engine.step();
                            }
                            Request::Reset => {
                                state.engine.reset();
                            }
                            Request::GetState { generation, viewport } => {
                                let resp = handle_get_state(&state, generation, viewport).await;
                                let _ = socket.send(Message::Binary(resp)).await;
                            }
                            Request::Start => {
                                state.engine.start();
                            }
                            Request::Stop => {
                                let engine = state.engine.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    engine.stop();
                                })
                                .await;

                                // Force a UI update so the client knows we stopped
                                let resp = make_snapshot_response(&state.engine);
                                let _ = socket.send(Message::Binary(resp.to_bytes())).await;
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
                } else {
                    break;
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
            result = rx.recv() => {
                if let Ok(resp) = result {
                    // resp is already Response::SnapshotAvailable
                    if socket.send(Message::Binary(resp.to_bytes())).await.is_err() {
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

    // Send Welcome message
    let welcome = Response::Welcome {
        cores: state.cores,
        patterns: state.engine.get_catalog(),
    };
    let bytes = welcome.to_bytes();
    let _ = writer.write_all(&bytes).await;

    let resp = make_snapshot_response(&state.engine);
    let bytes = resp.to_bytes();
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
                        Request::NextStep => { state.engine.step(); }
                        Request::Reset => { state.engine.reset(); }
                        Request::GetState { generation, viewport } => {
                            let bytes = handle_get_state(&state, generation, viewport).await;
                            let _ = writer.write_all(&bytes).await;
                        }
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
                if let Ok(resp) = result {
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
) -> Vec<u8> {
    let snapshot = state.engine.snapshots.get(generation);

    if snapshot.is_none() {
        // Return Ok as a silent signal that no data is available for this generation.
        // This prevents console spam in clients during UI events (zoom/pan) while
        // also allowing the client to clear its 'pending_request' flag.
        return Response::Ok.to_bytes();
    }

    let data = snapshot.unwrap();

    // Always decode and re-encode to ensure proper BinaryStateHeader format
    match rustylife_core::decode_binary_packet(&data) {
        Ok(packet) => {
            let filtered_cells: Vec<_> = if let Some(((min_x, min_y), (max_x, max_y))) = viewport {
                packet
                    .cells()
                    .filter(|((x, y), _)| *x >= min_x && *x <= max_x && *y >= min_y && *y <= max_y)
                    .collect()
            } else {
                packet.cells().collect()
            };

            rustylife_core::encode_binary_packet(packet.generation, &filtered_cells)
        }
        Err(e) => {
            Response::Error(format!("Failed to decode snapshot for filtering: {}", e)).to_bytes()
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
    }

    #[test]
    fn test_format_crash_telemetry_some() {
        let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
        let engine = SimulationEngine::new(space.clone(), 1);

        // Manipulate engine state
        engine.generation.store(42, Ordering::SeqCst);
        engine.living_count.store(100, Ordering::SeqCst);
        *engine.current_generation_bounds.lock().unwrap() = Some(((-10, -5), (10, 5)));

        {
            let mut tel = engine.telemetry.lock().unwrap();
            tel.gps = 12.34;
            tel.work_rate_ema = 56.78;
            tel.net_rate_ema = 90.12;
        }

        let report = capture_report(Some(&engine));
        println!("{}", report);
        assert!(report.contains("Generation: 42"));
        assert!(report.contains("Population: 100"));
        assert!(report.contains("Bounds:     (-10, -5) to (10, 5)"));
        assert!(report.contains("GPS:        12.34 / s"));
        assert!(report.contains("Work Rate:  56.78 / s"));
        assert!(report.contains("Net Rate:   90.12 / s"));

        engine.shutdown();
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

        drop(_tel_lock);
        drop(_bounds_lock);
        engine.shutdown();
    }
}
