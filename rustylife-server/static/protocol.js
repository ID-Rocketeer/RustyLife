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
 * Encodes a request into the Hybrid Protocol Format: [Len: u32][JSON Payload]
 * @param {string} type The request type (enum variant name).
 * @param {any} payload The optional payload (struct/value).
 * @returns {Uint8Array} The encoded buffer.
 */
export function encodeRequest(type, payload = null) {
    // Construct the Request object
    // Serde enum representation: { "type": "Variant", "payload": ... }
    const req = { type };
    if (payload !== null) {
        req.payload = payload;
    }

    // JSON Stringify with BigInt support (downcast to Number for safety)
    const jsonStr = JSON.stringify(req, (key, value) => {
        return typeof value === 'bigint' ? Number(value) : value;
    });

    const encoder = new TextEncoder();
    const jsonBytes = encoder.encode(jsonStr);
    const len = jsonBytes.length;

    // Buffer: [Len (4 bytes)][JSON Bytes]
    const buf = new Uint8Array(4 + len);
    const view = new DataView(buf.buffer);

    view.setUint32(0, len, true); // Little Endian Length
    buf.set(jsonBytes, 4);

    return buf;
}
