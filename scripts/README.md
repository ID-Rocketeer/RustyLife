<!--
Copyright (C) 2026 Steven P. Collins. All rights reserved.

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
-->

# Windows Service Daemon & Task Setup

This directory contains the daemon loop script for running the RustyLife server as a persistent background task on Windows 11 with instant recovery on crashes.

## Daemon Loop (`run_rustylife.ps1`)

The [run_rustylife.ps1](run_rustylife.ps1) script provides native, seconds-level recovery without requiring third-party tools like NSSM or WinSW. It runs the server synchronously, captures all stdout/stderr output into a single log file (`logs/server_console.log`), and automatically restarts it within 2 seconds if it terminates (e.g. due to Out-Of-Memory under heavy Breeder runs).

## Task Scheduler Configuration

To configure Windows Task Scheduler to launch the script automatically at startup:

1. Open **Task Scheduler** (`taskschd.msc`) as an Administrator.
2. Click **Create Task...** (not Basic Task) in the right-hand actions pane.
3. **General Tab**:
   - **Name**: `RustyLife Monitor`
   - Select **Run whether user is logged on or not**.
4. **Triggers Tab**:
   - Click **New...**
   - Set **Begin the task** to **At startup** (or **At log on**).
5. **Actions Tab**:
   - Click **New...**
   - Set **Action** to **Start a program**.
   - **Program/script**: `powershell.exe`
   - **Add arguments**: `-WindowStyle Hidden -ExecutionPolicy Bypass -File "<Path_To_RustyLife_Checkout>\scripts\run_rustylife.ps1"`
6. **Settings Tab**:
   - Uncheck **Stop the task if it runs longer than**.
7. Click **OK** and enter your Windows user credentials to save the task.

Once registered, the monitor loop will start silently on startup, keeping the server persistent and logging all telemetry and crash logs to the local log file.

## Antivirus & Heuristic Blocks (e.g., Trend Micro)

When running freshly compiled, unsigned local binaries like `rustylife-server.exe` in the background (especially when triggered via a hidden Task Scheduler process), aggressive antivirus solutions like **Trend Micro** may flag the activity as suspicious and silently block or quarantine the process.

Note that in Trend Micro, standard **Folder Exceptions** only bypass the **Real-Time Signature Scanner** (preventing files from being scanned on read/write). The active process execution is still subject to **Behavioral Monitoring** (which detects actions like a script spawning a binary that binds to network ports). 

To ensure the server is not blocked on reboot or after a recompile, you must configure both types of exceptions:

### 1. Folder Exceptions (Bypasses File Scanning)
1. Open your antivirus control console (e.g., Trend Micro Security).
2. Go to **Settings** -> **Exceptions** -> **Programs/Folders**.
3. Add the folder path where `rustylife-server.exe` is compiled (e.g., the `<Path_To_RustyLife_Checkout>\target\release` directory).

### 2. Behavioral Monitoring / Approved List (Bypasses Runtime Blocking)
If Trend Micro blocks the server even with the folder exclusion active:
1. Open your antivirus console.
2. Go to **Security Report** (or **Security History**) and locate the blocked event for `rustylife-server.exe`.
3. Select the entry and choose **Add to Approved List** (or **Trust this program**).
   *(This tells Trend Micro's behavioral scanner to permanently allow the network connection and execution behaviors of the binary).*

