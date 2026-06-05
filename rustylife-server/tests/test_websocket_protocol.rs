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

use futures_util::{SinkExt, StreamExt};
use rustylife_core::{Request, Response};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

// Helper to encode Request as [Len][JSON]
fn encode_request(req: &Request) -> Vec<u8> {
    let json = serde_json::to_vec(req).unwrap();
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&json);
    buf
}

struct ServerGuard(std::process::Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn test_websocket_protocol_handshake() {
    let status = std::process::Command::new("cargo")
        .args(["build", "--bin", "rustylife-server"])
        .status()
        .expect("Failed to build server");
    assert!(status.success());

    let server_process = std::process::Command::new("cargo")
        .args([
            "run",
            "--bin",
            "rustylife-server",
            "--",
            "--port",
            "9099",
            "--telemetry-port",
            "0",
            "--ipc-port",
            "0",
        ]) // Use non-standard port
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn server");
    let _guard = ServerGuard(server_process);

    tokio::time::sleep(Duration::from_secs(2)).await;

    let url = Url::parse("ws://127.0.0.1:9099/ws").unwrap();

    let (ws_stream, _) = connect_async(url.as_str())
        .await
        .expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    let mut found_welcome = false;
    let _timeout = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(msg) = read.next().await {
            let msg = msg.expect("Error reading message");
            if let Message::Binary(data) = msg {
                let parsed = Response::from_bytes(&data);
                if let Ok((Response::Welcome { .. }, _)) = parsed {
                    found_welcome = true;
                    break;
                }
            }
        }
    })
    .await;
    assert!(found_welcome, "Expected Welcome response");

    // Send Handshake
    let handshake_req = Request::HandshakeFullSnapshot { viewport: None };
    write
        .send(Message::Binary(encode_request(&handshake_req).into()))
        .await
        .expect("Failed to send Handshake");

    let timeout = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = read.next().await {
            let msg = msg.expect("Error reading message");
            if let Message::Binary(data) = msg {
                let parsed = Response::from_bytes(&data);
                if let Ok((Response::BinaryStateHeader { telemetry, .. }, _)) = parsed {
                    println!(
                        "Received BinaryStateHeader for Gen {}",
                        telemetry.generation
                    );
                    return; // Success!
                }
            }
        }
    })
    .await;

    if timeout.is_err() {
        panic!("Timed out waiting for BinaryStateHeader");
    }
}
