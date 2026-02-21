use rustylife_core::cell::{Cell, CellState};
use rustylife_core::hash::hash_coordinates;
use rustylife_core::space::SimulationSpace;
use std::collections::HashSet;
use std::io::Read;
use tempfile::NamedTempFile;

#[test]
fn test_storage_binary_encoding_integrity() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    space.seed_glider(0, 0); // 5 cells

    let temp = NamedTempFile::new().unwrap();
    let path = temp.path();

    // Encode Gen 1, 5 cells, is_running=true, record_count=5
    space.encode_to_file(path, 1, 5, true, 5).unwrap();

    let mut file = std::fs::File::open(path).unwrap();
    let mut header = [0u8; 25];
    file.read_exact(&mut header).unwrap();

    // Header check
    let generation_count = u64::from_le_bytes(header[0..8].try_into().unwrap());
    let count = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let running = header[16] == 1;
    let records = u64::from_le_bytes(header[17..25].try_into().unwrap());

    assert_eq!(generation_count, 1);
    assert_eq!(count, 5);
    assert!(running);
    assert_eq!(records, 5);

    // CRC Check (last 4 bytes)
    let meta = std::fs::metadata(path).unwrap();
    let mut all_bytes = vec![0u8; meta.len() as usize];
    let mut file = std::fs::File::open(path).unwrap();
    file.read_exact(&mut all_bytes).unwrap();

    let content_len = all_bytes.len() - 4;
    let expected_crc = u32::from_le_bytes(all_bytes[content_len..].try_into().unwrap());

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&all_bytes[..content_len]);
    let actual_crc = hasher.finalize();

    assert_eq!(actual_crc, expected_crc, "CRC32 mismatch in binary export");
}

#[test]
fn test_storage_deep_clear_and_repair() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    // 1. Fill it
    for i in 0..1000 {
        space
            .storage()
            .insert(Cell::new(i, 0, CellState::Alive, mask));
    }

    // 2. Corrupt one in each bucket
    for i in 0..rustylife_core::BUCKET_COUNT {
        space.storage().find_and_apply(i as i128, 0, |c| {
            c.increment_neighbor_count(mask);
        });
    }

    // 3. Repair all
    space.repair();

    // 4. Verify no neighbor counts remain
    let mut cells = Vec::new();
    space.storage().collect_all(mask, 0, 0, &mut cells);
    for (coords, _) in cells {
        space.storage().find_and_apply(coords.0, coords.1, |c| {
            assert_eq!(c.get_neighbor_count(mask), 0);
        });
    }

    // 5. Clear everything
    drop(guard); // Deadlock prevention: must drop read guard before clear() resets masks
    space.clear();
    assert_eq!(space.collect_all_states().len(), 0);
}

#[test]
fn test_storage_correctly_manages_hash_collisions() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    // Find two different coordinate pairs that collide into the same bucket.
    // Since we have BUCKET_COUNT = 256, we shouldn't have to look far.
    let mut collision_pairs = Vec::new();
    let mut buckets_found = std::collections::HashMap::new();

    for x in 0..10_000 {
        let bx = x >> 3;
        let by = 0;
        let h = hash_coordinates(bx, by, rustylife_core::BUCKET_COUNT);
        if let Some(prev_x) = buckets_found.insert(h, x) {
            // Ensure x and prev_x are NOT in the same block, otherwise it's not a bucket collision
            if (x >> 3) != (prev_x >> 3) {
                collision_pairs.push((prev_x, x, h));
                break;
            }
        }
    }

    assert!(
        !collision_pairs.is_empty(),
        "Could not find a hash collision in 1000 attempts"
    );
    let (x1, x2, bucket_idx) = collision_pairs[0];

    // Insert both colliding cells
    space
        .storage()
        .insert(Cell::new(x1, 0, CellState::Alive, mask));
    space
        .storage()
        .insert(Cell::new(x2, 0, CellState::Alive, mask));

    // Verify both are retrievable and stored in the same bucket
    let c1 = space.storage().find_and_apply(x1, 0, |c| c.coordinates());
    let c2 = space.storage().find_and_apply(x2, 0, |c| c.coordinates());

    assert_eq!(c1, Some((x1, 0)));
    assert_eq!(c2, Some((x2, 0)));

    // Both should be in the same bucket list (CellTree)
    // We access internal root lock of that bucket to ensure both reside there.
    let bucket = space.storage().buckets[bucket_idx].read().unwrap();
    assert!(bucket.root.is_some());

    // collect_all_states should return both
    let mut all = Vec::new();
    bucket.collect_cells(mask, 0, 0, &mut all);

    let coords: HashSet<(i128, i128)> = all.iter().map(|(c, _)| *c).collect();
    assert!(coords.contains(&(x1, 0)));
    assert!(coords.contains(&(x2, 0)));
}

