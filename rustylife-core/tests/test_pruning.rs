use rustylife_core::engine::SimulationEngine;
use rustylife_core::space::SimulationSpace;
use std::sync::Arc;

#[test]
fn test_block_pruning() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space.clone(), 1);

    // Place a glider at (0,0). It will move SE.
    // Block size is 8x8.
    // Glider speed is c/4.
    // To leave the (0,0)-(7,7) block, it needs to travel ~10 units. ~40 gens.
    // But we are using `BlockTree`.
    // Let's check internal block count.

    // Helper to count blocks in all buckets
    let count_blocks = || {
        let mut count = 0;
        for bucket in space.storage().buckets.iter() {
            let tree = bucket.read().unwrap();
            count += tree.arena.nodes.len();
        }
        count
    };

    engine.seed_sync(0, 0, "bob$2bo$3o!".to_string());
    assert!(count_blocks() > 0, "Should have allocated blocks");
    let initial_count = count_blocks();
    println!("Initial blocks: {}", initial_count);

    // Run enough generations for the glider to leave the initial block (e.g. 0,0) totally.
    // Glider at 0,0. Moves +1,+1 every 4 gens.
    // To pass x=8, y=8. Needs > 32 gens.
    // Let's run 60 gens.

    let engine_clone = engine.clone();
    std::thread::spawn(move || {
        SimulationEngine::run_worker(engine_clone, 0);
    });

    engine.step(); // Trigger one step? No, step enqueues 1 step.
    // We want to run many.
    // But `engine.run()` runs until stopped? No, `run_worker` processes queue.
    // We can use `engine.step()` 60 times.

    for _ in 0..60 {
        engine.step();
        while engine.work_queue_in_flight() > 0 {
            std::thread::yield_now();
        }
    }

    // Now glider should be in a new block. Old block (0,0) should be dead.
    // Prune.
    space.prune();

    let final_count = count_blocks();
    println!("Final blocks: {}", final_count);

    // We expect the original block to be removed.
    // However, new blocks were created.
    // The key is that the TOTAL might increase or stay same, but we want to confirm *pruning happened*.
    // Actually, `active_indices` logic in `prune` removes dead blocks.
    // If (0,0) block is permanently dead (all history buffers empty), it MUST be removed.
    // How to verify specifically (0,0) is gone?
    // We can check if `get_cell(0, 0)` still works (it should return Dead).
    // `SparseStorage` internal structure is hidden.
    // But we can check if `final_count` is reasonable or if we can access the tree.

    // Better: Seed a pattern that dies completely.
    // "Die hard"? No, simple 3 cells that die.
    // Then ALL blocks should be pruned.

    // Clear and retry with dying pattern.
    space.clear();
    assert_eq!(count_blocks(), 0);

    // Seed 2 cells that die immediately (under-population)
    engine.place_cell(0, 0);
    engine.place_cell(0, 1);

    assert!(count_blocks() > 0);

    engine.step();
    while engine.work_queue_in_flight() > 0 {
        std::thread::yield_now();
    }

    // Step 2 to ensure history clears (3 buffers!)
    engine.step();
    while engine.work_queue_in_flight() > 0 {
        std::thread::yield_now();
    }
    engine.step();
    while engine.work_queue_in_flight() > 0 {
        std::thread::yield_now();
    }

    space.prune();
    assert_eq!(
        count_blocks(),
        0,
        "All blocks should be pruned as pattern died"
    );
}

#[test]
fn test_pruning_headroom() {
    let space = Arc::new(SimulationSpace::new(rustylife_core::BUCKET_COUNT));
    let engine = SimulationEngine::new(space.clone(), 1);

    // Seed a glider that moves and leaves dead blocks
    engine.seed_sync(0, 0, "bob$2bo$3o!".to_string());

    let engine_clone = engine.clone();
    std::thread::spawn(move || {
        SimulationEngine::run_worker(engine_clone, 0);
    });

    // Run enough steps to create dead blocks but keep some alive
    for _ in 0..40 {
        engine.step();
        while engine.work_queue_in_flight() > 0 {
            std::thread::yield_now();
        }
    }

    // Capture pre-prune state
    let mut pre_prune_len = 0;
    for bucket in space.storage().buckets.iter() {
        let tree = bucket.read().unwrap();
        pre_prune_len += tree.arena.nodes.len();
    }
    assert!(pre_prune_len > 0);

    space.prune();

    let mut post_prune_len = 0;
    let mut post_prune_capacity = 0;

    for bucket in space.storage().buckets.iter() {
        let tree = bucket.read().unwrap();
        let len = tree.arena.nodes.len();
        let cap = tree.arena.nodes.capacity();

        post_prune_len += len;
        post_prune_capacity += cap;

        if len > 0 {
            // Verify headroom
            assert!(
                cap >= len + 1024,
                "Prune did not allocate headroom! Len: {}, Cap: {}",
                len,
                cap
            );
        }
    }

    println!(
        "Pruning verification: Pre-len: {}, Post-len: {}, Post-cap: {}",
        pre_prune_len, post_prune_len, post_prune_capacity
    );

    assert!(
        post_prune_len < pre_prune_len,
        "Pruning should remove dead blocks"
    );
}
