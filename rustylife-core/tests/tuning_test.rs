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

use rustylife_core::cell::{Cell, CellState};
use rustylife_core::engine::{EngineSubscriber, SimulationEngine};
use rustylife_core::space::SimulationSpace;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
enum BenchmarkStatus {
    Success(Duration),
    BailedOut(f64), // Completion ratio 0.0-1.0
    Timeout,
}

struct GenerationTracker {
    target: usize,
    current: AtomicUsize,
    tx: mpsc::Sender<()>,
    start_time: Instant,
    deadline: Option<Duration>,
}

impl EngineSubscriber for GenerationTracker {
    fn on_snapshot_available(
        &self,
        _data: Arc<Vec<((i128, i128), u8)>>,
        _telemetry: rustylife_core::Telemetry,
    ) -> bool {
        // The `current` field is still used for tracking progress for bail-out calculation,
        // but the primary stop condition now uses `generation`.
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;

        // Dynamic Bail-out Check
        if let Some(deadline) = self.deadline {
            let elapsed = self.start_time.elapsed();
            if elapsed > deadline {
                // If we're significantly behind schedule, abort
                // We don't need to send a message here, the main loop will check the stop signal
                return false;
            }
        }

        if current >= self.target {
            let _ = self.tx.send(());
            return false; // Stop engine
        }
        true
    }
}

pub trait Workload: Send + Sync {
    fn name(&self) -> &str;
    fn setup(&self, space: &SimulationSpace);
    fn target_generations(&self) -> usize;
    fn verify(&self, space: &SimulationSpace);
}

struct RPentomino;
impl Workload for RPentomino {
    fn name(&self) -> &str {
        "R-Pentomino"
    }
    fn setup(&self, space: &SimulationSpace) {
        let guard = space.read();
        let mask = guard.current_state_mask();
        let pattern = vec![(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];
        for (x, y) in pattern {
            space
                .storage()
                .insert(Cell::new(x, y, CellState::Alive, mask));
        }
    }
    fn target_generations(&self) -> usize {
        1103
    }
    fn verify(&self, space: &SimulationSpace) {
        let alive_cells = space.collect_all_states();
        let living_count = alive_cells
            .iter()
            .filter(|(_, view)| (*view & 0b10) != 0)
            .count();
        assert_eq!(living_count, 116);
    }
}

struct StressTest {
    count: usize,
    spacing: i128,
}
impl Workload for StressTest {
    fn name(&self) -> &str {
        "Stress Test (Heavy)"
    }
    fn setup(&self, space: &SimulationSpace) {
        let guard = space.read();
        let mask = guard.current_state_mask();
        for i in 0..self.count {
            for j in 0..self.count {
                let off_x = (i as i128) * self.spacing;
                let off_y = (j as i128) * self.spacing;
                let pattern = vec![
                    (off_x + 1, off_y),
                    (off_x + 2, off_y),
                    (off_x, off_y + 1),
                    (off_x + 1, off_y + 1),
                    (off_x + 1, off_y + 2),
                ];
                for (x, y) in pattern {
                    space
                        .storage()
                        .insert(Cell::new(x, y, CellState::Alive, mask));
                }
            }
        }
    }
    fn target_generations(&self) -> usize {
        1103
    }
    fn verify(&self, space: &SimulationSpace) {
        let alive_cells = space.collect_all_states();
        let living_count = alive_cells
            .iter()
            .filter(|(_, view)| (*view & 0b10) != 0)
            .count();
        let expected = self.count * self.count * 116;
        assert_eq!(living_count, expected);
    }
}

fn run_benchmark(
    pool_size: usize,
    bucket_count: usize,
    workload: &dyn Workload,
    deadline: Option<Duration>,
) -> BenchmarkStatus {
    let space = Arc::new(SimulationSpace::new(bucket_count));
    let engine = SimulationEngine::new(Arc::clone(&space), pool_size);
    workload.setup(&space);

    let (tx, rx) = mpsc::channel();
    let tracker = Arc::new(GenerationTracker {
        target: workload.target_generations(),
        current: AtomicUsize::new(0),
        tx,
        start_time: Instant::now(),
        deadline,
    });

    engine.add_subscriber(tracker.clone());
    let start = Instant::now();
    engine.start();

    // Use a hard wall-clock timeout as a safety net
    let timeout_duration = deadline.map(|d| d * 2).unwrap_or(Duration::from_secs(600));
    let result = rx.recv_timeout(timeout_duration);

    engine.shutdown();
    let elapsed = start.elapsed();

    let gens_done = tracker.current.load(Ordering::SeqCst);
    let ratio = gens_done as f64 / workload.target_generations() as f64;

    if result.is_ok() && gens_done >= workload.target_generations() {
        workload.verify(&space);
        BenchmarkStatus::Success(elapsed)
    } else if result.is_err() && elapsed >= timeout_duration {
        println!(
            "      - [DEBUG] run_benchmark Timeout: result={:?}, elapsed={:?}, timeout={:?}",
            result, elapsed, timeout_duration
        );
        BenchmarkStatus::Timeout
    } else {
        println!(
            "      - [DEBUG] run_benchmark BAILED: result={:?}, gens={}/{}, elapsed={:?}, timeout={:?}",
            result,
            gens_done,
            workload.target_generations(),
            elapsed,
            timeout_duration
        );
        BenchmarkStatus::BailedOut(ratio)
    }
}

struct TuningHistory {
    best_times: HashMap<(usize, usize), Duration>,
    gold_standard: Option<Duration>,
    reference_config: (usize, usize),
}

impl TuningHistory {
    fn new(ref_threads: usize, ref_buckets: usize) -> Self {
        Self {
            best_times: HashMap::new(),
            gold_standard: None,
            reference_config: (ref_threads, ref_buckets),
        }
    }

