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

use std::collections::HashSet;

fn main() {
    let rle = include_str!("patterns/breeder_1.rle");
    let mut x = 0;
    let mut y = 0;
    let mut num: i128 = 0;
    let mut coords = HashSet::new();

    let data = rle
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('x'))
        .collect::<String>();

    let mut chars = data.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_digit(10) {
            num = num * 10 + ch.to_digit(10).unwrap() as i128;
        } else {
            let count = if num == 0 { 1 } else { num };
            num = 0;
            match ch {
                'b' => x += count,
                'o' => {
                    for i in 0..count {
                        coords.insert((x + i, y));
                    }
                    x += count;
                }
                '$' => {
                    y += count;
                    x = 0;
                }
                '!' => break,
                _ => {}
            }
        }
    }
    println!("Distinct cells: {}", coords.len());
}
