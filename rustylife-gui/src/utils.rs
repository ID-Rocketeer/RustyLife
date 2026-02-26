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

pub fn fmt_num(val: i64, digits: usize, sign: bool) -> String {
    let s = format!("{:0width$}", val.abs(), width = digits);
    if sign {
        format!("{}{}", if val >= 0 { "+" } else { "-" }, s)
    } else {
        s
    }
}

pub fn fmt_coord(val: i128, digits: usize, sign: bool) -> String {
    let s = format!("{:0width$}", val.abs(), width = digits);
    if sign {
        format!("{}{}", if val >= 0 { "+" } else { "-" }, s)
    } else {
        s
    }
}

pub fn format_si(val: f64, digits: usize, signed: bool) -> String {
    let units = ["", "K", "M", "G", "T"];
    // Sub-units: m (milli), u (micro), n (nano)
    let mut v = val.abs();

    // Handle 0 explicitly or very small numbers
    if v < f64::EPSILON {
        v = 0.0;
    }

    let s = if signed {
        if val >= 0.0 {
            "+"
        } else {
            "-"
        }
    } else {
        ""
    };

    let width = digits + 3; // digits + 1 (dot) + 2 (fraction)

    if v == 0.0 {
        #[allow(clippy::format_in_format_args)]
        return format!(
            "[ {}{} \u{00A0}/S ]",
            s,
            format!("{:0>width$.2}", 0.0, width = width)
        );
    }

    // Scale Up
    if v >= 1.0 {
        let mut u = 0;
        while v >= 999.995 && u < units.len() - 1 {
            v /= 1000.0;
            u += 1;
        }
        let n = format!("{:0>width$.2}", v, width = width);
        let unit = if units[u].is_empty() {
            "\u{00A0}" // Non-breaking space for alignment
        } else {
            units[u]
        };
        format!("[ {}{} {}/S ]", s, n, unit)
    } else {
        // Scale Down (m, u, n)
        let sub_units = ["m", "u", "n"];
        let mut su = 0;
        // Limit to nano (1e-9)
        while v < 0.9995 && su < sub_units.len() {
            v *= 1000.0;
            su += 1;
        }

        let n = format!("{:0>width$.2}", v, width = width);
        let unit = if su > 0 {
            sub_units[su - 1]
        } else {
            // Should not happen if v < 0.9995, but fallback
            "\u{00A0}"
        };

        format!("[ {}{} {}/S ]", s, n, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_si() {
        assert_eq!(format_si(123.45, 3, false), "[ 123.45 \u{00A0}/S ]");
        assert_eq!(format_si(0.5, 3, false), "[ 500.00 m/S ]");
        assert_eq!(format_si(0.000005, 3, false), "[ 005.00 u/S ]");
        assert_eq!(format_si(0.0, 3, false), "[ 000.00 \u{00A0}/S ]");
        assert_eq!(format_si(1500.0, 3, false), "[ 001.50 K/S ]");
    }
}
