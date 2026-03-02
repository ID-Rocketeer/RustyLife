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

import { fmtNum, fmtCoord, formatSI } from './utils.js';
import { encodeRequest } from './protocol.js';

const countEl = /** @type {HTMLSpanElement} */ (document.getElementById('count'));
const genEl = /** @type {HTMLSpanElement} */ (document.getElementById('generation'));
const playPauseBtn = /** @type {HTMLButtonElement} */ (document.getElementById('play-pause-btn'));
const stepBtn = /** @type {HTMLButtonElement} */ (document.getElementById('step-btn'));
const resetBtn = /** @type {HTMLButtonElement} */ (document.getElementById('reset-btn'));
const originBtn = /** @type {HTMLButtonElement} */ (document.getElementById('origin-btn'));
const quitBtn = /** @type {HTMLButtonElement} */ (document.getElementById('quit-btn'));
const patternSelect = /** @type {HTMLSelectElement} */ (document.getElementById('pattern-select'));
const zoomInBtn = /** @type {HTMLButtonElement} */ (document.getElementById('zoom-in-btn'));
const zoomOutBtn = /** @type {HTMLButtonElement} */ (document.getElementById('zoom-out-btn'));

const extentEl = /** @type {HTMLSpanElement} */ (document.getElementById('extent-display'));
const centerEl = /** @type {HTMLSpanElement} */ (document.getElementById('center-display'));
const boundsEl = /** @type {HTMLSpanElement} */ (document.getElementById('bounds-display'));
const expanseEl = /** @type {HTMLSpanElement} */ (document.getElementById('expanse-display'));
const zoomEl = /** @type {HTMLSpanElement} */ (document.getElementById('zoom-display'));
const workEl = /** @type {HTMLSpanElement} */ (document.getElementById('work-display'));
const netEl = /** @type {HTMLSpanElement} */ (document.getElementById('net-display'));
const gpsEl = /** @type {HTMLSpanElement} */ (document.getElementById('gps-display'));
const coresEl = /** @type {HTMLSpanElement} */ (document.getElementById('cores-display'));

const statusDot = /** @type {HTMLDivElement} */ (document.getElementById('status-dot'));
const statusText = /** @type {HTMLSpanElement} */ (document.getElementById('status-text'));
const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById('sim-canvas'));
const ctx = canvas.getContext('2d');

let socket;
let scale = 4; // Initial zoom: 4 pixels per cell (Range: 1-16)
let isRunning = false;
let lastState = null;
let currentGen = 0n;
let lastRenderedGen = -1n;
let pendingRequest = false;
let nextRequestPending = false;
let debounceTimeout = null;
let expectedEpoch = 0;
let pendingRequestEpoch = 0;

// Panning State
let offsetX = 0;
let offsetY = 0;
let isDragging = false;
let lastMouseX = 0;
let lastMouseY = 0;

let latestTelemetry = null;

function uiLoop() {
    if (latestTelemetry) {
        const meta = latestTelemetry;
        latestTelemetry = null; // consume

        const total = meta.population !== undefined ? BigInt(meta.population) : 0n;
        countEl.innerText = total.toLocaleString();

        const workRate = meta.work_rate || 0;
        workEl.innerText = formatSI(workRate, 3, false);

        const netRate = meta.net_rate || 0;
        netEl.innerText = formatSI(netRate, 3, true);

        const gps = meta.gps || 0;
        gpsEl.innerText = formatSI(gps, 3, false);

        const bounds = meta.bounds;
        if (bounds) {
            const [[minX, minY], [maxX, maxY]] = bounds;
            boundsEl.innerText = `[ (${fmtCoord(minX, 9, true)}, ${fmtCoord(minY, 9, true)}) → (${fmtCoord(maxX, 9, true)}, ${fmtCoord(maxY, 9, true)}) ]`;
            const width = Math.abs(maxX - minX) + 1;
            const height = Math.abs(maxY - minY) + 1;
            expanseEl.innerText = `[ ${fmtNum(width, 9, false)} × ${fmtNum(height, 9, false)} ]`;
        } else {
            boundsEl.innerText = `[ (${fmtCoord(0, 9, true)}, ${fmtCoord(0, 9, true)}) → (${fmtCoord(0, 9, true)}, ${fmtCoord(0, 9, true)}) ]`;
            expanseEl.innerText = `[ ${fmtNum(0, 9, false)} × ${fmtNum(0, 9, false)} ]`;
        }
    }
    requestAnimationFrame(uiLoop);
}
requestAnimationFrame(uiLoop);

