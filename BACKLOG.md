# RustyLife Backlog

This document tracks future features, investigations, and known issues that have not yet been scheduled, to ensure cross-computer and cross-user visibility.

## High Priority Bugs
- [x] **Web GUI Idle Viewport Updates:** When panning or zooming the viewport while the simulation engine is paused/idle, the Web GUI does not update the display. This makes it very difficult to see changes to the view position.

## Licensing & Release
- [ ] **GPL-3.0 Licensing:** Implement the GNU General Public License v3.0 text in the repository and add copyright headers (`Copyright (C) 2026 Steven P. Collins. All rights reserved.`) to all source files. Add a rule to rules.md to ensure this is maintained.

## Performance & Telemetry
- [ ] **Fix GPS "Double-Count" Bug:** The current GPS calculation in `engine.rs` reports values nearly 2.0x higher than wall-clock progress.
    - **Root Cause:** `Telemetry::update` is called twice per generation cycle (once at `handle_transition` and once at `capture_state`), with each call counting as a full `1.0` cycle. This includes UI snapshot timing in the telemetry and doubles the effective rate.
    - **Fix Strategy:** Implement a single-timestamp-per-cycle rule. Capture a single timestamp when a `Start` or `Step` task begins, and compute the delta only once at the end of the full cycle in `capture_state`. Use the previous completion time as the next start time to ensure "idle" or snapshot time is properly accounted for in the wall-clock average. **Note:** Operations like the "Deep Memory Compaction Pass" should reset the start timestamp to avoid penalizing the GPS for emergency maintenance work.
- [ ] **Automated Performance Baselines:** Implement automated tracking of performance metrics over long-duration runs. This includes tracking the **minimum and maximum inter-generation intervals** in the logger to detect performance "slips" or environmental hitches.
- [ ] **Timestamp in Crash Report:** Ensure all crash reports (Panic and OOM) include the system wall-clock time. This is critical for identifying exactly when a failure occurred during unattended long-duration runs.

## Engine & Testing Hardening
- [ ] **"Corner Case" Verification Tests:** Implement a battery of tests that specifically verify engine stability for patterns spanning multiple block boundaries (e.g., the 4-corners configuration).
- [ ] **High-Load Test Stability:** Harden the integration test suite (`ipc_integration.rs`) to handle high system load scenarios without timing out or failing due to port contention (e.g., dynamic port allocation, unique target directories).
- [ ] **Windows Suspension Prevention (Advanced):** Investigate `PowerCreateRequest` as a more robust alternative to `SetThreadExecutionState` for preventing system sleep under aggressive power policies.

## Memory Management
- [ ] **Deep Memory Compaction Pass:** Research and implement a "Deep Cleaning" pass for the `BlockArena` and related structures. This passthrough could be triggered by the OOM catcher to reclaim memory from sparse block distributions, potentially involving a pause in simulation and client requests during the compaction cycle.
    - **"Stop the World" design:**
        - **State Transition:** Use a global atomic `SimulationState` (`None`, `ScrubRequested`, `ScrubInProgress`). The first thread to trigger OOM uses CAS to claim "Scrub Leader" status.
        - **Passive Atomic Checkpoints:** Add a `load(Ordering::Relaxed)` check for `ScrubRequested` at the start of every bucket processing loop in `engine.rs`. This provides a zero-cost (~0.00008% overhead) yield point.
        - **Safe Quiescence:** The Scrub Leader waits for the global `in_flight_count` to reach exactly **1** (itself). This naturally accounts for idle threads or threads already in the pool, avoiding "Hard Number" deadlocks.
        - **Selective Queue Purge:** Once quiescence is reached, the Scrub Leader performs a **Selective Purge** of the `WorkQueue`. It removes all existing `SpreadBatch` and `CommitBatch` tokens (which are now stale), but **preserves** high-priority control tokens like `Stop`, `Reset`, or `Seed`.
        - **Parallel Bucket Scrub:** To minimize simulation downtime, the Leader enqueues a `ScrubBucket(idx)` task for each of the 185 buckets and wakes the peer threads into a temporary "Scrub Mode". This ensures the deep memory clean is multi-threaded and completes in a fraction of the time.
        - **Thread Quarantine (The Waiting Room):** Peer threads seeing `ScrubRequested` must decrement their `in_flight_count` *before* parking on the global `MaintenanceCondvar`.
        - **Counter Hygiene:** To prevent wrapped counters, peer threads woken from quarantine MUST skip their normal loop-end decrement since they already "paid" that credit before parking.
        - **Implicit Abort & Full Phase Restart:** Waking from the `MaintenanceCondvar` is the inherent signal that memory layout may have changed. Peer threads immediately discard local phase buffers and return to the pool. To ensure no work is lost, the **Scrub Leader performs a Full Phase Reset**: it clears the current `phase_counter`, resets any partial scratchpad data, and re-enqueues the entire list of generation tasks for the current phase.
        - **Diagnostic Logging:** When `--log` is enabled, the Scrub Leader MUST log the exact system timestamp and generation at the start and stop of the scrub, including the total wall-clock duration of the "Stop the World" pause.
