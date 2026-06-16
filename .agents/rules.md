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

# RustyLife Project Rules

## Documentation
- **README Maintenance**: Whenever a new command-line switch is added or an existing one is modified in `rustylife-server` or `rustylife-client`, the `README.md` must be updated immediately to reflect the change.
- **Exhaustive help**: Ensure all `clap` arguments have an `about` or `help` string so `--help` is fully descriptive.

## Communication
- **No New Acronyms**: Do not create or use acronyms (e.g., "STW") unless they have been first introduced by the USER. "OOM" and "UI" are exceptions as they are established in the project context.

## Licensing
- **License Maintenance**: All source files (`.rs`, `.js`, `.ts`, etc.) must maintain the GPL-3.0 copyright headers. No new files should be committed without these headers once the licensing task is complete.
    - **Exclusion**: Generated files (e.g., in `rustylife-server/static/types/` via `ts-rs`) are exempt from this requirement.

## Development Workflow (TDD)
- **Strict TDD**: Before implementing any new logic or fixing a bug, a failing test must be written and verified to fail.
- **Engine Change Protocol**: Before undertaking any changes to the core engine logic (`block_tree.rs`, `engine.rs`), we MUST have a deep discussion to verify the rationale for such a change and ensure the current engine behavior is truly inadequate or broken. Do not assume the core logic is at fault for environmental or integration failures.
- **Prove Failure**: The test must be seen to fail either in CI or locally.
- **Wait for Input**: After the test fails, you MUST wait for the user to tell you to proceed.
- **Implement Fix**: Only then implement the logic to make the test pass.
- **Verify**: Run the full test suite (`/run-tests` workflow) to ensure no regressions.
- **Show Results**: Show the passing test result to the user.
- **Commit Approval**: Only commit after the user approves the passing result.
- **Uncommitted Work**: Remind the user about uncommitted work when starting work on a new phase/task. Do not leave the current task in an uncommitted state without explicit permission to pivot.
- **Test Naming**: Long-term tests must be named to describe the *expected behavior* when the test passes (e.g., `test_gps_accuracy.rs`). Names should NOT be based on the failure or bug being reproduced.

## Verification
- **Test Integrity**: All tests must pass before any task is considered complete.
- **Formatting**: `cargo fmt --check` must pass before any commit approval is requested.
- **Linting**: `cargo clippy -- -D warnings` must be run and all warnings addressed before any task is considered complete.
- **System Load Awareness**: In environments with high system load (e.g. running multiple or large simulations), be aware that integration tests with strict timeouts may fail. Favor logical verification over strict timing when possible.
