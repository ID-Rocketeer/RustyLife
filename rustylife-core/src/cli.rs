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

// use clap::CommandFactory; // Removed to fix warning

/// Prints the active configuration to the console in a structured way.
/// This ensures visibility into which flags are actually active.
pub fn print_cli_config<T: clap::Parser + std::fmt::Debug>() {
    let args = T::parse();
    let cmd = T::command();

    println!("--------------------------------------------------");
    println!(
        "{} v{}",
        cmd.get_name(),
        cmd.get_version().unwrap_or("0.1.0")
    );
    println!("Active Configuration:");

    // Use the Debug implementation for high-fidelity output of all fields
    // This ensures we don't "miss" fields if we manually formatted.
    let debug_str = format!("{:#?}", args);
    for line in debug_str.lines().skip(1) {
        // Skip the struct name line
        let line = line.trim_end_matches(',');
        println!("  [CONFIG] {}", line.trim());
    }
    println!("--------------------------------------------------");
}

/// Helper to ensure that if a switch is present, it's NOT silently ignored.
/// This addresses the user concern about "unrecognized switches".
/// Since clap::Parser::parse() already handles unknown switches by exiting,
/// this utility primarily serves to verify that all *defined* switches
/// are printed at startup.
pub fn init_cli<T: clap::Parser + std::fmt::Debug>() -> T {
    let args = T::parse();
    print_cli_args(&args, T::command().get_name());
    args
}

fn print_cli_args<T: std::fmt::Debug>(args: &T, name: &str) {
    println!("--------------------------------------------------");
    println!("Starting {}...", name);
    println!("Active Configuration:");
    let debug_str = format!("{:#?}", args);
    // Remove the first and last lines (struct name and closing brace)
    let lines: Vec<&str> = debug_str.lines().collect();
    if lines.len() > 2 {
        for line in &lines[1..lines.len() - 1] {
            println!("  [CONFIG] {}", line.trim().trim_end_matches(','));
        }
    } else {
        println!("  [CONFIG] (Default)");
    }
    println!("--------------------------------------------------");
}
