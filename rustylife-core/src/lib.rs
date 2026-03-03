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

//! # RustyLife Core
//!
//! `rustylife-core` provides the foundational data structures and simulation engine
//! for the RustyLife cellular automata project. This includes the high-performance
//! sparse storage system, the staged simulation engine, and the binary communication
//! protocol.

pub mod block_tree;
pub mod cell;
pub mod cli;
pub mod engine;
pub mod hash;
pub mod patterns;
pub mod scratchpad;
pub mod space;
pub mod state;
pub mod tree;

pub const THREAD_POOL_SIZE: usize = 16;
pub const BUCKET_COUNT: usize = 185;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Represents a request from the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../rustylife-server/static/types/")]
pub struct PatternInfo {
    pub name: String,
    pub description: String,
    pub rle: String,
}

/// Represents a request from a client to the simulation server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../rustylife-server/static/types/")]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    NextStep,
    Reset,

    Start,
    Stop,
    Seed(String),
    Shutdown,
    HandshakeMetricsOnly,
    HandshakeFullSnapshot {
        viewport: Option<((i128, i128), (i128, i128))>,
    },
    AckPreviousFrame,
    UpdateViewport {
        viewport: ((i128, i128), (i128, i128)),
    },
}

/// Shared telemetry metrics for all interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../rustylife-server/static/types/")]
pub struct Telemetry {
    pub generation: u64,
    pub timestamp: i64,
    pub population: u64,
    pub is_running: bool,
    pub gps: f64,
    pub work_rate: f64,
    pub net_rate: f64,
    pub bounds: Option<((i128, i128), (i128, i128))>,
}

impl Telemetry {
    pub fn to_cartesian_bounds(
        bounds: Option<((i128, i128), (i128, i128))>,
    ) -> Option<((i128, i128), (i128, i128))> {
        bounds.map(|((min_x, min_y), (max_x, max_y))| {
            // Invert Y axes since internal storage is Y-down
            ((min_x, -max_y), (max_x, -min_y))
        })
    }
}

/// Represents a response from the simulation server to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../rustylife-server/static/types/")]
#[serde(tag = "type", content = "payload")]
pub enum Response {
    Ok,
    /// Notification that a new snapshot is available with full telemetry.
    SnapshotAvailable {
        telemetry: Telemetry,
    },
    /// A generic error message.
    Error(String),
    /// Initial handshake with server capabilities.
    Welcome {
        cores: usize,
        #[serde(default)]
        patterns: Vec<PatternInfo>,
    },
    /// A header indicates a binary payload follows.
    /// Redundant metrics stripped to enforce SnapshotAvailable as single source of truth.
    BinaryStateHeader {
        generation: u64,
        record_count: u64,
        telemetry: Telemetry,
    },
}

impl Request {
    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).unwrap();
        let len = json.len() as u32;
        let mut buf = Vec::with_capacity(4 + json.len());
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&json);
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < 4 {
            return Err("Buffer too short for length prefix".into());
        }
        let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        if buf.len() < 4 + len {
            return Err("Buffer too short for JSON payload".into());
        }
        serde_json::from_slice(&buf[4..4 + len]).map_err(|e| e.to_string())
    }
}

impl Response {
    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).unwrap();
        let len = json.len() as u32;
        let mut buf = Vec::with_capacity(4 + json.len());
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(&json);
        buf
    }

    // Note: This only decodes the JSON part. If it's BinaryStateHeader,
    // the caller is responsible for reading the subsequent binary payload.
    pub fn from_bytes(buf: &[u8]) -> Result<(Self, usize), String> {
        if buf.len() < 4 {
            return Err("Buffer too short for length prefix".into());
        }
        let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        if buf.len() < 4 + len {
            return Err("Buffer too short for JSON payload".into());
        }
        let response = serde_json::from_slice(&buf[4..4 + len]).map_err(|e| e.to_string())?;
        Ok((response, 4 + len))
    }
}

/// Abstract interface for components that present or visualize simulation state.
pub trait SimulationPresenter: Send + Sync {
    /// Update the presenter with a new simulation packet and associated telemetry.
    fn update_state(&mut self, packet: BinaryPacket<'_>, telemetry: Telemetry);

    /// Update the presenter with bounds information.
    fn update_bounds(&mut self, _bounds: Option<((i128, i128), (i128, i128))>) {}

    /// Get the current desired viewport.
    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        None
    }
}

/// Hybrid protocol packet:
/// [Header: JSON] + [Payload: Binary]
/// This function constructs the full byte buffer.
pub fn encode_binary_packet(
    generation: u64,
    cells: &[((i128, i128), u8)],
    telemetry: Telemetry,
) -> Vec<u8> {
    let header = Response::BinaryStateHeader {
        generation,
        record_count: cells.len() as u64,
        telemetry,
    };

    let json = serde_json::to_vec(&header).unwrap();
    let json_len = json.len() as u32;

    // Calculate total size: 4 (len) + json + binary cells + 4 (crc)
    let record_count = cells.len();
    let binary_size = record_count * 33;
    let total_size = 4 + json.len() + binary_size + 4;

    let mut buf = Vec::with_capacity(total_size);

    // 1. Length Prefix
    buf.extend_from_slice(&json_len.to_le_bytes());
    // 2. JSON Header
    buf.extend_from_slice(&json);

    // 3. Binary Payload
    for ((x, y), state) in cells {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.push(*state);
    }

    // 4. CRC32 Checksum (of the binary payload ONLY, for speed?)
    // Or of the whole thing? Previously it was whole buffer.
    // Let's checksum the binary payload to ensure integrity of the heavy part.
    // The JSON part is handled by serde's own parsing safety.
    let payload_start = 4 + json.len();
    let payload_end = buf.len();
    let crc = crc32fast::hash(&buf[payload_start..payload_end]);
    buf.extend_from_slice(&crc.to_le_bytes());

    buf
}

