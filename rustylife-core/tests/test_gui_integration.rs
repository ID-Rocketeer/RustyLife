use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use rustylife_core::{BUCKET_COUNT, Request, Response};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::net::TcpStream;

// Simulates the Server-Side IPC Handler logic (simplified)
async fn mock_server_ipc(stream: TcpStream, engine: Arc<SimulationEngine>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    loop {
        let mut tag_buf = [0u8; 1];
        if reader.read_exact(&mut tag_buf).await.is_err() {
            break;
        }

        let mut length_bytes = [0u8; 4];
        length_bytes[0] = tag_buf[0];
        if reader.read_exact(&mut length_bytes[1..]).await.is_err() {
            break;
        }

        let len = u32::from_le_bytes(length_bytes) as usize;
        let mut payload = vec![0u8; len];
        if reader.read_exact(&mut payload).await.is_err() {
            break;
        }

        if let Ok(req) = serde_json::from_slice::<Request>(&payload) {
            match req {
                Request::Start => engine.start(),
                Request::Stop => engine.stop(),
                Request::NextStep => engine.step(),
                Request::Reset => engine.reset(),
                Request::GetState { generation, .. } => {
                    // Ack with SnapshotAvailable for test
                    let resp = Response::SnapshotAvailable {
                        telemetry: rustylife_core::Telemetry {
                            generation: generation,
                            population: 0,
                            is_running: true,
                            gps: 0.0,
                            work_rate: 0.0,
                            net_rate: 0.0,
                            bounds: None,
                        },
                    };
                    let _ = writer.write_all(&resp.to_bytes()).await;
                }
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn test_ipc_control_logic() {
    let space = Arc::new(SimulationSpace::new(BUCKET_COUNT));
    let engine = SimulationEngine::new(space, 4);
    let engine_clone = engine.clone();

    // Start a mock IPC server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            mock_server_ipc(stream, engine_clone.clone()).await;
        }
    });

    // Connect Client
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test Start
    let req = Request::Start;
    stream.write_all(&req.to_bytes()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !engine.is_stopped(),
        "Engine should be running after Start request"
    );

    // Test Stop
    let req = Request::Stop;
    stream.write_all(&req.to_bytes()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        engine.is_stopped(),
        "Engine should be stopped after Stop request"
    );

    // Test Step
    let initial_gen = engine.generation();
    let req = Request::NextStep;
    stream.write_all(&req.to_bytes()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        engine.generation() > initial_gen,
        "Engine should step after NextStep request"
    );

    // Test Reset
    let req = Request::Reset;
    stream.write_all(&req.to_bytes()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        engine.generation(),
        0,
        "Engine should be at gen 0 after Reset request"
    );
}
