# RustyLife

A high-performance implementation of John Conway's Game of Life in Rust, featuring a client-server architecture, infinite universe support, and real-time visualization.

## Architecture

- **`rustylife-core`**: The simulation engine (infinite grid, buckets, binary search trees).
- **`rustylife-server`**: The host process handling the simulation and networking (IPC, Web).
- **`rustylife-client`**: A CLI/Terminal client for controlling the simulation.
- **`rustylife-gui`**: A native GUI visualizer (using `egui`).

## Getting Started

### Prerequisites

You need **Rust** installed on your machine.
1.  Go to [rustup.rs](https://rustup.rs/).
2.  Download and run the installer for your OS.
3.  Ensure `cargo` is in your PATH (restart your terminal if needed).

### Setup Instructions

#### Windows 10/11
1.  **Install C++ Build Tools**:
    - Download the "Visual Studio Build Tools" installer from Microsoft.
    - Select the **"Desktop development with C++"** workload.
    - This is required for the MSVC linker used by Rust on Windows.
2.  **Clone the Repository**:
    ```powershell
    git clone https://github.com/ID-Rocketeer/RustyLife.git
    cd RustyLife
    ```

#### Linux (Ubuntu/Debian)
1.  **Install System Dependencies**:
    The GUI component (`eframe` / `winit`) requires several system libraries:
    ```bash
    sudo apt-get update
    sudo apt-get install build-essential pkg-config libssl-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libgtk-3-dev
    ```
2.  **Clone the Repository**:
    ```bash
    git clone https://github.com/ID-Rocketeer/RustyLife.git
    cd RustyLife
    ```

### Building and Running

1.  **Ensure you are on the main branch**:
    ```bash
    git checkout main
    ```

2.  **Run the Server (Simulation Host)**:
    This starts the simulation engine.
    ```bash
    cargo run --release --bin rustylife-server -- --gui
    ```
    *Flags:*
    - `--gui`: Launches the integrated graphical window.
    - `--port <N>`: Sets the web dashboard port (default 8080).

3.  **Run the Client (Optional)**:
    Open a new terminal window to control the server via CLI.
    ```bash
    cargo run --release --bin rustylife-client -- --help
    ```

## Development
- **Run Tests**: `cargo test`
- **Check Lints**: `cargo check`
- **Format Code**: `cargo fmt`
