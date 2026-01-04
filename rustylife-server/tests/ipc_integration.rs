use std::process::{Child, Command};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
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

    // Wait for server to start
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Connect to IPC port
    let stream = TcpStream::connect("127.0.0.1:9002").await?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Initial state check
    reader.read_line(&mut line).await?;
    assert!(line.contains("\"count\":0"));
    line.clear();

    // Send increment command
    writer.write_all(b"increment\n").await?;

    // Wait for push update
    reader.read_line(&mut line).await?;
    assert!(line.contains("\"count\":1"));

    Ok(())
}
