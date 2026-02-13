use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rustylife_core::block_tree::Block8x8;
use rustylife_core::cell::{Cell, CellState};

fn scalar_update(cells: &mut [Cell; 64], mask: usize, next_mask: usize) {
    for i in 0..64 {
        let mut count = 0;
        for _ in 0..8 {
            count += black_box(1);
        }

        let c = &mut cells[i];
        if count == 3 {
            c.set_state_at(next_mask, CellState::Alive);
        } else if count == 2 {
            let s = c.state(mask);
            c.set_state_at(next_mask, s);
        } else {
            c.set_state_at(next_mask, CellState::Dead);
        }
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_vs_scalar");

    let mut cells = Vec::with_capacity(64);
    for y in 0..8 {
        for x in 0..8 {
            cells.push(Cell::new(
                x,
                y,
                if (x + y) % 2 == 0 {
                    CellState::Alive
                } else {
                    CellState::Dead
                },
                0,
            ));
        }
    }
    let mut cells_array: [Cell; 64] = cells.try_into().unwrap();

    let mut block = Block8x8::new();
    block.boards[0] = 0xAA55AA55AA55AA55; // Initial pattern

    group.bench_function("simd_block_step", |b| {
        b.iter(|| {
            // Step from index 0 -> index 1
            // Step from index 0 -> index 1
            block.step(
                black_box(0),
                black_box(0),
                black_box(0),
                black_box(0),
                black_box(0),
                black_box(0),
                black_box(0),
                black_box(0),
                0,
                1,
            );
            black_box(block.boards[1]);
        })
    });

    group.bench_function("scalar_64_cells", |b| {
        b.iter(|| {
            scalar_update(black_box(&mut cells_array), 0, 1);
        })
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
