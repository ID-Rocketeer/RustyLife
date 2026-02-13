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

#[tokio::test]
async fn test_websocket_protocol_handshake() {
    // 1. Start Server in background
    // We assume the server binary is built or we can run it.
    // Actually, running the full server binary is tricky in a unit test due to port conflicts and lifetime.
    // Instead, we should ideally test the `handle_connection` logic if it were exposed.
    // But since it's an integration test, let's try to spawn the server process.

    // Find server executable
    let status = std::process::Command::new("cargo")
        .args(&["build", "--bin", "rustylife-server"])
        .status()
        .expect("Failed to build server");
    assert!(status.success());

    let mut server_process = std::process::Command::new("cargo")
        .args(&["run", "--bin", "rustylife-server", "--", "--port", "9099"]) // Use non-standard port
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn server");

    // Give it time to start
    tokio::time::sleep(Duration::from_secs(2)).await;

    let url = Url::parse("ws://127.0.0.1:9099/ws").unwrap();

    // 2. Connect
    let (ws_stream, _) = connect_async(url.as_str())
        .await
        .expect("Failed to connect");
    let (mut write, mut read) = ws_stream.split();

    // 3. Send Start Command (JSON encoded)
    let start_req = Request::Start;
    let binary_req = encode_request(&start_req);
    write
        .send(Message::Binary(binary_req.into()))
        .await
        .expect("Failed to send Start");

    // 4. Listen for Updates
    // We expect:
    // - Initially: SnapshotAvailable(0)
    // - After Start: SnapshotAvailable(>0)
    // - Then we request state: GetState -> BinaryStateHeader

    let mut received_generations = Vec::new();

    // Consume messages for a bit
    let timeout = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(msg) = read.next().await {
            let msg = msg.expect("Error reading message");
            if let Message::Binary(data) = msg {
                // Decode output
                if let Ok((response, _)) = Response::from_bytes(&data) {
                    match response {
                        Response::SnapshotAvailable { generation, .. } => {
                            // The original request was to add a redundant check here.
                            // Assuming the intent was to ensure the 'generation' field is correctly extracted.
                            // The original code used `g` for generation, now it's `generation`.
                            println!("Received SnapshotAvailable: {}", generation);
                            received_generations.push(generation);

                            // If we see progress, we can request data
                            if generation > 0 {
                                let get_req = Request::GetState {
                                    generation: generation,
                                    viewport: None,
                                };
                                write
                                    .send(Message::Binary(encode_request(&get_req).into()))
                                    .await
                                    .unwrap();
                            }
                        }
                        Response::BinaryStateHeader { generation, .. } => {
                            println!("Received BinaryStateHeader for Gen {}", generation);
                            return; // Success!
                        }
                        _ => {}
                    }
                }
            }
        }
    })
    .await;

    // Cleanup
    let _ = server_process.kill();

    if timeout.is_err() {
        panic!("Timed out waiting for BinaryStateHeader");
    }
}
