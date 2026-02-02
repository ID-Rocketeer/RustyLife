//! # RustyLife Core
//!
//! `rustylife-core` provides the foundational data structures and simulation engine
//! for the RustyLife cellular automata project. This includes the high-performance
//! sparse storage system, the staged simulation engine, and the binary communication
//! protocol.

pub mod cell;
pub mod engine;
pub mod hash;
pub mod scratchpad;
pub mod space;
pub mod state;
pub mod tree;

pub const THREAD_POOL_SIZE: usize = 16;
pub const BUCKET_COUNT: usize = 185;

/// Represents a request from a client to the simulation server.
///
/// Requests are serialized into a tagged binary format for high-performance
/// communication.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Advance the simulation by one generation.
    NextStep,
    /// Retrieve the state of the simulation for a specific generation.
    ///
    /// Optionally restricted to a specific viewport (bounding box).
    GetState {
        /// The generation number to retrieve.
        generation: u64,
        /// The optional bounding box: `((min_x, min_y), (max_x, max_y))`.
        viewport: Option<((i128, i128), (i128, i128))>,
    },
    /// Start the automatic simulation loop.
    Start,
    /// Stop the automatic simulation loop.
    Stop,
    /// Resets the simulation, optionally reloading the last seed pattern.
    Reset,
    /// Seeds the simulation with a named pattern.
    Seed(String),
    /// Gracefully shuts down the simulation server.
    Shutdown,
}

impl Request {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Request::NextStep => buf.push(0x01),
            Request::Reset => buf.push(0x02),
            Request::GetState {
                generation,
                viewport,
            } => {
                buf.push(0x03);
                buf.extend_from_slice(&generation.to_le_bytes());
                match viewport {
                    Some(((x1, y1), (x2, y2))) => {
                        buf.push(1); // Has viewport
                        buf.extend_from_slice(&x1.to_le_bytes());
                        buf.extend_from_slice(&y1.to_le_bytes());
                        buf.extend_from_slice(&x2.to_le_bytes());
                        buf.extend_from_slice(&y2.to_le_bytes());
                    }
                    None => buf.push(0), // No viewport
                }
            }
            Request::Start => buf.push(0x04),
            Request::Stop => buf.push(0x05),
            Request::Seed(pattern) => {
                buf.push(0x06);
                let bytes = pattern.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            Request::Shutdown => buf.push(0x07),
        }
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.is_empty() {
            return Err("Empty buffer".into());
        }
        match buf[0] {
            0x01 => Ok(Request::NextStep),
            0x02 => Ok(Request::Reset),
            0x03 => {
                if buf.len() < 10 {
                    return Err("Payload too short for GetState".into());
                }
                let generation = u64::from_le_bytes(buf[1..9].try_into().unwrap());
                let has_viewport = buf[9];
                let viewport = if has_viewport == 1 {
                    if buf.len() < 10 + 64 {
                        return Err("Payload too short for Viewport".into());
                    }
                    let x1 = i128::from_le_bytes(buf[10..26].try_into().unwrap());
                    let y1 = i128::from_le_bytes(buf[26..42].try_into().unwrap());
                    let x2 = i128::from_le_bytes(buf[42..58].try_into().unwrap());
                    let y2 = i128::from_le_bytes(buf[58..74].try_into().unwrap());
                    Some(((x1, y1), (x2, y2)))
                } else {
                    None
                };
                Ok(Request::GetState {
                    generation,
                    viewport,
                })
            }
            0x04 => Ok(Request::Start),
            0x05 => Ok(Request::Stop),
            0x06 => {
                if buf.len() < 5 {
                    return Err("Payload too short for Seed length".into());
                }
                let len = u32::from_le_bytes(buf[1..5].try_into().unwrap()) as usize;
                if buf.len() < 5 + len {
                    return Err("Incomplete Seed payload".into());
                }
                let pattern = String::from_utf8_lossy(&buf[5..5 + len]).to_string();
                Ok(Request::Seed(pattern))
            }
            0x07 => Ok(Request::Shutdown),
            _ => Err(format!("Unknown Request tag: 0x{:02x}", buf[0])),
        }
    }
}

/// Represents a response from the simulation server to a client.
///
/// Responses use a tagged binary format to minimize overhead.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// A generic success response.
    Ok,
    /// A list of cell states.
    ///
    /// *Deprecated*: Use `BinaryState` for high-performance transfers.
    State(Vec<((i128, i128), u8)>),
    /// A binary-encoded state payload containing cell coordinates and states.
    BinaryState(Vec<u8>),
    /// Notification that a new snapshot is available for a specific generation.
    SnapshotAvailable(u64),
    /// An error message.
    Error(String),
}