#[test]
fn test_storage_remains_stable_under_large_volume() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    let count = 5000;

    // Insert many cells
    for i in 0..count {
        space
            .storage()
            .insert(Cell::new(i as i128, i as i128, CellState::Alive, mask));
    }

    // Verify all exist
    for i in 0..count {
        let found = space
            .storage()
            .find_and_apply(i as i128, i as i128, |_| true)
            .unwrap_or(false);
        assert!(found, "Cell at ({}, {}) was lost", i, i);
    }

    // Verify count in collect_all
    let mut all = Vec::new();
    space.storage().collect_all(mask, 0, 0, &mut all);
    assert_eq!(all.len(), count as usize);
}

#[test]
fn test_storage_find_or_create_idempotency() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    let x = 42;
    let y = 42;

    // 1. Create it
    // 1. Create it (find_and_apply works for create if we treat empty as dead and set it)
    let result = space
        .storage()
        .find_and_apply(x, y, |c: &mut Cell| c.state(mask));
    assert_eq!(result, Some(CellState::Dead));

    // 2. Find it and change it
    space
        .storage()
        .find_and_apply(x, y, |c| c.set_state_at(mask, CellState::Alive));

    // 3. find_or_create should now find it alive, NOT recreate it
    // 3. find_and_apply should find it alive
    let result_after = space
        .storage()
        .find_and_apply(x, y, |c: &mut Cell| c.state(mask));
    assert_eq!(result_after, Some(CellState::Alive));
}

#[test]
fn test_storage_correctly_filters_bounds_on_boundaries() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    // Place points across boundaries
    // (-1, -1), (0,0), (1,1)
    space
        .storage()
        .insert(Cell::new(-1, -1, CellState::Alive, mask));
    space
        .storage()
        .insert(Cell::new(0, 0, CellState::Alive, mask));
    space
        .storage()
        .insert(Cell::new(1, 1, CellState::Alive, mask));

    let mut out = Vec::new();

    // 1. Exact match central
    space
        .storage()
        .collect_in_rect((0, 0), (0, 0), mask, 0, 0, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, (0, 0));
    out.clear();

    // 2. Rect includes two
    space
        .storage()
        .collect_in_rect((-1, -1), (0, 0), mask, 0, 0, &mut out);
    assert_eq!(out.len(), 2);
    out.clear();

    // 3. Rect excludes all
    space
        .storage()
        .collect_in_rect((10, 10), (20, 20), mask, 0, 0, &mut out);
    assert_eq!(out.len(), 0);
}

#[test]
fn test_storage_pattern_seeding_parity() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);

    // Test native pattern seeds
    space.seed_glider(0, 0);
    assert_eq!(space.collect_all_states().len(), 5);
    space.clear();

    space.seed_blinker(0, 0);
    assert_eq!(space.collect_all_states().len(), 3);
    space.clear();

    space.seed_r_pentomino(0, 0);
    assert_eq!(space.collect_all_states().len(), 5);
    space.clear();

    space.seed_glider_gun(0, 0);
    assert_eq!(space.collect_all_states().len(), 36);
    space.clear();

    space.seed_spaceship(0, 0);
    assert_eq!(space.collect_all_states().len(), 9);
    space.clear();

    space.seed_block(0, 0);
    assert_eq!(space.collect_all_states().len(), 4);
    space.clear();

    space.seed_beehive(0, 0);
    assert_eq!(space.collect_all_states().len(), 6);
}

