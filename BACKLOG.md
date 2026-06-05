# RustyLife Backlog

This document tracks future features, investigations, and known issues that have not yet been scheduled, to ensure cross-computer and cross-user visibility.

## High Priority Bugs
- [x] **Web GUI Idle Viewport Updates:** When panning or zooming the viewport while the simulation engine is paused/idle, the Web GUI does not update the display. This makes it very difficult to see changes to the view position.

## Licensing & Release
- [x] **GPL-3.0 Licensing:** Implement the GNU General Public License v3.0 text in the repository and add copyright headers (`Copyright (C) 2026 Steven P. Collins. All rights reserved.`) to all source files. Add a rule to rules.md to ensure this is maintained.

## Performance & Telemetry
- [x] **Fix GPS "Double-Count" Bug:** The current GPS calculation in `engine.rs` reports values nearly 2.0x higher than wall-clock progress.
    - *Fix: Refactored `Telemetry` to use a `reset()` method on cycle start, removed redundant updates, and treated Generation 0 as a reset event to ensure accurate initial GPS.*
- [x] **Automated Performance Baselines:** Implement automated tracking of performance metrics over long-duration runs. This includes tracking the **minimum and maximum inter-generation intervals** in the logger to detect performance "slips" or environmental hitches.
- [x] **Timestamp in Crash Report:** Ensure all crash reports (Panic and OOM) include the system wall-clock time. This is critical for identifying exactly when a failure occurred during unattended long-duration runs.

## Engine & Testing Hardening
- [x] **"Corner Case" Verification Tests:** Implement a battery of tests that specifically verify engine stability for patterns spanning multiple block boundaries (e.g., the 4-corners configuration).
- [ ] **High-Load Test Stability:** Harden the integration test suite (`ipc_integration.rs`) to handle high system load scenarios without timing out or failing due to port contention (e.g., dynamic port allocation, unique target directories).
- [x] **Windows Suspension Prevention (Advanced):** Investigate `PowerCreateRequest` as a more robust alternative to `SetThreadExecutionState` for preventing system sleep under aggressive power policies.
- [x] **Spawned Thread Panic Propagation in Tests:** Evaluate all existing unit and integration tests to ensure that any spawned threads (e.g., in concurrent or network test scenarios) explicitly propagate panic messages back to the main thread. Prevent silent failures or generic `unwrap()` failures from obscuring the root cause of test failures.

## Memory Management
- [x] **Parallelize Aggressive Scrubber:** The current `space.prune()` operation runs synchronously on a single thread when `dead_blocks > 1000`, causing a "Stop the World" pause that penalizes GPS. Investigate leveraging the existing worker pool to parallelize this `prune()` operation across the `BlockArena` buckets. (Completed in commit ce275d3)
    - If each of the 185 buckets is protected by its own lock, the `Scrub Leader` could enqueue `Tasks::PruneBucket(idx)` to the `WorkQueue`.
    - This would allow all idle threads to wake up and participate in the deep clean simultaneously, drastically reducing the pause duration and significantly improving overall GPS without risking OS-level OOM aborts.
- [ ] **Longevity A/B Testing & Threshold Tuning:** Once `prune()` is fully parallelized, conduct formal A/B tests to evaluate if we can relax the current "aggressive" 1000-block threshold. Determine if the multi-threaded scrubber is fast enough to allow higher thresholds (e.g., 5000 dead blocks) to prioritize simulation throughput between scrubs without risking memory exhaustion.
    - **Addendum:** As this work happens outside a normal cycle, the engine MUST re-capture the cycle start timestamp before resuming generations to ensure the GPS and Work Rate metrics are not penalized for the duration of the scrub.
- [ ] **Investigate B-Tree Replacement for `BlockArena`:** Explore replacing the custom Binary Search Tree (BST) within the `BlockArena` with a B-Tree to improve memory cache locality and lookup latency during highly active generations.
    - **Need Analysis:** While the `prune()` rebuilds a perfectly balanced BST, deep branches during expansive generations can cause high L1/L2 cache misses as the CPU jumps across the `Vec<BlockNode>`. A B-Tree explicitly aligns node sizes to CPU cache lines (e.g., 64 bytes), dramatically reducing jump latency.
    - **Implementation Check:** This does not require migrating to heap-allocated `Box` pointers; B-Tree nodes can still be tightly packed within the existing index-backed `Vec` architecture. The existing $O(N)$ bulk-rebuild during `prune()` would trivially accommodate recreating the B-Tree structure without implementing complex node-merging/deletion logic.
    - **Measurement Prerequisite:** Before committing to the B-Tree refactor, instrument the engine to empirically validate the latency bottleneck:
        - Track the maximum tree depth during `ensure_block` (e.g., > 30 depth implies extreme branching that a B-Tree would mitigate).
        - Profile the exact execution time of tree navigation (`ensure_block`) versus SIMD bitwise processing (`step()`). If navigation dominates frame time, the B-Tree upgrade is validated.

