//! End-to-end geometry benchmark: building a sticks `GeometrySpec` runs bond
//! perception (`perceive` → `bonds`) plus tessellation, so this measures the
//! whole pipeline, not just the grid. Run with `cargo bench -p molscene-core`.

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use molscene_core::{Expr, Scene, Source, Style};

#[path = "common.rs"]
mod common;
use common::lattice;

fn bench_geometry(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometry_sticks");
    for &n in &[100usize, 1_000, 10_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut scene = Scene::new(Source::InlinePdb {
                        data: String::new(),
                    })
                    .with_structure(lattice(n));
                    scene.sticks(Expr::All, Style::default());
                    scene
                },
                |scene| black_box(scene.to_geometry()),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_geometry);
criterion_main!(benches);
