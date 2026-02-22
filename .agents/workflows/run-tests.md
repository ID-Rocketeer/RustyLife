---
description: Run comprehensive project tests (Rust and JS)
---
When the user asks to run tests, or you have modified codebase logic (especially frontend HTML/JS/CSS), you MUST run this workflow to ensure nothing was broken.

1. Run Cargo tests for the backend logic.
```bash
cargo test
```

// turbo-all
2. Run JS tests for Web GUI logic.
```powershell
$env:PATH += ";C:\Program Files\nodejs"
npm.cmd run test --prefix rustylife-server
```
