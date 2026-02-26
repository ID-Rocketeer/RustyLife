# RustyLife Project Rules

## Documentation
- **README Maintenance**: Whenever a new command-line switch is added or an existing one is modified in `rustylife-server` or `rustylife-client`, the `README.md` must be updated immediately to reflect the change.
- **Exhaustive help**: Ensure all `clap` arguments have an `about` or `help` string so `--help` is fully descriptive.

## Communication
- **No New Acronyms**: Do not create or use acronyms (e.g., "STW") unless they have been first introduced by the USER. "OOM" is an exception as it is already established in the project context.

## Licensing
- **License Maintenance**: All source files (`.rs`, `.js`, `.ts`, etc.) must maintain the GPL-3.0 copyright headers. No new files should be committed without these headers once the licensing task is complete.

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

## Verification
- **Test Integrity**: All tests must pass before any task is considered complete.
- **System Load Awareness**: In environments with high system load (e.g. running multiple or large simulations), be aware that integration tests with strict timeouts may fail. Favor logical verification over strict timing when possible.
