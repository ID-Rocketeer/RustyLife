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

async fn read_response(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    buffer: &mut Vec<u8>,
    offset: &mut usize,
) -> anyhow::Result<rustylife_core::Response> {
    loop {
        if *offset >= 4 {
            let len = u32::from_le_bytes(buffer[0..4].try_into().unwrap()) as usize;
            if *offset >= 4 + len {
                let (resp, consumed) = rustylife_core::Response::from_bytes(&buffer[0..*offset])
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

#[tokio::test]
async fn test_server_client_tcp_interaction() -> anyhow::Result<()> {
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
            "--telemetry-port",
            "0",
        ])
        .spawn()?;
    let _guard = ServerGuard(server);

    timeout(Duration::from_secs(30), async {
        loop {
            if TcpStream::connect("127.0.0.1:9002").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Server failed to start"))?;

    let stream = timeout(Duration::from_secs(5), TcpStream::connect("127.0.0.1:9002")).await??;
    let (mut reader, mut writer) = stream.into_split();
    let mut buffer = Vec::new();
    let mut offset = 0;

    let resp = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    match resp {
        rustylife_core::Response::Welcome { .. } => {
            let req = rustylife_core::Request::HandshakeFullSnapshot { viewport: None };
            writer.write_all(&req.to_bytes()).await?;
        }
        _ => panic!("Expected Welcome response"),
    }

    let mut found_gen_0 = false;
    let mut found_next_gen = false;

    for _ in 0..10 {
        let resp = timeout(
            Duration::from_secs(5),
            read_response(&mut reader, &mut buffer, &mut offset),
        )
        .await??;
        match resp {
            rustylife_core::Response::BinaryStateHeader {
                record_count,
                telemetry,
                ..
            } => {
                let payload_size = (record_count as usize * 33) + 4;
                while offset < payload_size {
                    let n = reader.read_buf(&mut buffer).await?;
                    if n == 0 {
                        return Err(anyhow::anyhow!("EOF during binary read"));
                    }
                    offset += n;
                }
                buffer.drain(0..payload_size);
                offset -= payload_size;

                if telemetry.generation == 0 {
                    found_gen_0 = true;
                    writer
                        .write_all(&rustylife_core::Request::NextStep.to_bytes())
                        .await?;
                } else if telemetry.generation > 0 {
                    found_next_gen = true;
                    break;
                }
                writer
                    .write_all(
                        &rustylife_core::Request::AckPreviousFrame { viewport: None }.to_bytes(),
                    )
                    .await?;
            }
            _ => {}
        }
    }

    assert!(found_gen_0, "Should have received initial Gen 0");
    assert!(found_next_gen, "Should have advanced generation");
    Ok(())
}

#[tokio::test]
async fn test_ack_flow_control() -> anyhow::Result<()> {
    let server = Command::new("cargo")
        .args([
            "run",
            "-p",
            "rustylife-server",
            "--",
            "--port",
            "8082",
            "--ipc-port",
            "9003",
            "--telemetry-port",
            "0",
        ])
        .spawn()?;
    let _guard = ServerGuard(server);

    timeout(Duration::from_secs(30), async {
        loop {
            if TcpStream::connect("127.0.0.1:9003").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Server failed to start"))?;

    let stream = TcpStream::connect("127.0.0.1:9003").await?;
    let (mut reader, mut writer) = stream.into_split();
    let mut buffer = Vec::new();
    let mut offset = 0;

    let _ = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    writer
        .write_all(&rustylife_core::Request::HandshakeFullSnapshot { viewport: None }.to_bytes())
        .await?;

    let resp = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    let (gen_num, count) = match resp {
        rustylife_core::Response::BinaryStateHeader {
            telemetry,
            record_count,
            ..
        } => (telemetry.generation, record_count as usize),
        _ => panic!("Expected BinaryStateHeader"),
    };
    assert_eq!(gen_num, 0);

    let payload_size = (count * 33) + 4;
    while offset < payload_size {
        let n = reader.read_buf(&mut buffer).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("EOF"));
        }
        offset += n;
    }
    buffer.drain(0..payload_size);
    offset -= payload_size;

    writer
        .write_all(&rustylife_core::Request::NextStep.to_bytes())
        .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let timeout_res = timeout(
        Duration::from_millis(500),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await;
    assert!(
        timeout_res.is_err(),
        "Server should not send frames without ACK!"
    );

    writer
        .write_all(&rustylife_core::Request::AckPreviousFrame { viewport: None }.to_bytes())
        .await?;
    // We dropped the previous frame by not being ready. Ask the engine to step again
    // so we can receive the new frame now that we are ready.
    writer
        .write_all(&rustylife_core::Request::NextStep.to_bytes())
        .await?;

    let resp2 = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    match resp2 {
        rustylife_core::Response::BinaryStateHeader { telemetry, .. } => {
            assert!(telemetry.generation > 0)
        }
        _ => panic!("Expected Data"),
    }
    Ok(())
}

#[tokio::test]
async fn test_update_viewport_pushes_state_immediately() -> anyhow::Result<()> {
    let server = Command::new("cargo")
        .args([
            "run",
            "-p",
            "rustylife-server",
            "--",
            "--port",
            "8083",
            "--ipc-port",
            "9004",
            "--telemetry-port",
            "0",
        ])
        .spawn()?;
    let _guard = ServerGuard(server);

    timeout(Duration::from_secs(30), async {
        loop {
            if TcpStream::connect("127.0.0.1:9004").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Server failed to start"))?;

    let stream = TcpStream::connect("127.0.0.1:9004").await?;
    let (mut reader, mut writer) = stream.into_split();
    let mut buffer = Vec::new();
    let mut offset = 0;

    let _ = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    writer
        .write_all(&rustylife_core::Request::HandshakeFullSnapshot { viewport: None }.to_bytes())
        .await?;

    let resp = timeout(
        Duration::from_secs(5),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    let (gen_num, count) = match resp {
        rustylife_core::Response::BinaryStateHeader {
            telemetry,
            record_count,
            ..
        } => (telemetry.generation, record_count as usize),
        _ => panic!("Expected BinaryStateHeader"),
    };
    assert_eq!(gen_num, 0);

    let payload_size = (count * 33) + 4;
    while offset < payload_size {
        let n = reader.read_buf(&mut buffer).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("EOF"));
        }
        offset += n;
    }
    buffer.drain(0..payload_size);
    offset -= payload_size;

    // Send UpdateViewport
    writer
        .write_all(
            &rustylife_core::Request::UpdateViewport {
                viewport: ((-10, -10), (10, 10)),
            }
            .to_bytes(),
        )
        .await?;

    // We should receive a response IMMEDIATELY
    let resp2 = timeout(
        Duration::from_secs(1),
        read_response(&mut reader, &mut buffer, &mut offset),
    )
    .await??;
    match resp2 {
        rustylife_core::Response::BinaryStateHeader { telemetry, .. } => {
            assert_eq!(telemetry.generation, 0)
        }
        _ => panic!("Expected Data"),
    }
    Ok(())
}
