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
});
