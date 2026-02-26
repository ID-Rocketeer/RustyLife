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

/**
 * Formats a number with padding and optional sign.
 * @param {number} val The value to format.
 * @param {number} digits Minimum number of digits (padding).
 * @param {boolean} sign Whether to include a explicit +/- sign.
 * @returns {string} The formatted string.
 */
export function fmtNum(val, digits, sign) {
    const s = Math.abs(val).toString().padStart(digits, '0');
    if (sign) {
        return (val >= 0 ? '+' : '-') + s;
    }
    return s;
}

/**
 * Formats a coordinate with padding and optional sign (alias for fmtNum for i128 coordinates).
 * @param {number} val The coordinate value to format.
 * @param {number} digits Minimum number of digits (padding).
 * @param {boolean} sign Whether to include a explicit +/- sign.
 * @returns {string} The formatted string.
 */
export function fmtCoord(val, digits, sign) {
    return fmtNum(val, digits, sign);
}

/**
 * Formats a value using SI units (K, M, G, T).
 * @param {number} val The value to format.
 * @param {number} digits Number of decimal places.
 * @param {boolean} signed Whether to include a +/- sign.
 * @returns {string} The formatted string in [ +000.00 U/S ] format.
 */
export function formatSI(val, digits, signed) {
    const units = ["", "K", "M", "G", "T"];
    // Sub-units
    const subUnits = ["m", "u", "n"];

    let v = Math.abs(val);
    let s = "";

    if (signed) {
        s = val >= 0 ? "+" : "-";
    }

    if (v === 0) {
        // digits + 1 (dot) + 2 (fraction)
        const width = digits + 3;
        const n = (0).toFixed(2).padStart(width, '0');
        return `[ ${s}${n} \u00A0/S ]`;
    }

    // Scale Up
    if (v >= 1.0) {
        let u = 0;
        while (v >= 999.995 && u < units.length - 1) {
            v /= 1000.0;
            u++;
        }
        const unit = units[u] || "";
        const width = digits + 3;
        const n = v.toFixed(2).padStart(width, '0');
        const unitStr = unit === "" ? "\u00A0" : unit;
        return `[ ${s}${n} ${unitStr}/S ]`;
    } else {
        // Scale Down
        let su = 0;
        while (v < 0.9995 && su < subUnits.length) {
            v *= 1000.0;
            su++;
        }
        const unit = su > 0 ? subUnits[su - 1] : "";
        const width = digits + 3;
        const n = v.toFixed(2).padStart(width, '0');
        const unitStr = unit === "" ? "\u00A0" : unit;
        return `[ ${s}${n} ${unitStr}/S ]`;
    }
}
