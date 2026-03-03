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

use std::io::{Read, Write};
use std::net::TcpStream;

fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:9001")?;
    println!("Connected to RustyLife Server on 9001");

    // 1. Get current generation (the server sends Response::Welcome on connect)
    let mut tag_buf = [0u8; 1];
    stream.read_exact(&mut tag_buf)?;

    // Read Length Prefix (first byte read above, need 3 more)
    let mut length_buf = [0u8; 4];
    length_buf[0] = tag_buf[0];
    stream.read_exact(&mut length_buf[1..])?;
    let len = u32::from_le_bytes(length_buf) as usize;
    let mut _welcome_payload = vec![0u8; len];
    stream.read_exact(&mut _welcome_payload)?;

    // 2. Request Full State
    let req = rustylife_core::Request::HandshakeFullSnapshot { viewport: None };
    println!("Requesting full binary snapshot...");
    stream.write_all(&req.to_bytes())?;

    // 3. Receive Response::BinaryStateHeader
    let mut resp_tag = [0u8; 1];
    stream.read_exact(&mut resp_tag)?;
    let mut header_len_buf = [0u8; 4];
    header_len_buf[0] = resp_tag[0];
    stream.read_exact(&mut header_len_buf[1..])?;
    let header_len = u32::from_le_bytes(header_len_buf) as usize;
    let mut header_payload = vec![0u8; header_len];
    stream.read_exact(&mut header_payload)?;

    let gen = match serde_json::from_slice::<rustylife_core::Response>(&header_payload) {
        Ok(rustylife_core::Response::BinaryStateHeader { generation, .. }) => generation,
        _ => {
            println!("Expected BinaryStateHeader");
            return Ok(());
        }
    };
    println!("Server replied with Generation: {}", gen);

    // Read the 4-byte raw payload length prefix that follows BinaryStateHeader
    let mut raw_len_buf = [0u8; 4];
    stream.read_exact(&mut raw_len_buf)?;
    let raw_len = u32::from_le_bytes(raw_len_buf);

    println!(
        "Downloading snapshot ({} bytes / ~{:.2} MB)...",
        raw_len,
        raw_len as f64 / 1024.0 / 1024.0
    );

    let mut payload = vec![0u8; raw_len as usize];
    stream.read_exact(&mut payload)?;

    // 4. Save to Disk
    let filename = format!("breeder_gen_{}_checkpoint.binsnap", gen);
    let mut file = std::fs::File::create(&filename)?;
    file.write_all(&payload)?;

    println!("SUCCESS: Checkpoint saved to {}", filename);
    Ok(())
}
