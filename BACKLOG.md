# RustyLife Backlog

This document tracks future features, investigations, and known issues that have not yet been scheduled, to ensure cross-computer and cross-user visibility.

## High Priority Bugs
- [x] **Web GUI Idle Viewport Updates:** When panning or zooming the viewport while the simulation engine is paused/idle, the Web GUI does not update the display. This makes it very difficult to see changes to the view position.

## GUI Issues
- [x] **Investigate Native GUI Termination:** Sometimes the Native GUI client (`rustylife-client`) completely terminates the process when the Quit button is clicked, instead of cleanly stopping the server and exiting. Investigate why this behavior is inconsistent across runs.
- [ ] **Investigate Native GUI Zoom Invariance:** Verify that the zoom-in/out buttons in the native and network GUIs accurately preserve the center coordinates when zooming. (This was a bug recently identified and fixed in the Web GUI's `dashboard.js`).
- [ ] **Standardize Zoom Buttons:** Make zoom buttons uniformly use "+" and "-" across both Native/Network and Web GUIs, and add descriptive tooltips to them.
- [ ] **Fix Native GUI Pattern Button Height:** Ensure the "Patterns" dropdown button in the native GUI matches the vertical height of adjacent buttons (like the play controls or Origin button).
- [ ] **Suppress Client Console Spam:** Clean up the network client's request logic to prevent continuous "Server Error: Snapshot for generation 0 not found in memory" messages from spamming the console when the window is scaled or repositioned with no pattern loaded.
- [ ] **Web GUI Viewport Width Constraints:** Investigate an issue where the simulation canvas/viewport fails to expand to the full width of the screen on certain external displays, leaving unrendered margins (approx. 1") on the left and right, even though the header and footer divs correctly utilize the full horizontal resolution.

## Architecture & Testing
- [ ] **Client Test Coverage Refactoring:** Consider refactoring `rustylife-client/src/main.rs` to extract the `tokio` reading loop into a testable structural component (e.g., `ClientIpcAdapter`). This would allow writing integration tests that feed the adapter binary data to simulate server payloads and verify the GUI state updates correctly, completely bypassing the `egui` native windowing requirement.

## Performance & Telemetry
- [ ] **Validate GPS Calculation:** Investigate potential discrepancies in Generations Per Second (GPS) reporting at high population counts (e.g., >1M cells, like Breeder 1 at Gen 28k+). The reported GPS (e.g., 2/S or 8.74/S) sometimes appears significantly faster than the actual observed screen update rate, suggesting the metric might be miscalculated, decoupled from the broadcast frequency, or the GUI might be dropping frames.
- [x] **Crash Telemetry Reporter:** Install a global `std::panic::set_hook` at server startup that intercepts **all** panics. Include a custom global allocator wrapper (`OomTelemetryAllocator`) to ensure telemetry (Generation, Population, Bounds, Rates) is captured even during raw Out-Of-Memory exhaustion before the process natively aborts.

## GUI Aesthetics & Layout (Native/Network vs Web Parity)
- [ ] **Fix Native GUI Header Overlap:** Address issues where header elements (title, play controls, population, etc.) misalign and overlap when the native/network GUI window is constrained horizontally. Implement responsive wrapping/reflowing to keep elements visible without collision, matching the Web GUI's behavior.
- [ ] **Fix Native GUI Font Sizes:** Several elements are rendered too small to be comfortably read in the native/network GUI. Increase the font sizes to improve readability and achieve parity with the Web GUI.
- [ ] **Fix Native GUI Footer Scrolling:** Resolve an issue in the native/network GUI footer where limited horizontal space prevents scrolling for all metric lines. Currently, only one of the two lines gets a scrollable region when space is constrained. Ensure both metric lines can be horizontally scrolled.

## Future Features
- [ ] **Pattern Editor:** Add pattern editor mode.
