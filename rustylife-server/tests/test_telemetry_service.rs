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
async fn test_telemetry_service() {
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
            "8089",
            "--ipc-port",
            "9009",
            "--telemetry-port",
            "8087",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn server");
    let _guard = ServerGuard(server_process);

    // Wait for the server to start (including both 8080 and 8086)
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Connect to the new Telemetry Dashboard service on port 8087
    let url = Url::parse("ws://127.0.0.1:8087/ws").unwrap();

    let connect_result = connect_async(url.as_str()).await;

    // We expect the connection to succeed if the service is implemented
    assert!(
        connect_result.is_ok(),
        "Failed to connect to Telemetry service on port 8086. Is it implemented?"
    );

    let (ws_stream, _) = connect_result.unwrap();
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
    assert!(
        found_welcome,
        "Expected Welcome response from Telemetry service"
    );

    // Send HandshakeMetricsOnly
    let handshake_req = Request::HandshakeMetricsOnly;
    write
        .send(Message::Binary(encode_request(&handshake_req).into()))
        .await
        .expect("Failed to send HandshakeMetricsOnly");

    let timeout = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = read.next().await {
            let msg = msg.expect("Error reading message");
            if let Message::Binary(data) = msg {
                let parsed = Response::from_bytes(&data);
                if let Ok((Response::TelemetryBundle { telemetry }, _)) = parsed {
                    println!(
                        "Received TelemetryBundle with {} frames via Telemetry service",
                        telemetry.len()
                    );
                    return; // Success!
                }
            }
        }
    })
    .await;

    if timeout.is_err() {
        panic!("Timed out waiting for TelemetryBundle via Telemetry service");
    }
}