function updateInstrumentation() {
    if (!canvas.width || !canvas.height) return;
    const w = Math.round(canvas.width / scale);
    const h = Math.round(canvas.height / scale);
    const cx = Math.round(-offsetX / scale);
    const cy = Math.round(offsetY / scale);

    extentEl.innerHTML = `[ ${fmtNum(w, 5, false)} &times; ${fmtNum(h, 5, false)} ]`;
    centerEl.innerHTML = `[ ${fmtNum(cx, 9, true)} , ${fmtNum(cy, 9, true)} ]`;
    zoomEl.innerHTML = `[ ${scale.toFixed(2).padStart(5, '0')}X ]`;
}

canvas.addEventListener('mousedown', (e) => {
    isDragging = true;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    canvas.style.cursor = 'grabbing';
});

window.addEventListener('mousemove', (e) => {
    if (isDragging) {
        const dx = e.clientX - lastMouseX;
        const dy = e.clientY - lastMouseY;
        offsetX += dx;
        offsetY += dy;
        lastMouseX = e.clientX;
        lastMouseY = e.clientY;
        if (lastState) renderCellsHybrid(lastState.meta, lastState.dataView, lastState.binaryOffset, true);
        updateInstrumentation();
    }
});

window.addEventListener('mouseup', () => {
    if (isDragging) {
        isDragging = false;
        canvas.style.cursor = 'default';
        updateServerViewportDebounced();
    }
});

canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    const delta = -Math.sign(e.deltaY);

    const rect = canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    updateZoom(delta, mouseX, mouseY);
}, { passive: false });

function resizeCanvas() {
    canvas.width = canvas.parentElement.clientWidth;
    canvas.height = canvas.parentElement.clientHeight;
    if (lastState) {
        renderCellsHybrid(lastState.meta, lastState.dataView, lastState.binaryOffset, true);
    }
    updateInstrumentation();
    updateServerViewportDebounced();
}
window.onresize = resizeCanvas;
resizeCanvas();


// Adapted for Hybrid Protocol
function renderCellsHybrid(meta, dataView, binaryOffset, forceRender = false) {
    const gen = BigInt(meta.telemetry.generation);

    // Ensure we don't render stale out-of-order packets.
    // We allow gen === lastRenderedGen to support panning/zooming updates while the simulation is stopped.
    if (!forceRender && gen < lastRenderedGen && gen !== 0n) return;

    lastState = { meta, dataView, binaryOffset }; // Store for re-rendering pan/zoom
    lastRenderedGen = gen;

    const recordCount = Number(meta.record_count);

    genEl.innerText = gen.toString();

    // Performance Telemetry (Embedded securely in the BinaryStateHeader payload)
    const telemetry = meta.telemetry;
    if (telemetry) {
        updateTelemetry(telemetry);
    }

    // Render Canvas
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const centerX = canvas.width / 2 + offsetX;
    const centerY = canvas.height / 2 + offsetY;
    const size = scale <= 1 ? scale : scale - 1;

    let recordsOffset = binaryOffset;
    for (let i = 0; i < recordCount; i++) {
        // Check bounds (prevent overrun if CRC is at end)
        if (recordsOffset + 33 > dataView.byteLength) break;

        const x = Number(dataView.getBigInt64(recordsOffset, true));
        const y = Number(dataView.getBigInt64(recordsOffset + 16, true));
        const state = dataView.getUint8(recordsOffset + 32);
        recordsOffset += 33;

        let color;
        switch (state) {
            case 0b11: color = '#3b82f6'; break;
            case 0b10: color = '#10b981'; break;
            case 0b01: color = '#ef4444'; break;
            default: continue;
        }

        ctx.fillStyle = color;
        ctx.fillRect(centerX + x * scale, centerY + y * scale, size, size);
    }
}


function updateTelemetry(meta) {
    if (!meta) return;
    latestTelemetry = meta;
}

