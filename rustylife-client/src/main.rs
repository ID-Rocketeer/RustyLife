//! # RustyLife Client
//!
//! The `rustylife-client` binary is a native visualization tool for the
//! RustyLife simulation. It connects to the server via IPC (TCP) and
//! implements a high-performance rendering loop with 4-state lifecycle tracking.

use rustylife_core::{Request, SimulationPresenter};
use rustylife_gui::{AppState, RustyLifeApp, UserActionHandler};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct ClientArgs {
    /// Server IP address to connect to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Server port to connect to
    #[arg(short, long, default_value_t = 9001)]
    port: u16,
}

struct ClientActionHandler {
    tx: mpsc::Sender<Request>,
}

impl UserActionHandler for ClientActionHandler {
    fn start(&mut self) {
        let _ = self.tx.try_send(Request::Start);
    }
    fn stop(&mut self) {
        let _ = self.tx.try_send(Request::Stop);
    }
    fn step(&mut self) {
        let _ = self.tx.try_send(Request::NextStep);
    }
    fn reset(&mut self) {
        let _ = self.tx.try_send(Request::Reset);
    }
    fn seed(&mut self, pattern: String) {
        let _ = self.tx.try_send(Request::Seed(pattern));
    }
    fn request_state(&mut self, generation: u64, viewport: Option<((i128, i128), (i128, i128))>) {
        let _ = self.tx.try_send(Request::GetState {
            generation,
            viewport,
        });
    }
    fn shutdown(&mut self) {
        // Send shutdown request to server (?) or just disconnect?
        // Let's send it to be polite/thorough, though connection drop handles it too.
        let _ = self.tx.try_send(Request::Shutdown);
        // We do not need to exit here; the UI loop handles the close command
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = ClientArgs::parse();
    let client_state = Arc::new(Mutex::new(AppState::default()));

    // Ctrl-C Handler for clean shutdown
    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            println!("Client Shutdown requested via Ctrl-C");
            std::process::exit(0);
        }
    });

    let (tx, mut rx) = mpsc::channel::<Request>(10);

    let state_clone = client_state.clone();
    let host = args.host.clone();
    let port = args.port;

    // Background task for TCP communication
    tokio::spawn(async move {
        let addr = format!("{}:{}", host, port);
        loop {
            println!("Connecting to server at {}...", addr);
            if let Ok(stream) = TcpStream::connect(&addr).await {
                println!("Connected!");
                {
                    let mut s = state_clone.lock().unwrap();
                    s.is_connected = true;
                }

                let mut pending_request = false;
                let mut next_request_needed = false;
                let mut latest_generation = 0;

                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                loop {
                    let mut len_buf = [0u8; 4];
                    tokio::select! {
                        result = reader.read_exact(&mut len_buf) => {
                            if result.is_err() { break; }

                            let len = u32::from_le_bytes(len_buf) as usize;
                            let mut json_payload = vec![0u8; len];
                            if reader.read_exact(&mut json_payload).await.is_err() { break; }

                            // Reconstruct partial buffer for potential binary read
                            let mut full_packet = Vec::with_capacity(4 + len);
                            full_packet.extend_from_slice(&len_buf);
                            full_packet.extend_from_slice(&json_payload);

                            if let Ok(response) = serde_json::from_slice::<rustylife_core::Response>(&json_payload) {
                                match response {
                                    rustylife_core::Response::Welcome { cores, patterns } => {
                                        let mut s = state_clone.lock().unwrap();
                                        s.cores = cores;
                                        s.patterns = patterns;
                                    }
                                    rustylife_core::Response::SnapshotAvailable(generation) => {
                                        latest_generation = generation;
                                        if !pending_request {
                                            // Send Request for data
                                            let viewport = {
                                                state_clone.lock().unwrap().target_viewport
                                            };
                                            let req = Request::GetState { generation, viewport };
                                            let _ = writer.write_all(&req.to_bytes()).await;
                                            pending_request = true;
                                        } else {
                                            next_request_needed = true;
                                        }
                                    }
                                    rustylife_core::Response::BinaryStateHeader { record_count, .. } => {
                                        // Binary Payload follows
                                        let payload_size = (record_count as usize * 33) + 4; // Cells + CRC
                                        let mut binary_payload = vec![0u8; payload_size];
                                        if reader.read_exact(&mut binary_payload).await.is_err() { break; }

                                        full_packet.extend_from_slice(&binary_payload);

                                        if let Ok(packet) = rustylife_core::decode_binary_packet(&full_packet) {
                                            let mut s = state_clone.lock().unwrap();
                                            // SimulationPresenter trait is implemented for AppState in rustylife-gui
                                            s.update_state(packet);
                                        }

                                        pending_request = false;
                                        if next_request_needed {
                                            next_request_needed = false;
                                            // Send Request for latest available generation
                                            let viewport = {
                                                state_clone.lock().unwrap().target_viewport
                                            };
                                            let req = Request::GetState { generation: latest_generation, viewport };
                                            let _ = writer.write_all(&req.to_bytes()).await;
                                            pending_request = true;
                                        }
                                    }
                                    rustylife_core::Response::Error(msg) => {
                                        println!("Server Error: {}", msg);
                                        pending_request = false;
                                    }
                                    _ => {
                                        // Catch-all for other messages (e.g. Ok) that shouldn't lock us up
                                        if pending_request {
                                            println!("Received unexpected non-binary response while pending, clearing flag.");
                                            pending_request = false;
                                        }
                                    }
                                }
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
            {
                let mut s = state_clone.lock().unwrap();
                s.is_connected = false;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });

    let options = eframe::NativeOptions::default();

    // Create Handler
    let handler = Box::new(ClientActionHandler { tx });

    eframe::run_native(
        "RustyLife",
        options,
        Box::new(|_cc| {
            Ok(Box::new(RustyLifeApp::new(
                client_state,
                handler,
                None, // Client handles its own shutdown via Window Close -> Drop, or OS signal
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))?;

    Ok(())
}
