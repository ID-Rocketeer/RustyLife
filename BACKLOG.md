# RustyLife Backlog

This document tracks future features, investigations, and known issues that have not yet been scheduled, to ensure cross-computer and cross-user visibility.

## GUI Issues
- [ ] **Investigate Native GUI Termination:** Sometimes the Native GUI client (`rustylife-client`) completely terminates the process when the Quit button is clicked, instead of cleanly stopping the server and exiting. Investigate why this behavior is inconsistent across runs.
- [ ] **Investigate Native GUI Zoom Invariance:** Verify that the zoom-in/out buttons in the native and network GUIs accurately preserve the center coordinates when zooming. (This was a bug recently identified and fixed in the Web GUI's `dashboard.js`).

## Architecture & Testing
- [ ] **Client Test Coverage Refactoring:** Consider refactoring `rustylife-client/src/main.rs` to extract the `tokio` reading loop into a testable structural component (e.g., `ClientIpcAdapter`). This would allow writing integration tests that feed the adapter binary data to simulate server payloads and verify the GUI state updates correctly, completely bypassing the `egui` native windowing requirement.

## Performance & Telemetry
- [ ] **Validate GPS Calculation:** Investigate potential discrepancies in Generations Per Second (GPS) reporting at high population counts (e.g., >1M cells, like Breeder 1 at Gen 28k+). The reported GPS (e.g., 2/S or 8.74/S) sometimes appears significantly faster than the actual observed screen update rate, suggesting the metric might be miscalculated, decoupled from the broadcast frequency, or the GUI might be dropping frames.

## GUI Aesthetics & Layout (Native/Network vs Web Parity)
- [ ] **Fix Native GUI Header Overlap:** Address issues where header elements overlap when the native/network GUI window is constrained horizontally. Ensure responsive layout behavior similar to the web GUI.
- [ ] **Fix Native GUI Font Sizes:** Increase the font size of overly small elements in the native/network GUI to improve readability and match the aesthetics of the web GUI.
- [ ] **Fix Native GUI Footer Scrolling:** Resolve an issue in the native/network GUI footer where limited horizontal space prevents scrolling for all metric lines. Ensure both metric lines can be navigated when cramped.
