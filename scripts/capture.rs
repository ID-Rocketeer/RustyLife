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

    // 1. Get current generation (the server sends Response::SnapshotAvailable on connect)
    let mut buf = [0u8; 9];
    stream.read_exact(&mut buf)?;
    if buf[0] != 0x01 {
        println!("Expected SnapshotAvailable (0x01), got 0x{:02x}", buf[0]);
        return Ok(());
    }
    let gen = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    println!("Server is currently at Generation: {}", gen);

    // 2. Request Full State (Request::GetState { gen, viewport: None })
    // Tag: 0x03, gen: u64, has_viewport: 0
    let mut req = Vec::new();
    req.push(0x03);
    req.extend_from_slice(&gen.to_le_bytes());
    req.push(0); // No viewport

    println!("Requesting full binary snapshot for generation {}...", gen);
    stream.write_all(&req)?;

    // 3. Receive Response::BinaryState (0x02)
    // Tag: 0x02, len: u64, payload: [u8; len]
    let mut resp_header = [0u8; 9];
    stream.read_exact(&mut resp_header)?;
    if resp_header[0] != 0x02 {
        println!("Expected BinaryState (0x02), got 0x{:02x}", resp_header[0]);
        // Might be an Error (0x03)
        if resp_header[0] == 0x03 {
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf)?;
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut msg = vec![0u8; len];
            stream.read_exact(&mut msg)?;
            println!("Error from server: {}", String::from_utf8_lossy(&msg));
        }
        return Ok(());
    }

    let len = u64::from_le_bytes(resp_header[1..9].try_into().unwrap());
    println!(
        "Downloading snapshot ({} bytes / ~{:.2} MB)...",
        len,
        len as f64 / 1024.0 / 1024.0
    );

    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;

    // 4. Save to Disk
    let filename = format!("breeder_gen_{}_checkpoint.binsnap", gen);
    let mut file = std::fs::File::create(&filename)?;
    file.write_all(&payload)?;

    println!("SUCCESS: Checkpoint saved to {}", filename);
    Ok(())
}
