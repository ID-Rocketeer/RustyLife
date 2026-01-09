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
    Request, Response,
    engine::{EngineSubscriber, SimulationEngine},
    space::SimulationSpace,
};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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
}

/// Broadcasts the current living cell coordinates to all subscribers.
pub struct ServerEngineSubscriber {
    pub engine: Arc<SimulationEngine>,
    pub tx: broadcast::Sender<Vec<((i128, i128), u8)>>,
}

impl EngineSubscriber for ServerEngineSubscriber {
    fn notify_and_wait(&self) -> bool {
        let guard = self.engine.space.index.read();
        let current_mask = guard.current_state_mask();
        let last_mask = guard.last_state_mask();

        let mut all_cells = Vec::new();
        for bucket in &self.engine.space.storage.buckets {
            bucket.collect_all_states(current_mask, last_mask, &mut all_cells);
        }
        let _ = self.tx.send(all_cells);
        true
    }
}

struct AppStateEnv {
    engine: Arc<SimulationEngine>,
    tx: broadcast::Sender<Vec<((i128, i128), u8)>>,
}

struct RustyLifeGui {
    living_cells: Vec<((i128, i128), u8)>,
    state_rx: broadcast::Receiver<Vec<((i128, i128), u8)>>,
    engine: Arc<SimulationEngine>,
}

impl RustyLifeGui {
    fn new(state_env: Arc<AppStateEnv>) -> Self {
        Self {
            living_cells: Vec::new(),
            state_rx: state_env.tx.subscribe(),
            engine: state_env.engine.clone(),
        }
    }
}

impl eframe::App for RustyLifeGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(new_cells) = self.state_rx.try_recv() {
            self.living_cells = new_cells;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RustyLife Native GUI (Integrated - Sparse)");
            ui.label(format!(
                "Living Cells (incl. traces): {}",
                self.living_cells.len()
            ));
            if ui.button("Step").clicked() {
                self.engine.step();
            }

            // Simple visualization list or status
            let born = self.living_cells.iter().filter(|(_, s)| *s == 0b10).count();
            let stable = self.living_cells.iter().filter(|(_, s)| *s == 0b11).count();
            let dying = self.living_cells.iter().filter(|(_, s)| *s == 0b01).count();
            ui.label(format!(
                "Stable: {}, NewBorn: {}, Dying: {}",
                stable, born, dying
            ));
        });

        ctx.request_repaint();
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let space = Arc::new(SimulationSpace::new());
    let engine = SimulationEngine::new(space);
    let (tx, _rx) = broadcast::channel::<Vec<((i128, i128), u8)>>(100);

    let shared_state = Arc::new(AppStateEnv {
        engine: engine.clone(),
        tx: tx.clone(),
    });

    // Register subscriber for real-time broadcasts
    let subscriber = Arc::new(ServerEngineSubscriber {
        engine: engine.clone(),
        tx: tx.clone(),
    });
    engine.add_subscriber(subscriber);

    // Integrated GUI
    if args.gui {
        let state_for_gui = shared_state.clone();
        std::thread::spawn(move || {
            let options = eframe::NativeOptions::default();
            eframe::run_native(
                "RustyLife",
                options,
                Box::new(|_cc| Ok(Box::new(RustyLifeGui::new(state_for_gui)))),
            )
            .unwrap();
        });
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .with_state(shared_state.clone());

    // IPC/TCP Server for Native Clients
    let ipc_state = shared_state.clone();
    let ipc_port = args.ipc_port;
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

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.port))
        .await
        .unwrap();
    println!("Web Server running on http://localhost:{}", args.port);
    axum::serve(listener, app).await.unwrap();
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

    loop {
        tokio::select! {
            result = socket.recv() => {
                if let Some(Ok(Message::Text(text))) = result {
                    if let Ok(req) = serde_json::from_str::<Request>(&text) {
                        match req {
                            Request::NextStep => {
                                state.engine.step();
                            }
                            Request::Reset => {
                                // TODO: Implementation of Reset in engine
                            }
                            Request::GetState => {
                                // Already being broadcasted or on-demand?
                                // Let's send current state immediately.
                            }
                            Request::Start => {
                                state.engine.start();
                            }
                            Request::Stop => {
                                state.engine.stop();
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            result = rx.recv() => {
                if let Ok(cells) = result {
                    let resp = Response::State(cells);
                    if socket.send(Message::Text(serde_json::to_string(&resp).unwrap().into())).await.is_err() {
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
    let mut line = String::new();

    loop {
        tokio::select! {
            result = reader.read_line(&mut line) => {
                if let Ok(n) = result {
                    if n == 0 { break; }
                    if let Ok(req) = serde_json::from_str::<Request>(&line.trim()) {
                        match req {
                            Request::NextStep => { state.engine.step(); }
                            Request::Reset => { /* Reset engine */ }
                            Request::GetState => { /* Send current */ }
                            Request::Start => { state.engine.start(); }
                            Request::Stop => { state.engine.stop(); }
                        }
                    }
                    line.clear();
                } else {
                    break;
                }
            }
            result = rx.recv() => {
                if let Ok(cells) = result {
                    let resp = Response::State(cells);
                    let msg = serde_json::to_string(&resp).unwrap() + "\n";
                    if writer.write_all(msg.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}
