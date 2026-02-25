import { describe, it, expect, beforeEach, vi } from 'vitest';

describe('Dashboard Panning & Zooming', () => {
    let mockCtx;
    let wsInstance;

    beforeEach(async () => {
        document.body.innerHTML = `
            <div id="count"></div><div id="generation"></div>
            <button id="play-pause-btn"></button><button id="step-btn"></button>
            <button id="reset-btn"></button><button id="origin-btn"></button>
            <button id="quit-btn"></button><select id="pattern-select"></select>
            <button id="zoom-in-btn"></button><button id="zoom-out-btn"></button>
            <div id="extent-display"></div><div id="center-display"></div>
            <div id="bounds-display"></div><div id="expanse-display"></div>
            <div id="zoom-display"></div><div id="work-display"></div>
            <div id="net-display"></div><div id="gps-display"></div>
            <div id="cores-display"></div><div id="status-dot"></div>
            <div id="status-text"></div>
            <canvas id="sim-canvas" width="800" height="600"></canvas>
        `;

        mockCtx = {
            fillRect: vi.fn(),
            fillStyle: '',
            clearRect: vi.fn()
        };
        HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);

        global.WebSocket = class {
            constructor() {
                wsInstance = this;
                setTimeout(() => { if (this.onopen) this.onopen(); }, 0);
            }
            send() { }
            close() { }
        };
        vi.resetModules();
    });

    it('should redraw the screen when panning while paused', async () => {
        await import('../static/dashboard.js');
        await new Promise(r => setTimeout(r, 10));

        const metaStr = JSON.stringify({
            type: "BinaryStateHeader",
            payload: { generation: 1, record_count: 1, is_running: false, population: 1 }
        });

        const metaBytes = new TextEncoder().encode(metaStr);
        const buffer = new ArrayBuffer(4 + metaBytes.length + 33);
        const dv = new DataView(buffer);
        const u8 = new Uint8Array(buffer);

        dv.setUint32(0, metaBytes.length, true);
        u8.set(metaBytes, 4);
        const cellOffset = 4 + metaBytes.length;
        dv.setBigInt64(cellOffset, 10n, true);
        dv.setBigInt64(cellOffset + 16, 10n, true);
        dv.setUint8(cellOffset + 32, 0b11);

        // 1. Send this through WebSocket
        wsInstance.onmessage({ data: buffer });
        await new Promise(r => setTimeout(r, 10));

        // Verify it rendered the initial state
        const initialDrawCount = mockCtx.fillRect.mock.calls.length;
        expect(initialDrawCount).toBeGreaterThan(0);

        // 2. Simulate panning while paused
        const canvas = document.getElementById('sim-canvas');
        canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: 0, clientY: 0 }));
        window.dispatchEvent(new MouseEvent('mousemove', { clientX: 10, clientY: 10 }));
        window.dispatchEvent(new MouseEvent('mouseup'));

        // Panning should have triggered another renderCellsHybrid call which should call fillRect again
        // Due to the bug, it aborts immediately and never repaints!
        expect(mockCtx.fillRect.mock.calls.length).toBeGreaterThan(initialDrawCount);
    });

    it('should NOT reset viewport center when generation resets to 0', async () => {
        await import('../static/dashboard.js');
        await new Promise(r => setTimeout(r, 10));

        // 1. Simulate panning both X and Y
        const canvas = document.getElementById('sim-canvas');
        canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: 0, clientY: 0 }));
        window.dispatchEvent(new MouseEvent('mousemove', { clientX: 100, clientY: 100 }));
        window.dispatchEvent(new MouseEvent('mouseup'));

        const centerEl = document.getElementById('center-display');
        const pannedCenter = centerEl.innerHTML;
        // The default is [ +000000000 , +000000000 ]. 
        // Panning by 100,100 with scale 4 should result in cx=-25, cy=25
        expect(pannedCenter).not.toContain('+000000000'); // Now it should fail to contain it in both parts

        // 2. Setup currentGen > 0
        const msg1 = JSON.stringify({
            type: "SnapshotAvailable",
            payload: { telemetry: { generation: 1 } }
        });
        const bytes1 = new TextEncoder().encode(msg1);
        const buf1 = new ArrayBuffer(4 + bytes1.length);
        new DataView(buf1).setUint32(0, bytes1.length, true);
        new Uint8Array(buf1).set(bytes1, 4);
        wsInstance.onmessage({ data: buf1 });

        // 3. Simulate "Reset" (SnapshotAvailable with gen 0)
        const msg0 = JSON.stringify({
            type: "SnapshotAvailable",
            payload: { telemetry: { generation: 0 } }
        });
        const bytes0 = new TextEncoder().encode(msg0);
        const buf0 = new ArrayBuffer(4 + bytes0.length);
        new DataView(buf0).setUint32(0, bytes0.length, true);
        new Uint8Array(buf0).set(bytes0, 4);
        wsInstance.onmessage({ data: buf0 });

        // 4. Verify center is preserved (THIS SHOULD FAIL)
        expect(centerEl.innerHTML).toBe(pannedCenter);
    });
});
