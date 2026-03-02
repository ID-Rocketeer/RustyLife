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

use rustylife_core::Request;

#[test]
fn test_request_serialization() {
    let start = Request::Start;
    let json = serde_json::to_string(&start).unwrap();
    println!("Start JSON: {}", json);

    // Ensure we can deserialize it back
    let deserialized: Request = serde_json::from_str(&json).unwrap();
    assert_eq!(start, deserialized);

    let seed = Request::Seed("glider".to_string());
    let json_seed = serde_json::to_string(&seed).unwrap();
    println!("Seed JSON: {}", json_seed);
    let deserialized_seed: Request = serde_json::from_str(&json_seed).unwrap();
    assert_eq!(seed, deserialized_seed);
}

#[test]
fn test_handshake_serialization() {
    let hs_metrics = Request::HandshakeMetricsOnly;
    let json = serde_json::to_string(&hs_metrics).unwrap();
    let de_hs_metrics: Request = serde_json::from_str(&json).unwrap();
    assert_eq!(hs_metrics, de_hs_metrics);

    let hs_full = Request::HandshakeFullSnapshot {
        viewport: Some(((-10, -10), (10, 10))),
    };
    let json_full = serde_json::to_string(&hs_full).unwrap();
    let de_hs_full: Request = serde_json::from_str(&json_full).unwrap();
    assert_eq!(hs_full, de_hs_full);

    let ack = Request::AckPreviousFrame;
    let de_ack: Request = serde_json::from_str(&serde_json::to_string(&ack).unwrap()).unwrap();
    assert_eq!(ack, de_ack);

    let up_view = Request::UpdateViewport {
        viewport: ((-5, -5), (5, 5)),
    };
    let de_up_view: Request =
        serde_json::from_str(&serde_json::to_string(&up_view).unwrap()).unwrap();
    assert_eq!(up_view, de_up_view);
}

#[test]
fn test_binary_packet_with_telemetry() {
    let cells = vec![((1, 2), 0b11), ((3, 4), 0b10)];
    let generation = 100;
    let telemetry = rustylife_core::Telemetry {
        generation: 100,
        timestamp: 1715494444000,
        population: 50,
        is_running: true,
        gps: 120.5,
        work_rate: 10_000.0,
        net_rate: 0.0,
        bounds: Some(((-10, -10), (10, 10))),
    };

    let encoded = rustylife_core::encode_binary_packet(generation, &cells, telemetry);
    let decoded = rustylife_core::decode_binary_packet(&encoded).expect("Failed to decode");

    assert_eq!(decoded.generation, generation);
    assert_eq!(decoded.record_count, 2);
    assert_eq!(decoded.telemetry, telemetry);

    let decoded_cells: Vec<_> = decoded.cells().collect();
    assert_eq!(decoded_cells, cells);
}
