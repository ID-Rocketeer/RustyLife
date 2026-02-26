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