    fn check_health(&mut self, workload: &dyn Workload) -> bool {
        println!("      - [Sentinel] Running health check...");
        match run_benchmark(
            self.reference_config.0,
            self.reference_config.1,
            workload,
            self.gold_standard.map(|d| d * 115 / 100),
        ) {
            BenchmarkStatus::Success(d) => {
                if self.gold_standard.is_none() {
                    println!(
                        "      - [Sentinel] Established Gold Standard: {:.3}s",
                        d.as_secs_f64()
                    );
                    self.gold_standard = Some(d);
                    true
                } else {
                    let limit = self.gold_standard.unwrap() * 115 / 100;
                    if d > limit {
                        println!(
                            "      - [Sentinel] WARNING: System is slow ({:.3}s > {:.3}s). Cooldown needed.",
                            d.as_secs_f64(),
                            limit.as_secs_f64()
                        );
                        false
                    } else {
                        println!(
                            "      - [Sentinel] System healthy ({:.3}s).",
                            d.as_secs_f64()
                        );
                        true
                    }
                }
            }
            _ => {
                println!(
                    "      - [Sentinel] Health check FAILED (Timed out or Bailed). System compromised."
                );
                false
            }
        }
    }

    fn wait_for_health(&mut self, workload: &dyn Workload) {
        if !self.check_health(workload) {
            println!(
                "      - [Sentinel] Health check failed. Proceeding anyway (Quarantine disabled)."
            );
        }
    }

    fn update_best(&mut self, threads: usize, buckets: usize, time: Duration) {
        let entry = self.best_times.entry((threads, buckets)).or_insert(time);
        if time < *entry {
            if *entry > time * 110 / 100 {
                println!(
                    "      - [History] Significant improvement found for ({}, {}): {:.3}s -> {:.3}s. (Previous run was likely throttled)",
                    threads,
                    buckets,
                    entry.as_secs_f64(),
                    time.as_secs_f64()
                );
            }
            *entry = time;
        }
    }

