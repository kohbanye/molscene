//! Benchmarks the distance-based bond inference (the cell grid) across a range
//! of structure sizes. Run with `cargo bench -p molscene-core`.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

#[path = "common.rs"]
mod common;
use common::lattice;

fn bench_bonds(c: &mut Criterion) {
    let mut group = c.benchmark_group("bond_inference");
    for &n in &[100usize, 1_000, 10_000, 100_000] {
        let structure = lattice(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &structure, |b, s| {
            b.iter(|| black_box(s.bonds()));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_bonds);
criterion_main!(benches);
