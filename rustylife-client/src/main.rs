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
#[command(author, version, about = "RustyLife Client - Remote simulation monitor", long_about = None)]
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
    fn request_state(&mut self, _generation: u64, viewport: Option<((i128, i128), (i128, i128))>) {
        if let Some(vp) = viewport {
            let _ = self.tx.try_send(Request::UpdateViewport { viewport: vp });
        }
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
    let args = rustylife_core::cli::init_cli::<ClientArgs>();
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
                                    rustylife_core::Response::Welcome { cores, patterns, palette } => {
                                        let req = {
                                            let mut s = state_clone.lock().unwrap();
                                            s.cores = cores;
                                            s.patterns = patterns;
                                            s.palette = palette;
                                            Request::HandshakeFullSnapshot { viewport: s.target_viewport }
                                        };
                                        let _ = writer.write_all(&req.to_bytes()).await;
                                    }
                                    rustylife_core::Response::SnapshotAvailable { .. } => {
                                        // Ignored in push architecture
                                    }
                                    rustylife_core::Response::TelemetryBundle { .. } => {
                                        // Ignored by IPC client because it expects FullSnapshot with BinaryPayload
                                    }
                                    rustylife_core::Response::BinaryStateHeader { record_count, .. } => {
                                        // Binary Payload follows
                                        let payload_size = (record_count as usize * 33) + 4; // Cells + CRC
                                        let mut binary_payload = vec![0u8; payload_size];
                                        if reader.read_exact(&mut binary_payload).await.is_err() { break; }

                                        full_packet.extend_from_slice(&binary_payload);

                                        if let Ok(packet) = rustylife_core::decode_binary_packet(&full_packet) {
                                            let mut s = state_clone.lock().unwrap();

                                            // Directly update the state with the embedded telemetry from the packet
                                            let embedded_telemetry = packet.telemetry;
                                            s.update_state(packet, embedded_telemetry);

                                            // Prompt the Egui thread to render this newly received frame
                                            if let Some(ctx) = &s.repaint_ctx {
                                                ctx.request_repaint();
                                            }
                                        }

                                        // Acknowledge receipt of the frame to get the next one
                                        let _ = writer.write_all(&Request::AckPreviousFrame { viewport: None }.to_bytes()).await;
                                    }
                                    rustylife_core::Response::Error(msg) => {
                                        // Ignore 'not found' errors (though server now sends Ok)
                                        if !msg.contains("not found") {
                                            println!("Server Error: {}", msg);
                                        }
                                    }
                                    rustylife_core::Response::Ok => {
                                        // Silent No-Op
                                    }
                                }
                            }
                        }
                        Some(req) = rx.recv() => {
                            if let Request::UpdateViewport { .. } = &req {
                                if writer.write_all(&req.to_bytes()).await.is_err() {
                                    break;
                                }
                            } else if writer.write_all(&req.to_bytes()).await.is_err() {
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