function sendRequest(type, payload = null) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(encodeRequest(type, payload));
}


function updateButtonStates() {
    playPauseBtn.disabled = false; // Always enabled for toggle
    playPauseBtn.innerText = isRunning ? "⏸" : "▶";
    resetBtn.disabled = isRunning;
    stepBtn.disabled = isRunning;
    patternSelect.disabled = isRunning;
}

function getViewportPayload() {
    const cx = canvas.width / 2 + offsetX;
    const cy = canvas.height / 2 + offsetY;

    const padding = 20;
    const min_x = Math.floor((0 - cx) / scale) - padding;
    const max_x = Math.ceil((canvas.width - cx) / scale) + padding;
    const min_y = Math.floor((0 - cy) / scale) - padding;
    const max_y = Math.ceil((canvas.height - cy) / scale) + padding;

    return [[min_x, min_y], [max_x, max_y]];
}

function updateServerViewport() {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    sendRequest("UpdateViewport", { viewport: getViewportPayload() });
}

function updateServerViewportDebounced(ms = 250) {
    if (debounceTimeout) clearTimeout(debounceTimeout);
    debounceTimeout = setTimeout(() => {
        updateServerViewport();
        debounceTimeout = null;
    }, ms);
}

function connect() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host || 'localhost:8080';
    socket = new WebSocket(`${protocol}//${host}/ws`);
    socket.binaryType = 'arraybuffer';

    socket.onopen = () => {
        statusDot.classList.add('connected');
        statusText.innerText = 'Connected';
    };

    socket.onmessage = (event) => {
        const rawData = event.data; // ArrayBuffer
        const view = new DataView(rawData);

        // 1. Read Length Prefix (u32 le)
        if (rawData.byteLength < 4) return;
        const jsonLen = view.getUint32(0, true);

        // 2. Parse JSON Header
        if (rawData.byteLength < 4 + jsonLen) return;
        // TextDecoder decode() takes (buffer, options), not (buffer, offset, length)?
        // MDN: decode(buffer) or decode(typedArray). We need to slice or subarray.
        const jsonBytes = new Uint8Array(rawData, 4, jsonLen);
        const jsonStr = new TextDecoder().decode(jsonBytes);

        /** @type {import('./types/Response').Response} */
        let header;
        try {
            header = JSON.parse(jsonStr);
        } catch (e) {
            console.error("Failed to parse JSON header:", e);
            return;
        }

        // 3. Dispatch based on Type
        // Rust Response enum: { type: "SnapshotAvailable", payload: 100 } OR { type: "BinaryStateHeader", ... }
        // Wait, serde_json default enum serialization:
        // #[serde(tag = "type", content = "payload")]
        // So it looks like: { "type": "SnapshotAvailable", "payload": 100 }
        // Or: { "type": "BinaryStateHeader", "generation": ..., "population": ... } (Struct variant is flattened?)
        // Let's check lib.rs...
        // #[serde(tag = "type", content = "payload")] on enum Response
        // BUT BinaryStateHeader is a STRUCT VARIANT.
        // Serde logic:
        // Enum:
        // SnapshotAvailable(u64) -> { "type": "SnapshotAvailable", "payload": 100 }
        // BinaryStateHeader { ... } -> { "type": "BinaryStateHeader", "generation": 100, ... } (Fields are flattened into the object if no content field specified? No, content="payload" usually means struct variants are inside "payload" object via map? 
        // ACTUALLY: Let's assume standard behavior for now. Unit/Struct variants with `tag="..."` usually flatten IF `content` is NOT specified. 
        // But `content="payload"` is specified!
        // So: { "type": "BinaryStateHeader", "payload": { "generation": 100, ... } }
        // ... Wait, let's look at `lib.rs` again if needed. 
        // Line 40: `#[serde(tag = "type", content = "payload")]`
        // Line 47: `BinaryStateHeader { generation: u64, ... }`
        // So `header.type` === "BinaryStateHeader", and `header.payload` is the object with fields.

        if (header.type === "Welcome") {
            const cores = header.payload.cores;
            coresEl.innerHTML = `[ ${String(cores).padStart(2, '0')} ]`;

            // Populate Patterns
            if (header.payload.patterns) {
                header.payload.patterns.forEach(p => {
                    const opt = document.createElement('option');
                    opt.value = p.name;
                    opt.innerText = p.name; // Could use p.description too as title?
                    opt.title = p.description;
                    patternSelect.appendChild(opt);
                });
            }

            // Initiate Push Protocol Handshake
            sendRequest("HandshakeFullSnapshot", { viewport: getViewportPayload() });

        } else if (header.type === "BinaryStateHeader") {
            const meta = header.payload; // { telemetry, record_count, ... }
            const generation = meta.telemetry.generation;

            // Update UI telemetry
            if (meta.telemetry.is_running !== undefined && isRunning !== meta.telemetry.is_running) {
                isRunning = meta.telemetry.is_running;
                updateButtonStates();
            }
            updateTelemetry(meta.telemetry);

            const gen = BigInt(generation);
            if (gen < currentGen) {
                lastRenderedGen = -1n;
                updateInstrumentation();
            }
            currentGen = gen;

            // Binary Payload starts after JSON
            const binaryOffset = 4 + jsonLen;

            renderCellsHybrid(meta, view, binaryOffset);

            // Acknowledge the frame to request the next one
            sendRequest("AckPreviousFrame");
            
        } else if (header.type === "Error") {
            console.error("Server Error:", header.payload);
        }
    };


    socket.onclose = () => {
        statusDot.classList.remove('connected');
        statusText.innerText = 'Disconnected - retrying...';
        setTimeout(connect, 2000);
    };
}