- [ ] **OOM Recovery**: Implement Phase 1 - Global Memory Monitor.
- [ ] **Longevity A/B Testing & Threshold Tuning:** Once the Stop the World OOM recovery is implemented, conduct formal A/B tests to evaluate if we can relax the current "aggressive" 1000-block pruning threshold (which currently delays OOM at the cost of GPS). Determine if the Stop the World safety net allows for a "Lazy Pruning" strategy that prioritizes simulation throughput between emergency scrubs.
    - **Addendum:** As this work happens outside a normal cycle, the engine MUST re-capture the cycle start timestamp before resuming generations to ensure the GPS and Work Rate metrics are not penalized for the duration of the scrub.

## CLI & Integrated Tools
- [x] **Infrastructure**: Generate error/exit for unrecognized or unimplemented CLI switches.
- [ ] **Configurable Diagnostic Intervals:** Add a `--log-interval <MINUTES>` flag to allow customization of the periodic status logging frequency (currently hardcoded to 20m).
- [ ] **Integrated Plotting Capability:** Implement a `--plot` switch for both server and client (and a separate web interface on port 8081). This would allow real-time graphing of simulation metrics (Population, GPS, Work) using free tools like Google Charts. `--gui` and `--plot` should be independently togglable.

## GUI Issues
- [x] **Investigate Native GUI Termination:** Sometimes the Native GUI client (`rustylife-client`) completely terminates the process when the Quit button is clicked, instead of cleanly stopping the server and exiting.
- [x] **Investigate Native GUI Zoom Invariance:** Verify that the zoom-in/out buttons in the native and network GUIs accurately preserve the center coordinates when zooming.
- [ ] **Standardize Zoom Buttons:** Make zoom buttons uniformly use "+" and "-" across both Native/Network and Web GUIs, and add descriptive tooltips to them.
- [ ] **Fix Native GUI Pattern Button Height:** Ensure the "Patterns" dropdown button in the native GUI matches the vertical height of adjacent buttons.
- [ ] **Comprehensive GUI Tooltips:** Ensure all interactive controls (buttons, sliders, etc.) in both the Native and Web GUIs have descriptive tooltips. Many controls currently lack this documentation.
- [x] **Suppress Client Console Spam:** Clean up the network client's request logic to prevent continuous "Server Error: Snapshot for generation 0 not found in memory" messages.
- [ ] **Web GUI Viewport Width Constraints:** Investigate an issue where the simulation canvas/viewport fails to expand to the full width of the screen on certain external displays.
- [ ] **Zone-Based Click-to-Pan:** Implement viewport panning via clicks on defined zones (orthogonal and diagonal).
    - **Behavior:** Discriminate between a single click and a press-and-hold (to preserve existing drag-and-drop). A click in a zone moves the viewport by 1/2 of its dimension in that direction. Diagonal movement results in 1/4 context overlap.

## Architecture & Testing
- [ ] **B-tree/Quad-tree Research:** Discuss and evaluate the potential impact of moving from the current binary-tree implementation to a B-tree structure with four internal nodes (effectively a quad-tree). This research should consider the memory footprint, traversal efficiency for sparse grids, and impact on SIMD-aligned block lookups.
- [ ] **Client Test Coverage Refactoring:** Consider refactoring `rustylife-client/src/main.rs` to extract the `tokio` reading loop into a testable structural component.

## GUI Aesthetics & Layout (Native/Network vs Web Parity)
- [ ] **Fix Native GUI Header Overlap:** Address issues where header elements misalign and overlap when the native/network GUI window is constrained horizontally.
- [ ] **Fix Native GUI Font Sizes:** Increase font sizes in the native/network GUI to improve readability and achieve parity with the Web GUI.
- [ ] **Fix Native GUI Footer Scrolling:** Ensure both metric lines in the native GUI footer can be horizontally scrolled when space is constrained.

## Future Features
- [ ] **Pattern Editor:** Add pattern editor mode.
