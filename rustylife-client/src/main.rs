use eframe::egui;
use rustylife_core::{Request, Response};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

struct RustyLifeClientApp {
    living_cells: Arc<Mutex<Vec<(i128, i128)>>>,
    tx: mpsc::Sender<Request>,
}

impl RustyLifeClientApp {
    fn new(living_cells: Arc<Mutex<Vec<(i128, i128)>>>, tx: mpsc::Sender<Request>) -> Self {
        Self { living_cells, tx }
    }
}

impl eframe::App for RustyLifeClientApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let cells = self.living_cells.lock().unwrap().clone();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("RustyLife Standalone Client (Sparse)");
            ui.label(format!("Living Cells: {}", cells.len()));
            if ui.button("Next Step").clicked() {
                let _ = self.tx.try_send(Request::NextStep);
            }
        });

        ctx.request_repaint();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let living_cells = Arc::new(Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::channel::<Request>(10);

    let cells_clone = living_cells.clone();

    // Background task for TCP communication
    tokio::spawn(async move {
        loop {
            println!("Connecting to server...");
            if let Ok(stream) = TcpStream::connect("127.0.0.1:9001").await {
                println!("Connected!");
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                let mut line = String::new();

                loop {
                    tokio::select! {
                        result = reader.read_line(&mut line) => {
                            if let Ok(n) = result {
                                if n == 0 { break; }
                                if let Ok(resp) = serde_json::from_str::<Response>(&line) {
                                    if let Response::State(new_cells) = resp {
                                        let mut s = cells_clone.lock().unwrap();
                                        *s = new_cells;
                                    }
                                }
                                line.clear();
                            } else {
                                break;
                            }
                        }
                        Some(req) = rx.recv() => {
                            let msg = serde_json::to_string(&req).unwrap() + "\n";
                            if writer.write_all(msg.as_bytes()).await.is_err() {
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
        Box::new(|_cc| Ok(Box::new(RustyLifeClientApp::new(living_cells, tx)))),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