## CLI & Integrated Tools
- [x] **Infrastructure**: Generate error/exit for unrecognized or unimplemented CLI switches.
- [x] **Configurable Diagnostic Intervals:** Add a `--log-interval <MINUTES>` flag to allow customization of the periodic status logging frequency (currently hardcoded to 20m).
    - *Implementation Drafted: See [implementation_plan_logging_interval.md](file:///c:/Users/spcfo/.gemini/antigravity/brain/3df84e2e-db39-44da-8de7-5519fb0f92c4/implementation_plan_logging_interval.md)*
- [ ] **Integrated Plotting Capability:** Implement a `--plot` switch for both server and client (and a separate web interface on port 8081). This would allow real-time graphing of simulation metrics (Population, GPS, Work) using free tools like Google Charts. `--gui` and `--plot` should be independently togglable.
- [ ] **InteL 808x Port Conventions:** Adopt the Intel 808x processor family nomenclature for RustyLife port selections to standardise ecosystem bindings:
    - **8080**: Primary Dashboard Web Client
    - **8086**: Graphing / Plotting Web Client
    - **8088**: Auxiliary Client / Future Use
    - *Avoid 8081/8082 to preserve the 808x theme where practical.*
- [x] **Plotting Web Client (Port 8086):** Implement an auxiliary Web Client running on port 8086 dedicated to plotting simulation telemetry using Google Charts. 
    - **V1 (Initial Goal):** Render a sliding-window graph of "Population over Time".
    - **V2 (Future Goal):** Expand to support multi-metric simultaneous graphing (e.g., GPS, Work Rate, Net Rate) with user-selectable series. The design will be refined iteratively after V1 is operational.

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
- [x] **Verify Web GUI Control State Logic:** Audit the Web GUI (`dashboard.js`) control enable/disable logic to ensure it matches the behavior recently implemented in the native/network GUI: the Stop button must transition correctly after a stop, the Patterns selector must be disabled while the simulation is running, and Step/Reset must only become active once the engine is fully quiescent. Confirm `is_running` is derived from a reliable, authoritative source (not solely from a snapshot that may be delayed at high GPS).

## Architecture & Testing
- [ ] **Control Plane / Data Plane (CP/DP) Refactoring:** Add a formal split to separate the Control Plane (networking, lifecycle, UI interactions) from the Data Plane (block tree, simulation rendering threads). Waiting on formal design discussion before proceeding.
- [x] **Push-Based Stateful Pub/Sub Protocol:** Replace the client-driven polling `GetState` REST-like architecture with a Stateful Publisher-Subscriber model using connection handshakes (`HandshakeMetricsOnly`, `HandshakeFullSnapshot`). Introduce explicit `ACK` frame pacing and manual viewport overrides to completely eliminate the necessity of the `SnapshotStore` historical ring buffer, reducing structural memory utilization to zero.
- [ ] **B-tree/Quad-tree Research:** Discuss and evaluate the potential impact of moving from the current binary-tree implementation to a B-tree structure with four internal nodes (effectively a quad-tree). This research should consider the memory footprint, traversal efficiency for sparse grids, and impact on SIMD-aligned block lookups.
- [ ] **HashLife / Space Compression Research:** Investigate applying LZ-style memoization to the Quad-tree/Block-tree. Because Conway's Game of Life produces incredibly redundant patterns (e.g., thousands of identical gliders in a Breeder pattern), the data plane could aggressively compress memory by guaranteeing uniquely hashed sub-blocks point to the exact same pointer in memory. (This algorithm, known historically as HashLife, trades CPU cache predictability and pointer chasing for near-infinite compression and temporal skipping, which must be carefully balanced against our existing SIMD brute-force speeds).
- [ ] **Network Payload Compression (Deflate/Zlib):** Evaluate applying standard lossless spatial compression (e.g., Zlib/Deflate over WebSocket per-message compression or gzip for TCP) to the `BinaryPacket` payload. Since the 33-byte `CellRecord` structures generated by redundant patterns (like gliders) have extremely low entropy, compression could drastically reduce network bandwidth. **Prerequisite:** Develop a benchmarking harness to explicitly measure Server CPU compression time + Client CPU decompression time versus the uncompressed network transit time to ensure the bandwidth savings justify the CPU overhead.
- [ ] **Client Test Coverage Refactoring:** Consider refactoring `rustylife-client/src/main.rs` to extract the `tokio` reading loop into a testable structural component.
- [ ] **Clean up CI Runner transition environment variables:** Remove `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24` from `.github/workflows/ci.yml` once GitHub Actions runners drop Node.js 20 completely (expected Fall 2026) and native runner support defaults to Node 24+.

## GUI Aesthetics & Layout (Native/Network vs Web Parity)
- [ ] **Web GUI Fluidity vs Native/Network Parity:** Investigate replacing the HTML5 2D Canvas in `dashboard.js` with WebGL or WebGPU. Currently, panning/zooming in the browser requires JavaScript to manually parse thousands of binary coordinates and issue individual CPU-bound `ctx.fillRect` draw calls per frame, which stutters compared to the GPU-accelerated Native clients. A WebGL implementation could pass the binary `ArrayBuffer` directly into a Vertex Buffer and apply a camera transform matrix via uniforms, achieving true performance parity with Native without JavaScript overhead.
- [ ] **Fix Native GUI Header Overlap:** Address issues where header elements misalign and overlap when the native/network GUI window is constrained horizontally.
- [ ] **Fix Native GUI Font Sizes:** Increase font sizes in the native/network GUI to improve readability and achieve parity with the Web GUI.
- [ ] **Fix Native GUI Footer Scrolling:** Ensure both metric lines in the native GUI footer can be horizontally scrolled when space is constrained.

## Future Features
- [ ] **Pattern Editor:** Add pattern editor mode.
