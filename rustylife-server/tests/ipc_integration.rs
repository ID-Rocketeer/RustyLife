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
    timeout(Duration::from_secs(30), async {
        loop {
            if TcpStream::connect("127.0.0.1:9002").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Server failed to start within 30 seconds"))?;

    // Connect to IPC port
    println!("Connecting to IPC port...");
    let stream = timeout(Duration::from_secs(5), TcpStream::connect("127.0.0.1:9002")).await??;
    let (mut reader, mut writer) = stream.into_split();

    // Wait for push updates (server sends Welcome + initial SnapshotAvailable first)
    println!("Waiting for updates...");
    let mut buffer = Vec::new(); // Start empty - read_buf will append
    let mut offset = 0;

    // Helper to read a response
    async fn read_response(
        reader: &mut tokio::net::tcp::OwnedReadHalf,
        buffer: &mut Vec<u8>,
        offset: &mut usize,
    ) -> anyhow::Result<rustylife_core::Response> {
        loop {
            if *offset >= 4 {
                let len = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
                if *offset >= 4 + len {
                    let (resp, consumed) =
                        rustylife_core::Response::from_bytes(&buffer[0..*offset])
                            .map_err(|e| anyhow::anyhow!("Parse error: {}", e))?;
                    buffer.drain(0..consumed);
                    *offset -= consumed;
                    return Ok(resp);
                }
            }
            let n = reader.read_buf(buffer).await?;
            if n == 0 {
                return Err(anyhow::anyhow!("EOF"));
            }
            *offset += n;
        }
    }

    let mut found_gen_0 = false;
    let mut found_next_gen = false;

    for _ in 0..10 {
        // Try a few messages
        let resp = timeout(
            Duration::from_secs(5),
            read_response(&mut reader, &mut buffer, &mut offset),
        )
        .await??;
        match resp {
            rustylife_core::Response::SnapshotAvailable { generation, .. } => {
                println!("Received SnapshotAvailable for generation: {}", generation);
                if generation == 0 {
                    found_gen_0 = true;
                    // Trigger first step
                    writer
                        .write_all(&rustylife_core::Request::NextStep.to_bytes())
                        .await?;
                } else if generation > 0 {
                    found_next_gen = true;
                    break;
                }
            }
            _ => println!("Received other response: {:?}", resp),
        }
    }

    assert!(found_gen_0, "Should have received initial Gen 0");
    assert!(found_next_gen, "Should have advanced generation");

    Ok(())
}