impl Response {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Response::Ok => buf.push(0x00),
            Response::State(_) => {
                // Deprecated in favor of BinaryState
                buf.push(0xFE);
            }
            Response::BinaryState(payload) => {
                buf.push(0x02);
                buf.extend_from_slice(&(payload.len() as u64).to_le_bytes());
                buf.extend_from_slice(payload);
            }
            Response::SnapshotAvailable(generation_count) => {
                buf.push(0x01);
                buf.extend_from_slice(&generation_count.to_le_bytes());
            }
            Response::Error(msg) => {
                buf.push(0x03);
                let bytes = msg.as_bytes();
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
        }
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, String> {
        if buf.is_empty() {
            return Err("Empty buffer".into());
        }
        match buf[0] {
            0x00 => Ok(Response::Ok),
            0x01 => {
                if buf.len() < 9 {
                    return Err("Payload too short for Snapshot".into());
                }
                let generation_count = u64::from_le_bytes(buf[1..9].try_into().unwrap());
                Ok(Response::SnapshotAvailable(generation_count))
            }
            0x02 => {
                if buf.len() < 9 {
                    return Err("Payload too short for BinaryState size".into());
                }
                let len = u64::from_le_bytes(buf[1..9].try_into().unwrap()) as usize;
                if buf.len() < 9 + len {
                    return Err("Incomplete BinaryState payload".into());
                }
                Ok(Response::BinaryState(buf[9..9 + len].to_vec()))
            }
            0x03 => {
                if buf.len() < 5 {
                    return Err("Payload too short for Error size".into());
                }
                let len = u32::from_le_bytes(buf[1..5].try_into().unwrap()) as usize;
                if buf.len() < 5 + len {
                    return Err("Incomplete Error payload".into());
                }
                let msg = String::from_utf8_lossy(&buf[5..5 + len]).to_string();
                Ok(Response::Error(msg))
            }
            _ => Err(format!("Unknown Response tag: 0x{:02x}", buf[0])),
        }
    }
}

/// Abstract interface for components that present or visualize simulation state.
///
/// This decouples the source of the simulation data (e.g., File, TCP, WebSocket)
/// from the presentation logic.
pub trait SimulationPresenter: Send + Sync {
    /// Update the presenter with a new simulation packet.
    fn update_state(&mut self, packet: BinaryPacket);

    /// Get the current desired viewport.
    fn get_viewport(&self) -> Option<((i128, i128), (i128, i128))> {
        None
    }
}

/// Strict binary protocol packet:
/// [generation: u64] [total_cells: u64] [is_running: u8] [record_count: u64] [ {x: i128, y: i128, state: u8} ... ] [checksum: u32]
pub fn encode_binary_packet(
    generation: u64,
    total_cells: u64,
    is_running: bool,
    cells: &[((i128, i128), u8)],
) -> Vec<u8> {
    let record_count = cells.len() as u64;
    let packet_size = 8 + 8 + 1 + 8 + (record_count as usize * 33) + 4;
    let mut buf = Vec::with_capacity(packet_size);

    buf.extend_from_slice(&generation.to_le_bytes());
    buf.extend_from_slice(&total_cells.to_le_bytes());
    buf.push(if is_running { 1 } else { 0 });
    buf.extend_from_slice(&record_count.to_le_bytes());

    for ((x, y), state) in cells {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.push(*state);
    }

    // CRC32 Checksum
    let crc = crc32fast::hash(&buf);
    buf.extend_from_slice(&crc.to_le_bytes());

    buf
}

#[derive(Debug, PartialEq)]
pub struct BinaryPacket {
    pub generation: u64,
    pub total_cells: u64,
    pub is_running: bool,
    pub record_count: u64,
    pub cells: Vec<((i128, i128), u8)>,
}

pub fn decode_binary_packet(buf: &[u8]) -> Result<BinaryPacket, String> {
    if buf.len() < 29 {
        // 8 (gen) + 8 (total) + 1 (is_running) + 8 (record_count) + 4 (crc) = 29
        return Err("Packet too short".to_string());
    }

    let (payload, received_crc) = buf.split_at(buf.len() - 4);
    let expected_crc = u32::from_le_bytes(received_crc.try_into().unwrap());
    let actual_crc = crc32fast::hash(payload);

    if actual_crc != expected_crc {
        return Err("Checksum mismatch".to_string());
    }

    let generation = u64::from_le_bytes(payload[0..8].try_into().unwrap());
    let total_cells = u64::from_le_bytes(payload[8..16].try_into().unwrap());
    let is_running = payload[16] == 1;
    let record_count = u64::from_le_bytes(payload[17..25].try_into().unwrap());

    let mut cells = Vec::with_capacity(record_count as usize);
    let mut offset = 25; // 8 (gen) + 8 (total) + 1 (is_running) + 8 (record_count) = 25
    for _ in 0..record_count {
        if offset + 33 > payload.len() {
            return Err("Unexpected end of records".to_string());
        }
        let x = i128::from_le_bytes(payload[offset..offset + 16].try_into().unwrap());
        let y = i128::from_le_bytes(payload[offset + 16..offset + 32].try_into().unwrap());
        let state = payload[offset + 32];
        cells.push(((x, y), state));
        offset += 33;
    }

    Ok(BinaryPacket {
        generation,
        total_cells,
        is_running,
        record_count,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_protocol_integrity() {
        let cells = vec![
            ((0, 0), 0b11),
            ((10, -5), 0b10),
            ((1_000_000_000_000_i128, 9_999_999_999_999_i128), 0b01),
        ];

        let generation_count = 42;
        let total = 1000;
        let packet_buf = encode_binary_packet(generation_count, total, true, &cells);

        let decoded = decode_binary_packet(&packet_buf).expect("Failed to decode");

        assert_eq!(decoded.generation, generation_count);
        assert_eq!(decoded.total_cells, total);
        assert_eq!(decoded.record_count, 3);
        assert_eq!(decoded.cells, cells);
    }

    #[test]
    fn test_checksum_failure() {
        let cells = vec![((0, 0), 0b11)];
        let mut packet_buf = encode_binary_packet(1, 1, false, &cells);

        // Corrupt the packet
        packet_buf[10] ^= 0xFF;

        let result = decode_binary_packet(&packet_buf);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Checksum mismatch");
    }
}
