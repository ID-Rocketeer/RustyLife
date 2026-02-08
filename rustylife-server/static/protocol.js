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
