use futures_util::{SinkExt, StreamExt};
use rustylife_core::Request;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

fn encode_request(req: &Request) -> Vec<u8> {
    let json = serde_json::to_vec(req).unwrap();
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&json);
    buf
}

#[tokio::test]
async fn test_gui_shutdown_lifecycle() {
    // Skip if running in headless Linux CI without a display
    if std::env::consts::OS == "linux"
        && std::env::var("DISPLAY").is_err()
        && std::env::var("WAYLAND_DISPLAY").is_err()
    {
        println!("Skipping GUI test on headless Linux system");
        return;
    }

    // 1. Spawn Server with GUI
    let mut server_process = std::process::Command::new("cargo")
        .args([
            "run",
            "--bin",
            "rustylife-server",
            "--",
            "--port",
            "9100",
            "--gui",
        ]) // Enable GUI to repro hang
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn server");

    // Give it time to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    let url = Url::parse("ws://127.0.0.1:9100/ws").unwrap();

    // 2. Connect
    let (ws_stream, _) = connect_async(url.as_str())
        .await
        .expect("Failed to connect to server");
    let (mut write, _) = ws_stream.split();

    // 3. Send Shutdown Command
    println!("Sending Shutdown Command...");
    let shutdown_req = Request::Shutdown;
    let binary_req = encode_request(&shutdown_req);
    write
        .send(Message::Binary(binary_req.into()))
        .await
        .expect("Failed to send Shutdown");

    // 4. Verification: The server process should exit.
    // We poll the process status.
    let mut is_dead = false;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        match server_process.try_wait() {
            Ok(Some(status)) => {
                println!("Server exited with: {}", status);
                is_dead = true;
                break;
            }
            Ok(None) => continue, // Still running
            Err(e) => panic!("Error waiting for process: {}", e),
        }
    }

    // Killing it manually if it didn't die (cleanup)
    if !is_dead {
        let _ = server_process.kill();
    }

    // 5. Assert EXPECTED FAILURE (Broken Test)
    // The user states the server acknowledges the request but logic fails.
    // So this assertion should FAIL if the bug exists (Server is still alive).
    assert!(
        is_dead,
        "Server process did not terminate after Shutdown command!"
    );
}