playPauseBtn.onclick = () => {
    if (isRunning) {
        isRunning = false; // Eager UI Update
        updateButtonStates();
        sendRequest("Stop");
    } else {
        isRunning = true; // Eager UI Update
        updateButtonStates();
        sendRequest("Start");
    }
};
stepBtn.onclick = () => {
    sendRequest("NextStep");
};
resetBtn.onclick = () => {
    expectedEpoch++;
    currentGen = 0n;
    lastRenderedGen = -1n;
    sendRequest("Reset");
};
originBtn.onclick = () => {
    offsetX = 0;
    offsetY = 0;
    expectedEpoch++;
    if (lastState) renderCellsHybrid(lastState.meta, lastState.dataView, lastState.binaryOffset, true);
    updateInstrumentation();
    updateServerViewportDebounced();
};
quitBtn.onclick = () => {
    sendRequest("Shutdown");
};

patternSelect.onchange = (e) => {
    const target = /** @type {HTMLSelectElement} */ (e.target);
    const pattern = target.value;
    if (pattern) {
        expectedEpoch++;
        currentGen = 0n;
        lastRenderedGen = -1n;
        sendRequest("Seed", pattern);
        target.value = "";
    }
};

function updateZoom(delta, mouseX = null, mouseY = null) {
    const oldScale = scale;
    scale = Math.max(1, Math.min(16, scale + delta));
    if (scale === oldScale) return;

    // Reset expectedEpoch to stop any pending ghost layout requests that match the old scale
    expectedEpoch++;

    if (mouseX !== null && mouseY !== null) {
        // Adjust offset to keep the point under the mouse stable
        const centerX = canvas.width / 2;
        const centerY = canvas.height / 2;

        const worldX = (mouseX - centerX - offsetX) / oldScale;
        const worldY = (mouseY - centerY - offsetY) / oldScale;

        offsetX = mouseX - centerX - (worldX * scale);
        offsetY = mouseY - centerY - (worldY * scale);
    } else {
        // When using buttons (no mouse focal point), scale offsets proportionally to preserve the logical center
        offsetX = (offsetX / oldScale) * scale;
        offsetY = (offsetY / oldScale) * scale;
    }

    if (lastState) {
        renderCellsHybrid(lastState.meta, lastState.dataView, lastState.binaryOffset, true);
    }
    updateInstrumentation();
    updateServerViewportDebounced();
}

zoomInBtn.onclick = () => updateZoom(1);
zoomOutBtn.onclick = () => updateZoom(-1);

window.addEventListener('keydown', (e) => {
    if (e.key === '+' || e.key === '=') updateZoom(1);
    if (e.key === '-') updateZoom(-1);
});

connect();
updateButtonStates();