#[derive(Debug, PartialEq)]
pub struct BinaryPacket<'a> {
    pub generation: u64,
    pub record_count: u64,
    pub telemetry: Telemetry,
    payload: &'a [u8],
}

impl<'a> BinaryPacket<'a> {
    pub fn cells(&self) -> BinaryCellIterator<'a> {
        BinaryCellIterator {
            payload: self.payload,
            count: self.record_count,
            current: 0,
            offset: 0,
        }
    }
}

pub struct BinaryCellIterator<'a> {
    payload: &'a [u8],
    count: u64,
    current: u64,
    offset: usize,
}

impl<'a> Iterator for BinaryCellIterator<'a> {
    type Item = ((i128, i128), u8);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.count {
            return None;
        }
        if self.offset + 33 > self.payload.len() {
            return None;
        }
        let x = i128::from_le_bytes(
            self.payload[self.offset..self.offset + 16]
                .try_into()
                .unwrap(),
        );
        let y = i128::from_le_bytes(
            self.payload[self.offset + 16..self.offset + 32]
                .try_into()
                .unwrap(),
        );
        let state = self.payload[self.offset + 32];
        self.offset += 33;
        self.current += 1;
        Some(((x, y), state))
    }
}

/// Decodes the new Hybrid Packet.
/// Expects buf to start with [Length: u32][JSON Header]...
pub fn decode_binary_packet(buf: &[u8]) -> Result<BinaryPacket<'_>, String> {
    if buf.len() < 4 {
        return Err("Buffer too short".into());
    }

    // 1. Decode Header
    let (response, consumed) = Response::from_bytes(buf)?;

    match response {
        Response::BinaryStateHeader {
            generation,
            record_count,
            telemetry,
        } => {
            // 2. Verify Binary Payload
            let payload_start = consumed;
            if buf.len() < payload_start + 4 {
                return Err("Missing checksum".into());
            }

            let payload_end = buf.len() - 4;
            let binary_len = payload_end - payload_start;
            let expected_len = (record_count as usize) * 33;

            if binary_len != expected_len {
                return Err(format!(
                    "Payload length mismatch. Expected {}, got {}",
                    expected_len, binary_len
                ));
            }

            let payload = &buf[payload_start..payload_end];
            let received_crc = u32::from_le_bytes(buf[payload_end..].try_into().unwrap());
            let actual_crc = crc32fast::hash(payload);

            if actual_crc != received_crc {
                return Err("Checksum mismatch".into());
            }

            Ok(BinaryPacket {
                generation,
                record_count,
                telemetry,
                payload,
            })
        }
        _ => Err("Packet is not a BinaryStateHeader".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_protocol_integrity() {
        let cells = vec![((0, 0), 0b11), ((1, 1), 0b10)];

        let generation_count = 101;
        let mock_telemetry = Telemetry {
            generation: generation_count,
            timestamp: 1715494444000,
            population: 2,
            is_running: true,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            bounds: None,
        };
        let packet_buf = encode_binary_packet(generation_count, &cells, mock_telemetry);

        let decoded = decode_binary_packet(&packet_buf).expect("Failed to decode");

        assert_eq!(decoded.generation, generation_count);
        assert_eq!(decoded.record_count, 2);
        let decoded_cells: Vec<_> = decoded.cells().collect();
        assert_eq!(decoded_cells, cells);
    }

    #[test]
    fn test_checksum_failure() {
        let cells = vec![((0, 0), 0b11)];
        let mock_telemetry = Telemetry {
            generation: 1,
            timestamp: 1715494444000,
            population: 1,
            is_running: true,
            gps: 0.0,
            work_rate: 0.0,
            net_rate: 0.0,
            bounds: None,
        };
        let packet_buf = encode_binary_packet(1, &cells, mock_telemetry);

        let mut corrupted_buf = packet_buf;
        // Corrupt the packet (last byte)
        let last = corrupted_buf.len() - 1;
        corrupted_buf[last] ^= 0xFF;

        let result = decode_binary_packet(&corrupted_buf);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Checksum mismatch");
    }

    #[test]
    fn export_ts_bindings() {
        use ts_rs::TS;
        let config = ts_rs::Config::default();
        Request::export(&config).expect("Failed to export Request type");
        Response::export(&config).expect("Failed to export Response");
        Telemetry::export(&config).expect("Failed to export Telemetry");
        PatternInfo::export(&config).expect("Failed to export PatternInfo");
    }
}
