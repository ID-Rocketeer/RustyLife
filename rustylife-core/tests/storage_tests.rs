use rustylife_core::cell::{Cell, CellState};
use rustylife_core::hash::hash_coordinates;
use rustylife_core::space::SimulationSpace;
use std::collections::HashSet;

#[test]
fn test_storage_correctly_manages_hash_collisions() {
    let space = SimulationSpace::new(rustylife_core::BUCKET_COUNT);
    let guard = space.read();
    let mask = guard.current_state_mask();

    // Find two different coordinate pairs that collide into the same bucket.
    // Since we have BUCKET_COUNT = 256, we shouldn't have to look far.
    let mut collision_pairs = Vec::new();
    let mut buckets_found = std::collections::HashMap::new();

    for x in 0..1000 {
        let h = hash_coordinates(x, 0, rustylife_core::BUCKET_COUNT);
        if let Some(prev_x) = buckets_found.insert(h, x) {
            collision_pairs.push((prev_x, x, h));
            break;
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
    bucket.collect_all(mask, 0, 0, &mut all);

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
    let result = space.storage().find_or_create_and_apply(
        x,
        y,
        || Cell::new(x, y, CellState::Dead, mask),
        |c| c.state(mask),
    );
    assert_eq!(result, CellState::Dead);

    // 2. Find it and change it
    space
        .storage()
        .find_and_apply(x, y, |c| c.set_state_at(mask, CellState::Alive));

    // 3. find_or_create should now find it alive, NOT recreate it
    let result_after = space.storage().find_or_create_and_apply(
        x,
        y,
        || panic!("Should not be called!"),
        |c| c.state(mask),
    );
    assert_eq!(result_after, CellState::Alive);
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
