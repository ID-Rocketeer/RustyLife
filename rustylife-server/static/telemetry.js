// @ts-nocheck
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

import { encodeRequest } from './protocol.js';

const statusDot = /** @type {HTMLDivElement} */ (document.getElementById('status-dot'));
const statusText = /** @type {HTMLSpanElement} */ (document.getElementById('status-text'));

let socket = null;

// Telemetry graph state
let chartPop = null;
let chartGps = null;
let chartPopOptions = {};
let chartGpsOptions = {};
let telemetryBuffer = [];
let lastChartDrawTs = 0;

if (window.google) {
    google.charts.load('current', { 'packages': ['corechart', 'gauge'] });
    google.charts.setOnLoadCallback(initCharts);
}

function initCharts() {
    chartPop = new google.visualization.AreaChart(document.getElementById('chart-population'));
    chartGps = new google.visualization.Gauge(document.getElementById('chart-gps'));

    const commonOpts = {
        backgroundColor: 'transparent',
        titleTextStyle: { color: '#9ca3af', fontSize: 13, bold: true },
        legend: { position: 'none' },
        hAxis: {
            textPosition: 'out',
            textStyle: { color: '#6b7280', fontSize: 10 },
            gridlines: { color: 'rgba(255, 255, 255, 0.05)', minSpacing: 50 },
            baselineColor: 'transparent',
            format: 'HH:mm:ss' // Show hours, minutes, seconds explicitly
        },
        vAxis: {
            textStyle: { color: '#6b7280', fontSize: 10 },
            gridlines: { color: 'rgba(255, 255, 255, 0.05)' },
            minorGridlines: { color: 'transparent' },
            baselineColor: 'rgba(255, 255, 255, 0.2)'
        },
        chartArea: { left: 60, top: 30, right: 20, bottom: 40, width: '100%', height: '100%' },
        animation: { duration: 0 }
    };

    chartPopOptions = {
        ...commonOpts,
        title: 'Population over Time',
        colors: ['#10b981'] // Emerald 
    };

    chartGpsOptions = {
        min: 0,
        max: 7, // 10^7 = 10,000,000 GPS
        majorTicks: ['0', '1', '2', '3', '4', '5', '6', '7'],
        minorTicks: 2,
        animation: { duration: 0 } // Stop gauge bouncing on reconnect
    };

    // Start drawing loop only after charts are init
    requestAnimationFrame(uiLoop);
}

function uiLoop() {
    const now = performance.now();
    if (now - lastChartDrawTs > 100) { // Throttle chart redraws to ~10 FPS
        drawCharts();
        lastChartDrawTs = now;
    }
    requestAnimationFrame(uiLoop);
}

function updateTelemetry(meta) {
    if (!meta) return;

    const ts = Number(meta.timestamp);
    if (!isNaN(ts) && meta.population !== undefined && meta.work_rate !== undefined) {
        telemetryBuffer.push({
            timestamp: ts,
            date: new Date(ts),
            population: Number(meta.population),
            workRate: Number(meta.work_rate),
            gps: Number(meta.gps || 0)
        });
    }
}

function drawCharts() {
    if (!chartPop || !chartGps || telemetryBuffer.length === 0) return;

    // --- Draw GPS Gauge (Persists last known value even if buffer empties briefly) ---
    if (telemetryBuffer.length > 0) {
        const latestGps = telemetryBuffer[telemetryBuffer.length - 1].gps;
        const logGps = Math.log10(Math.max(1, latestGps));
        const gpsData = google.visualization.arrayToDataTable([
            ['Label', 'Value'],
            ['Log GPS', { v: logGps, f: latestGps.toFixed(0) }]
        ]);
        chartGps.draw(gpsData, chartGpsOptions);
    }

    // --- Draw Area Charts ---
    const container = document.getElementById('chart-population');
    if (!container || container.clientWidth === 0) return;

    const timeWindowMs = (container.clientWidth / 100) * 10000;
    const latestTs = telemetryBuffer[telemetryBuffer.length - 1].timestamp;
    const cutoffTs = latestTs - timeWindowMs;

    let splitIdx = 0;
    while (splitIdx < telemetryBuffer.length && telemetryBuffer[splitIdx].timestamp < cutoffTs) {
        splitIdx++;
    }
    if (splitIdx > 0) {
        telemetryBuffer = telemetryBuffer.slice(splitIdx);
    }

    if (telemetryBuffer.length < 2) return; // Area chart needs at least 2 points

    const popData = new google.visualization.DataTable();
    popData.addColumn('datetime', 'Time');
    popData.addColumn('number', 'Population');

    const popRows = [];
    for (const pt of telemetryBuffer) {
        popRows.push([pt.date, pt.population]);
    }

    popData.addRows(popRows);
    chartPop.draw(popData, chartPopOptions);
}

function sendRequest(type, payload = null) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(encodeRequest(type, payload));
}

function connect() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';

    // Connect to port 8086 instead of the main 8080 or window.location.port
    // Unless we are inside the test harness which sets window.mockWs and operates via 8081 usually,
    // wait, test_harness mocks `WebSocket` globally so the URL doesn't actually matter for the mock.
    // But for production, we want 8086.
    const host = window.location.host || 'localhost:8086';
    socket = new WebSocket(`${protocol}//${host}/ws`);
    socket.binaryType = 'arraybuffer';

    socket.onopen = () => {
        statusDot.classList.add('connected');
        statusText.innerText = 'Connected';
    };

    socket.onmessage = (event) => {
        const rawData = event.data; // ArrayBuffer
        const view = new DataView(rawData);

        if (rawData.byteLength < 4) return;
        const jsonLen = view.getUint32(0, true);

        if (rawData.byteLength < 4 + jsonLen) return;
        const jsonBytes = new Uint8Array(rawData, 4, jsonLen);
        const jsonStr = new TextDecoder().decode(jsonBytes);

        let header;
        try {
            header = JSON.parse(jsonStr);
        } catch (e) {
            console.error("Failed to parse JSON header:", e);
            return;
        }

        if (header.type === "Welcome") {
            // Initiate Push Protocol Handshake for Metrics Only
            sendRequest("HandshakeMetricsOnly");
            console.log("Telemetry received Welcome, sent HandshakeMetricsOnly");

        } else if (header.type === "SnapshotAvailable") {
            const telemetry = header.payload ? header.payload.telemetry : header.telemetry;
            updateTelemetry(telemetry);

            // Acknowledge the frame to request the next one
            sendRequest("AckPreviousFrame");
        } else if (header.type === "TelemetryBundle") {
            const bundle = header.payload ? header.payload.telemetry : header.telemetry;
            for (const t of bundle) {
                updateTelemetry(t);
            }
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

// Start connection
connect();
