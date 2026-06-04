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

import { describe, it, expect, beforeEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';

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
            <button id="color-mode-btn"></button>
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
            beginPath: vi.fn(),
            rect: vi.fn(),
            fill: vi.fn(),
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
            payload: { record_count: 1, telemetry: { generation: 1, is_running: false, population: 1 } }
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
        const initialDrawCount = mockCtx.fill.mock.calls.length;
        expect(initialDrawCount).toBeGreaterThan(0);

        // 2. Simulate panning while paused
        const canvas = document.getElementById('sim-canvas');
        canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: 0, clientY: 0 }));
        window.dispatchEvent(new MouseEvent('mousemove', { clientX: 10, clientY: 10 }));
        window.dispatchEvent(new MouseEvent('mouseup'));

        // Panning should have triggered another renderCellsHybrid call which should call fill again
        // Due to the bug, it aborts immediately and never repaints!
        expect(mockCtx.fill.mock.calls.length).toBeGreaterThan(initialDrawCount);
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
            type: "BinaryStateHeader",
            payload: { record_count: 0, telemetry: { generation: 1 } }
        });
        const bytes1 = new TextEncoder().encode(msg1);
        const buf1 = new ArrayBuffer(4 + bytes1.length);
        new DataView(buf1).setUint32(0, bytes1.length, true);
        new Uint8Array(buf1).set(bytes1, 4);
        wsInstance.onmessage({ data: buf1 });

        // 3. Simulate "Reset" (BinaryStateHeader with gen 0)
        const msg0 = JSON.stringify({
            type: "BinaryStateHeader",
            payload: { record_count: 0, telemetry: { generation: 0 } }
        });
        const bytes0 = new TextEncoder().encode(msg0);
        const buf0 = new ArrayBuffer(4 + bytes0.length);
        new DataView(buf0).setUint32(0, bytes0.length, true);
        new Uint8Array(buf0).set(bytes0, 4);
        wsInstance.onmessage({ data: buf0 });

        // 4. Verify center is preserved (THIS SHOULD FAIL)
        expect(centerEl.innerHTML).toBe(pannedCenter);
    });

    it('should toggle classic B&W mode and change cell fill colors', async () => {
        await import('../static/dashboard.js');
        await new Promise(r => setTimeout(r, 10));

        const colorModeBtn = document.getElementById('color-mode-btn');
        // Initial mode is Tri-State: button has label 'Tri-State' and is selected
        expect(colorModeBtn.classList.contains('selected')).toBe(true);
        expect(colorModeBtn.innerText).toBe('Tri-State');

        // Define a state helper with 1 cell at coordinate (10, 10)
        const makeStatePacket = (cellState) => {
            const metaStr = JSON.stringify({
                type: "BinaryStateHeader",
                payload: { record_count: 1, telemetry: { generation: 1, is_running: false, population: 1 } }
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
            dv.setUint8(cellOffset + 32, cellState);
            return buffer;
        };

        // Render first time in Tri-State mode: state 4 (Born) -> Blue (#0000FF)
        wsInstance.onmessage({ data: makeStatePacket(4) });
        await new Promise(r => setTimeout(r, 10));
        expect(mockCtx.fillStyle).toBe('#0000FF');

        // Toggle to Classic mode
        colorModeBtn.click();
        expect(colorModeBtn.classList.contains('selected')).toBe(true);
        expect(colorModeBtn.innerText).toBe('Classic');

        // Classic mode rerenders cell with state 4 (currently alive) -> Green (#00FF00)
        expect(mockCtx.fillStyle).toBe('#00FF00');

        // Toggle to Bi-State mode
        colorModeBtn.click();
        expect(colorModeBtn.classList.contains('selected')).toBe(false);
        expect(colorModeBtn.innerText).toBe('Bi-State');

        // Bi-State mode rerenders cell with state 4 (Born) -> Blue (#0000FF)
        expect(mockCtx.fillStyle).toBe('#0000FF');

        // Toggle to Tri-State mode
        colorModeBtn.click();
        expect(colorModeBtn.classList.contains('selected')).toBe(true);
        expect(colorModeBtn.innerText).toBe('Tri-State');

        // Tri-state mode with state 2 (Dying) -> Orange (#FF8000)
        wsInstance.onmessage({ data: makeStatePacket(2) });
        await new Promise(r => setTimeout(r, 10));
        expect(mockCtx.fillStyle).toBe('#FF8000');

        // Toggle back to Classic mode
        colorModeBtn.click();
        expect(colorModeBtn.classList.contains('selected')).toBe(true);
        expect(colorModeBtn.innerText).toBe('Classic');

        // Classic mode: send cell with state 4 -> Green (#00FF00)
        wsInstance.onmessage({ data: makeStatePacket(4) });
        await new Promise(r => setTimeout(r, 10));
        expect(mockCtx.fillStyle).toBe('#00FF00');
    });

    describe('Layout Width Constraints', () => {
        it('should not have a fixed max-width on .container in index.html to allow full-width scaling', () => {
            const filePath = path.resolve(__dirname, '../static/index.html');
            const content = fs.readFileSync(filePath, 'utf8');

            const styleMatch = content.match(/<style>([\s\S]*?)<\/style>/);
            expect(styleMatch).not.toBeNull();
            const styleCss = styleMatch[1];

            const containerRuleMatch = styleCss.match(/\.container\s*\{([\s\S]*?)\}/);
            expect(containerRuleMatch).not.toBeNull();
            const containerCss = containerRuleMatch[1];

            // Should not contain max-width limiting to pixels
            expect(containerCss).not.toContain('max-width:');
            
            // Should be set to full width
            expect(containerCss).toContain('width: 100%');
        });

        it('should not have a fixed max-width on .container in telemetry.html to allow full-width scaling', () => {
            const filePath = path.resolve(__dirname, '../static/telemetry.html');
            const content = fs.readFileSync(filePath, 'utf8');

            const styleMatch = content.match(/<style>([\s\S]*?)<\/style>/);
            expect(styleMatch).not.toBeNull();
            const styleCss = styleMatch[1];

            const containerRuleMatch = styleCss.match(/\.container\s*\{([\s\S]*?)\}/);
            expect(containerRuleMatch).not.toBeNull();
            const containerCss = containerRuleMatch[1];

            // Should not contain max-width limiting to pixels
            expect(containerCss).not.toContain('max-width:');
            
            // Should be set to full width
            expect(containerCss).toContain('width: 100%');
        });
    });
});
