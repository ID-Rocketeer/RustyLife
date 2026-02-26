---
description: Run comprehensive project tests (Rust and JS)
---

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
When the user asks to run tests, or you have modified codebase logic (especially frontend HTML/JS/CSS), you MUST run this workflow to ensure nothing was broken.

1. Run Cargo checks, lints, and tests for the backend logic.
```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
```

// turbo-all
2. Run JS tests for Web GUI logic.
```powershell
$env:PATH += ";C:\Program Files\nodejs"
npm.cmd run test --prefix rustylife-server
```
