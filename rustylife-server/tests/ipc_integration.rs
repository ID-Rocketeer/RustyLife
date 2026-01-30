use std::process::{Child, Command};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    let (mut reader, mut writer) = stream.into_split();

    // Trigger an update by sending NextStep
    println!("Sending NextStep command...");
    let req = rustylife_core::Request::NextStep;
    timeout(Duration::from_secs(5), writer.write_all(&req.to_bytes())).await??;

    // Wait for push update (SnapshotAvailable 0x01)
    println!("Waiting for update (initial)...");
    let mut resp_tag = [0u8; 1];
    timeout(Duration::from_secs(5), reader.read_exact(&mut resp_tag)).await??;
    assert_eq!(resp_tag[0], 0x01); // SnapshotAvailable tag

    let mut gen_buf = [0u8; 8];
    timeout(Duration::from_secs(5), reader.read_exact(&mut gen_buf)).await??;
    let generation_count = u64::from_le_bytes(gen_buf);
    println!(
        "Received SnapshotAvailable for generation: {}",
        generation_count
    );

    // Trigger another step
    println!("Sending NextStep command (again)...");
    timeout(Duration::from_secs(5), writer.write_all(&req.to_bytes())).await??;

    // Wait for update
    println!("Waiting for update (after step)...");
    timeout(Duration::from_secs(5), reader.read_exact(&mut resp_tag)).await??;
    assert_eq!(resp_tag[0], 0x01);

    timeout(Duration::from_secs(5), reader.read_exact(&mut gen_buf)).await??;
    let generation_count_2 = u64::from_le_bytes(gen_buf);
    println!(
        "Received SnapshotAvailable for generation: {}",
        generation_count_2
    );
    assert!(generation_count_2 > generation_count);

    Ok(())
}
