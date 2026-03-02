---
description: Rules for modifying the Client-Server Communication Protocol
---

# Protocol Modification Workflow

Whenever changes are made to the communication protocol (IPC or WebSocket) between `rustylife-server`, `rustylife-gui`, or the Web Dashboard, the following steps **MUST** be adhered to:

### 1. Update the JavaScript Test Harness First
Before finalizing server-side changes, you must update `rustylife-server/static/test_harness.html`. 
- The test harness is the designated Mock Server for the Web GUI.
- It must accurately reflect the new protocol payloads (e.g. `BinaryStateHeader`, `Telemetry`).
- You must manually open `test_harness.html` in a local browser and perform interactive tests (simulating running, stopped, and viewport panning states) to visually verify front-end reactions.

### 2. Update Server Integration Tests
To test the server's response to client commands (which the JS test harness cannot do), you must update or add tests in `rustylife-server/tests/`.
- Integration tests like `ipc_integration.rs` act as a **Mock Client**. 
- They connect to the real server over IPC/TCP, inject requests (like `UpdateViewport`), and assert that the server pushes the correct payloads back.
- If a new client behavior is added, a corresponding mock client test must be written to ensure the server honors the contract.

### 3. Do Not Rely Solely on Static Analysis
Successful compilation and passing unit tests are insufficient for declaring a protocol change "working". 
- Data structure changes (like moving a JSON key) will compile fine in Rust but cause silent `TypeError` panics in the dynamically typed JavaScript client. 
- Always validate the full loop (Harness -> GUI, and Mock Client -> Server) before committing.
