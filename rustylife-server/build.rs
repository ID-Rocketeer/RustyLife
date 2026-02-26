// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![allow(clippy::collapsible_if)]

fn main() {
    // Rerun this script if the patterns directory changes
    println!("cargo:rerun-if-changed=../rustylife-core/src/patterns");

    let source_dir = std::path::Path::new("../rustylife-core/src/patterns");

    // We want to copy to the directory where the binary is located,
    // which is usually distinct from OUT_DIR (which is for intermediate build artifacts).
    // However, finding the actual binary output path from build.rs is tricky.
    // A common reliable pattern is to copy to OUT_DIR and then have the *runtime*
    // maybe look there, but the user wants them next to the binary.

    // Strategy: Copy to a 'patterns' directory in target/debug (or release)
    // We can infer the profile target dir from OUT_DIR somewhat, but it's brittle.
    // BETTER: Copy to OUT_DIR, and code the server to look in OUT_DIR?
    // NO, user wants deployment simplicity (copy exe + folder).

    // Alternative: We interpret "next to binary" as the runtime Current Working Directory
    // when running `cargo run`.
    // But `cargo run` sets CWD to the workspace root.

    // Let's implement the copy logic to `target_dir/patterns`.
    // We can try to find the "target" folder by traversing up from OUT_DIR.

    // build.rs execution CWD is the package root (rustylife-server).

    // Let's iterate and copy files.
    // NOTE: Creating files outside of OUT_DIR is technically frowned upon in build scripts
    // but necessary for this specific user requirement of "folder next to binary"
    // unless we use `cargo make` or similar external tools.
    // We will copy them to `../../target/debug/patterns` if we can guess it,
    // or just assume the user accepts a copy in `rustylife-server/patterns`?
    // No, user hated that.

    // Best effort approach for "native" feel:
    // Copy to the profile output directory.
    let dest_path = std::path::PathBuf::from("../target")
        .join(std::env::var("PROFILE").unwrap())
        .join("patterns");

    if dest_path.exists() {
        // Clean up stale patterns in the destination
        if let Ok(dest_entries) = std::fs::read_dir(&dest_path) {
            for entry in dest_entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("rle") {
                    if let Some(file_name) = path.file_name() {
                        let source_file = source_dir.join(file_name);
                        if !source_file.exists() {
                            println!("cargo:warning=Removing stale pattern: {:?}", file_name);
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            }
        }
    } else {
        std::fs::create_dir_all(&dest_path).expect("Failed to create target patterns directory");
    }

    if source_dir.exists() && source_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(source_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("rle") {
                    let file_name = path.file_name().unwrap();
                    let dest_file = dest_path.join(file_name);
                    std::fs::copy(&path, &dest_file).expect("Failed to copy pattern");
                }
            }
        }
    }
}
