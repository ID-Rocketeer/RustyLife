use std::process::{Child, Command};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn test_server_client_tcp_interaction() -> anyhow::Result<()> {
    // Start the server in the background
    // We use a different port for testing to avoid collisions
    let server = Command::new("cargo")
        .args([
            "run",
            "-p",
            "rustylife-server",
            "--",
            "--port",
            "8081",
            "--ipc-port",
            "9002",
        ])
        .spawn()?;

    let _guard = ServerGuard(server);

    // Wait for server to start with a timeout
    timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(_) = TcpStream::connect("127.0.0.1:9002").await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Server failed to start within 5 seconds"))?;

    // Connect to IPC port
    println!("Connecting to IPC port...");
    let stream = timeout(Duration::from_secs(5), TcpStream::connect("127.0.0.1:9002")).await??;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Trigger an update by sending NextStep
    println!("Sending NextStep command...");
    let req = rustylife_core::Request::NextStep;
    let req_json = serde_json::to_string(&req)? + "\n";
    timeout(
        Duration::from_secs(5),
        writer.write_all(req_json.as_bytes()),
    )
    .await??;

    // Wait for push update (Count should be 0 because we haven't added any cells)
    println!("Waiting for update (initial)...");
    timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    println!("Received: {}", line);
    assert!(line.contains("\"State\":[]")); // Empty state initially
    line.clear();

    // Trigger another step
    println!("Sending NextStep command (again)...");
    timeout(
        Duration::from_secs(5),
        writer.write_all(req_json.as_bytes()),
    )
    .await??;

    // Wait for update
    println!("Waiting for update (after step)...");
    timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;
    println!("Received: {}", line);
    assert!(line.contains("\"State\":[]")); // Still empty but received

    Ok(())
}
