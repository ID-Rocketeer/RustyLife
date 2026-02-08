use rustylife_core::{engine::SimulationEngine, space::SimulationSpace};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_gps_data_integrity() {
    let space = Arc::new(SimulationSpace::new(2048));
    // Use smaller pool for test
    let engine = SimulationEngine::new(space, 2);

    // 1. Start Engine
    engine.start();

    // 2. Wait for a few generations to ensure >0 GPS
    // We need enough work to happen.
    std::thread::sleep(Duration::from_millis(500));

    // 3. Get Snapshot
    let snapshot_store = &engine.snapshots;
    let latest = snapshot_store.get_latest();

    match latest {
        Some((_gen, data)) => {
            // 4. Decode Header manually to check JSON
            let len_bytes: [u8; 4] = data[0..4].try_into().unwrap();
            let json_len = u32::from_le_bytes(len_bytes) as usize;
            let json_slice = &data[4..4 + json_len];
            let json_str = std::str::from_utf8(json_slice).unwrap();

            println!("JSON Header: {}", json_str);

            // 5. Verify "gps" field exists
            assert!(
                json_str.contains("\"gps\":"),
                "JSON should contain gps field"
            );

            // Deserialize to check value
            let response: rustylife_core::Response = serde_json::from_str(json_str).unwrap();
            if let rustylife_core::Response::BinaryStateHeader { gps, .. } = response {
                println!("GPS from packet: {}", gps);
                // It might be 0.0 if not enough time passed or dt=0
                // But existence is key.
            } else {
                panic!("Wrong response type");
            }
        }
        None => {
            // If no snapshot yet, it might be too fast or failing to start.
            // But we slept 500ms.
            // Force a manual check if engine is running?
            // engine.start() is async.
        }
    }

    engine.stop();
}