    fn get_best(&self, threads: usize, buckets: usize) -> Option<Duration> {
        self.best_times.get(&(threads, buckets)).cloned()
    }
}

fn measure_with_history(
    threads: usize,
    buckets: usize,
    workload: &dyn Workload,
    history: &mut TuningHistory,
) -> Duration {
    // 1. Memoization: Return cached result if available
    if let Some(cached) = history.get_best(threads, buckets) {
        return cached;
    }

    // 2. Statistical Sampling: Run multiple times
    let samples = 3;
    let mut successes = Vec::new();
    let mut failures = 0;

    // Use a relatively loose deadline for sampling to avoid false timeouts on noisy machines
    let sample_deadline = history
        .gold_standard
        .map(|d| d * 3)
        .or(Some(Duration::from_secs(60)));

    for _ in 0..samples {
        match run_benchmark(threads, buckets, workload, sample_deadline) {
            BenchmarkStatus::Success(d) => successes.push(d),
            BenchmarkStatus::BailedOut(_) | BenchmarkStatus::Timeout => failures += 1,
        }
        // If majority failed, abort early
        if failures >= 2 {
            break;
        }
    }

    if successes.is_empty() {
        println!(
            "      - [Failure] Configuration ({}, {}) failed consistently ({} attempts).",
            threads, buckets, failures
        );
        history.wait_for_health(workload);
        // Return penalty
        return Duration::from_secs(3600);
    }

    // 3. Outlier Rejection & Averaging
    successes.sort(); // Sort by duration ascending (fastest first)

    let final_time = if successes.len() >= 3 {
        // Discard the slowest run (outlier) and average the rest
        let valid_count = successes.len() - 1;
        let sum: Duration = successes.iter().take(valid_count).sum();
        sum / valid_count as u32
    } else {
        // If only 1 or 2 successes, averge them all
        let sum: Duration = successes.iter().sum();
        sum / successes.len() as u32
    };

    if successes.len() > 1 {
        // Optional: Debug print for samples
        // println!("        Debug: Samples {:?}, Mean {:.3}s", successes, final_time.as_secs_f64());
    }

    history.update_best(threads, buckets, final_time);
    final_time
}

fn run_tuning_process(workload: &dyn Workload) {
    println!(
        "\n--- Starting Health-Aware Auto-Tuning for: {} ---",
        workload.name()
    );
    let mut history = TuningHistory::new(10, rustylife_core::BUCKET_COUNT);
    history.wait_for_health(workload);

    // Stage 1: Thread Optimization
    println!("Stage 1: Finding initial thread pool peak...");
    let best_thread_count =
        tune_threads(2, 256, rustylife_core::BUCKET_COUNT, workload, &mut history);
    println!("Initial optimal thread count: {}", best_thread_count);

    history.wait_for_health(workload);

    // Stage 2: Bucket Count Optimization
    println!("\nStage 2: Finding optimal bucket count...");
    let best_bucket_count = tune_buckets(2, 1024, best_thread_count, workload, &mut history);
    println!("Initial optimal bucket count: {}", best_bucket_count);

    history.wait_for_health(workload);

    // Stage 3: Final Thread Refinement
    println!("\nStage 3: Final thread count refinement...");
    let final_thread_count = tune_threads(2, 256, best_bucket_count, workload, &mut history);
    println!("Final optimal thread count: {}", final_thread_count);

    println!("\n--- Tuning Results for {} ---", workload.name());
    println!("Optimal Threads: {}", final_thread_count);
    println!("Optimal Buckets: {}", best_bucket_count);

    let baseline = measure_with_history(
        rustylife_core::THREAD_POOL_SIZE,
        rustylife_core::BUCKET_COUNT,
        workload,
        &mut history,
    );
    let optimized = measure_with_history(
        final_thread_count,
        best_bucket_count,
        workload,
        &mut history,
    );

    println!(
        "Baseline ({} threads, {} buckets): {:.3}s",
        rustylife_core::THREAD_POOL_SIZE,
        rustylife_core::BUCKET_COUNT,
        baseline.as_secs_f64()
    );
    println!(
        "Optimized ({} threads, {} buckets): {:.3}s",
        final_thread_count,
        best_bucket_count,
        optimized.as_secs_f64()
    );
    println!(
        "Total Speedup: {:.2}x",
        baseline.as_secs_f64() / optimized.as_secs_f64()
    );
}

fn tune_threads(
    min: usize,
    max: usize,
    fixed_buckets: usize,
    workload: &dyn Workload,
    history: &mut TuningHistory,
) -> usize {
    let mut best_t = min;
    let mut min_time = Duration::MAX;

    let mut t = min;
    while t <= max {
        let time = measure_with_history(t, fixed_buckets, workload, history);
        println!("    - Threads {}: {:.3}s", t, time.as_secs_f64());
        if time < min_time {
            min_time = time;
            best_t = t;
        } else if t > 8 && time.as_secs_f64() > min_time.as_secs_f64() * 1.5 {
            break;
        }
        t = if t == 0 { 1 } else { (t * 2).min(max + 1) };
        if t > max {
            break;
        }
    }

    let mut low = (best_t / 2).max(min);
    let mut high = (best_t * 2).min(max);

    while high - low > 2 {
        let m1 = low + (high - low) / 3;
        let m2 = high - (high - low) / 3;
        let m1 = if m1 == low { low + 1 } else { m1 };
        let m2 = if m2 == high { high - 1 } else { m2 };

        let t1 = measure_with_history(m1, fixed_buckets, workload, history);
        let t2 = measure_with_history(m2, fixed_buckets, workload, history);
        println!(
            "    - Probe threads {} ({:.3}s) vs {} ({:.3}s)",
            m1,
            t1.as_secs_f64(),
            m2,
            t2.as_secs_f64()
        );

        if t1 < t2 {
            high = m2;
        } else {
            low = m1;
        }
    }

    let mut best = low;
    let mut min_t = measure_with_history(low, fixed_buckets, workload, history);
    for t in (low + 1)..=high {
        let time = measure_with_history(t, fixed_buckets, workload, history);
        if time < min_t {
            min_t = time;
            best = t;
        }
    }
    best
}

fn tune_buckets(
    min: usize,
    max: usize,
    fixed_threads: usize,
    workload: &dyn Workload,
    history: &mut TuningHistory,
) -> usize {
    let mut best_b = min;
    let mut min_time = Duration::MAX;

    let mut b = min;
    while b <= max {
        let time = measure_with_history(fixed_threads, b, workload, history);
        println!("    - Buckets {}: {:.3}s", b, time.as_secs_f64());
        if time < min_time {
            min_time = time;
            best_b = b;
        }
        if b >= max {
            break;
        }
        b = (b * 2).min(max);
    }

    let mut low = (best_b / 2).max(min);
    let mut high = (best_b * 2).min(max);

    while high - low > 2 {
        let m1 = low + (high - low) / 3;
        let m2 = high - (high - low) / 3;
        let m1 = if m1 == low { low + 1 } else { m1 };
        let m2 = if m2 == high { high - 1 } else { m2 };

        let t1 = measure_with_history(fixed_threads, m1, workload, history);
        let t2 = measure_with_history(fixed_threads, m2, workload, history);
        println!(
            "    - Probe buckets {} ({:.3}s) vs {} ({:.3}s)",
            m1,
            t1.as_secs_f64(),
            m2,
            t2.as_secs_f64()
        );

        if t1 < t2 {
            high = m2;
        } else {
            low = m1;
        }
    }

    let mut result = low;
    let mut min_val = Duration::MAX;
    for b in low..=high {
        let t = measure_with_history(fixed_threads, b, workload, history);
        if t < min_val {
            min_val = t;
            result = b;
        }
    }
    result
}

#[test]
#[ignore]
fn test_tuning_heavy_workload_discovery() {
    run_tuning_process(&StressTest {
        count: 10,
        spacing: 555,
    });
}

#[test]
#[ignore]
fn test_tuning_rpentomino_workload_discovery() {
    run_tuning_process(&RPentomino);
}
