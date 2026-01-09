pub mod cell;
pub mod engine;
pub mod hash;
pub mod space;
pub mod state;
pub mod tree;

use serde::{Deserialize, Serialize};

// The messaging protocol should eventually be updated to
// support viewport-based queries or delta updates, avoiding monolithic state transfers.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Request {
    NextStep,
    Reset,
    GetState,
    Start,
    Stop,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum Response {
    Ok,
    State(Vec<((i128, i128), u8)>),
    BinaryState(Vec<u8>),
    Error(String),
}

pub fn encode_cells_binary(cells: &[((i128, i128), u8)]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + cells.len() * 33);
    buf.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    for ((x, y), state) in cells {
        buf.extend_from_slice(&x.to_le_bytes());
        buf.extend_from_slice(&y.to_le_bytes());
        buf.push(*state);
    }
    buf
}

pub fn decode_cells_binary(buf: &[u8]) -> (u32, Vec<((i128, i128), u8)>) {
    if buf.len() < 4 {
        return (0, Vec::new());
    }
    let count = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let mut cells = Vec::with_capacity(count as usize);
    let mut offset = 4;
    for _ in 0..count {
        if offset + 33 > buf.len() {
            break;
        }
        let x = i128::from_le_bytes(buf[offset..offset + 16].try_into().unwrap());
        let y = i128::from_le_bytes(buf[offset + 16..offset + 32].try_into().unwrap());
        let state = buf[offset + 32];
        cells.push(((x, y), state));
        offset += 33;
    }
    (count, cells)
}