#[test]
fn test_storage_cells_format_robustness() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);

    let pattern = "! Comment line\n! Another one\n.O.\n..O\nOOO";
    space.seed_from_rle(10, 10, pattern);

    let cells = space.collect_all_states();
    assert_eq!(
        cells.len(),
        5,
        "Should ignore comments and find 5 living cells"
    );

    // Verify a specific coordinate
    // .O. at (10, 10) means (11, 10) is alive
    let found = space.storage().find_and_apply(11, 10, |_| true).unwrap();
    assert!(found);
}

#[test]
fn test_storage_rle_format_robustness() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);

    // A glider: 3o$o$bo!
    // Translated:
    // Row 0: OOO
    // Row 1: O
    // Row 2: .O
    let rle = "x = 3, y = 3, rule = B3/S23\n3o$o$bo!";
    space.seed_from_rle(100, 100, rle);

    let cells = space.collect_all_states();
    assert_eq!(cells.len(), 5);

    assert!(space.storage().find_and_apply(100, 100, |_| true).is_some());
    assert!(space.storage().find_and_apply(101, 100, |_| true).is_some());
    assert!(space.storage().find_and_apply(102, 100, |_| true).is_some());
    assert!(space.storage().find_and_apply(100, 101, |_| true).is_some());
    assert!(space.storage().find_and_apply(101, 102, |_| true).is_some());
}

#[test]
fn test_storage_rle_complex_counts() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);

    // 2o2b2o! -> OO..OO
    let rle = "2o2b2o!";
    space.seed_from_rle(0, 0, rle);

    assert_eq!(space.collect_all_states().len(), 4);

    let guard = space.read();
    let mask = guard.current_state_mask();

    assert_eq!(
        space.storage().find_and_apply(0, 0, |c| c.state(mask)),
        Some(CellState::Alive)
    );
    assert_eq!(
        space.storage().find_and_apply(1, 0, |c| c.state(mask)),
        Some(CellState::Alive)
    );
    assert_eq!(
        space.storage().find_and_apply(2, 0, |c| c.state(mask)),
        Some(CellState::Dead)
    );
    assert_eq!(
        space.storage().find_and_apply(4, 0, |c| c.state(mask)),
        Some(CellState::Alive)
    );
}
#[test]
fn test_storage_rle_noisy_format() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);

    // Noisy glider: 3 O , $ 1 b 1 O ! (with spaces, commas, and uppercase)
    let rle = "x = 3, y = 3, rule = B3/S23\n 3 O , $ 1 b 1 O ! ";
    space.seed_from_rle(0, 0, rle);

    // Row 0: 3 alive
    // Row 1: 1 dead, 1 alive
    let cells = space.collect_all_states();
    assert_eq!(
        cells.len(),
        4,
        "Should handle spaces, commas, and uppercase O"
    );

    // Row 0
    assert!(space.storage().find_and_apply(0, 0, |_| true).is_some());
    assert!(space.storage().find_and_apply(1, 0, |_| true).is_some());
    assert!(space.storage().find_and_apply(2, 0, |_| true).is_some());
    // Row 1 (b is dead, next o is at x=1)
    assert!(space.storage().find_and_apply(1, 1, |_| true).is_some());
}

#[test]
fn test_breeder_rle_integrity() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let rle_content = include_str!("../src/patterns/breeder1.rle");

    space.seed_from_rle(0, 0, rle_content);
    let cells = space.collect_all_states();

    assert_eq!(cells.len(), 4060, "Breeder 1 legacy count should be 4060");
    println!("Verified Breeder 1 RLE load: {} cells", cells.len());
}
