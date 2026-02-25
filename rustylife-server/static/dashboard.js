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

// Telemetry State (Server-Side)
let pendingTelemetry = new Map(); // Generation -> Telemetry
// Panning State
let offsetX = 0;
let offsetY = 0;
let isDragging = false;
let lastMouseX = 0;
let lastMouseY = 0;

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
        requestStateDebounced();
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
    requestStateDebounced();
}
window.onresize = resizeCanvas;
resizeCanvas();


// Adapted for Hybrid Protocol
function renderCellsHybrid(meta, dataView, binaryOffset, forceRender = false) {
    const gen = BigInt(meta.generation);

    // Drop out-of-order packets based on what we actually rendered, BUT accept Gen 0 (Reset)
    // Local UI events override this check using forceRender.
    if (!forceRender && gen <= lastRenderedGen && gen !== 0n) return;

    lastState = { meta, dataView, binaryOffset }; // Store for re-rendering pan/zoom
    lastRenderedGen = gen;
    currentGen = gen; // Keep synched

    const recordCount = Number(meta.record_count);

    genEl.innerText = gen.toString();

    // Performance Telemetry (Atomic sync with cached announcement)
    const telemetry = pendingTelemetry.get(meta.generation);
    if (telemetry) {
        updateTelemetry(telemetry);
        pendingTelemetry.delete(meta.generation);

        // Cleanup old entries (robustness)
        if (pendingTelemetry.size > 100) {
            const keys = Array.from(pendingTelemetry.keys()).sort((a, b) => a - b);
            for (let i = 0; i < keys.length - 50; i++) {
                pendingTelemetry.delete(keys[i]);
            }
        }
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

    if (meta.is_running !== undefined) {
        isRunning = meta.is_running;
        updateButtonStates();
    }

    // Total Cells / Population
    const total = meta.population !== undefined ? BigInt(meta.population) : 0n;
    countEl.innerText = total.toLocaleString();

    // Work Rate
    const workRate = meta.work_rate || 0;
    workEl.innerText = formatSI(workRate, 3, false);

    // Net Rate
    const netRate = meta.net_rate || 0;
    netEl.innerText = formatSI(netRate, 3, true);

    // GPS
    const gps = meta.gps || 0;
    gpsEl.innerText = formatSI(gps, 3, false);
    // Use toFixed(2) for consistency (or formatSI/Engineering if desired, but user asked for Engineering later?)
    // User asked "We should also be prepared to display GPS as a floating point value... use engineering notation".
    // formatSI supports engineering notation? 
    // Let's use formatSI(gps, 3, false) like others for now, or just fixed if simple.
    // User: "As breeder 1 grows the GPS will steadily fall... We should use engineering notation to display this value in all UIs."
    // formatSI(num, width, sign)
    gpsEl.innerText = formatSI(gps, 3, false);

    // Bounds and Expanse
    const bounds = meta.bounds;
    if (bounds) {
        const [[minX, minY], [maxX, maxY]] = bounds;
        // Bounds already in Cartesian coordinates from server
        boundsEl.innerText = `[ (${fmtCoord(minX, 9, true)}, ${fmtCoord(minY, 9, true)}) → (${fmtCoord(maxX, 9, true)}, ${fmtCoord(maxY, 9, true)}) ]`;

        const width = Math.abs(maxX - minX) + 1;
        const height = Math.abs(maxY - minY) + 1;
        expanseEl.innerText = `[ ${fmtNum(width, 9, false)} × ${fmtNum(height, 9, false)} ]`;
    } else {
        boundsEl.innerText = `[ (${fmtCoord(0, 9, true)}, ${fmtCoord(0, 9, true)}) → (${fmtCoord(0, 9, true)}, ${fmtCoord(0, 9, true)}) ]`;
        expanseEl.innerText = `[ ${fmtNum(0, 9, false)} × ${fmtNum(0, 9, false)} ]`;
    }
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

function requestState() {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;

    if (pendingRequest) {
        nextRequestPending = true;
        return;
    }

    pendingRequest = true;

    // Calculate visible bounds
    const cx = canvas.width / 2 + offsetX;
    const cy = canvas.height / 2 + offsetY;

    const padding = 20;
    const min_x = Math.floor((0 - cx) / scale) - padding;
    const max_x = Math.ceil((canvas.width - cx) / scale) + padding;
    const min_y = Math.floor((0 - cy) / scale) - padding;
    const max_y = Math.ceil((canvas.height - cy) / scale) + padding;

    // Send JSON Request
    const viewport = [[min_x, min_y], [max_x, max_y]];

    // Request::GetState { generation, viewport }
    sendRequest("GetState", {
        generation: currentGen,
        viewport
    });

    // Safety: Auto-reset if stuck (e.g. server crash, dropped packet)
    // This MUST be inside requestState to cover server-triggered updates (like Reset/Stop)
    setTimeout(() => {
        if (pendingRequest) {
            console.warn("Request timed out, force resetting state");
            pendingRequest = false;
            nextRequestPending = false;
        }
    }, 2000);
}

function requestStateDebounced(ms = 250) {
    if (debounceTimeout) clearTimeout(debounceTimeout);
    debounceTimeout = setTimeout(() => {
        requestState();
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

        if (header.type === "SnapshotAvailable") {
            const { telemetry } = header.payload;
            const generation = telemetry.generation;
            const gen = BigInt(generation);
            if (gen === 0n && currentGen !== 0n) {
                lastRenderedGen = -1n;
                updateInstrumentation();
            }
            currentGen = gen;

            // (DOM updates removed from here to prevent JS thread starvation)

            // Cache telemetry for atomic update with binary cells
            pendingTelemetry.set(generation, telemetry);

            // Fetch state immediately to avoid trailing-edge debounce starvation
            requestState();
        } else if (header.type === "Welcome") {
            const cores = header.payload.cores;
            coresEl.innerHTML = `[ ${String(cores).padStart(2, '0')} ]`;

            // Populate Patterns
            patternSelect.innerHTML = '<option value="" disabled selected>Select Pattern...</option>';
            if (header.payload.patterns) {
                header.payload.patterns.forEach(p => {
                    const opt = document.createElement('option');
                    opt.value = p.name;
                    opt.innerText = p.name; // Could use p.description too as title?
                    opt.title = p.description;
                    patternSelect.appendChild(opt);
                });
            }

        } else if (header.type === "Ok") {
            // Silent No-Op (e.g. from GetState on a missing snapshot)
            pendingRequest = false;
            nextRequestPending = false;
        } else if (header.type === "BinaryStateHeader") {
            pendingRequest = false;

            // Binary Payload starts after JSON
            // 4 + jsonLen
            const binaryOffset = 4 + jsonLen;
            const meta = header.payload; // { generation, population, is_running, record_count }

            renderCellsHybrid(meta, view, binaryOffset);

            // If a new snapshot became available while we were waiting, fetch it now
            if (nextRequestPending) {
                nextRequestPending = false;
                requestState();
            }
        } else if (header.type === "Error") {
            pendingRequest = false;
            nextRequestPending = false;
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
        sendRequest("Stop");
    } else {
        sendRequest("Start");
    }
};
stepBtn.onclick = () => sendRequest("NextStep");
resetBtn.onclick = () => sendRequest("Reset");
originBtn.onclick = () => {
    offsetX = 0;
    offsetY = 0;
    if (lastState) renderCellsHybrid(lastState.meta, lastState.dataView, lastState.binaryOffset, true);
    updateInstrumentation();
    requestStateDebounced();
};
quitBtn.onclick = () => {
    sendRequest("Shutdown");
};

patternSelect.onchange = (e) => {
    const target = /** @type {HTMLSelectElement} */ (e.target);
    const pattern = target.value;
    if (pattern) {
        sendRequest("Seed", pattern);
        target.value = "";
    }
};

function updateZoom(delta, mouseX = null, mouseY = null) {
    const oldScale = scale;
    scale = Math.max(1, Math.min(16, scale + delta));
    if (scale === oldScale) return;

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
    requestStateDebounced();
}

zoomInBtn.onclick = () => updateZoom(1);
zoomOutBtn.onclick = () => updateZoom(-1);

window.addEventListener('keydown', (e) => {
    if (e.key === '+' || e.key === '=') updateZoom(1);
    if (e.key === '-') updateZoom(-1);
});

connect();
updateButtonStates();
