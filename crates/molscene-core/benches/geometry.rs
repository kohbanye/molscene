//! End-to-end geometry benchmark: building a sticks `GeometrySpec` runs bond
//! perception (`perceive` → `bonds`) plus tessellation, so this measures the
//! whole pipeline, not just the grid. Run with `cargo bench -p molscene-core`.

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use molscene_core::{Expr, Scene, Source, Style};

#[path = "common.rs"]
mod common;
use common::{lattice, lattice_jittered};

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

/// Several spatial selections in one scene: each `within`/`around` node used to
/// rebuild the k-d tree from scratch. `EvalCtx` now builds it once per compile,
/// so this measures the shared-tree win across representations.
fn bench_spatial(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometry_spatial");
    for &n in &[1_000usize, 10_000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut scene = Scene::new(Source::InlinePdb {
                        data: String::new(),
                    })
                    .with_structure(lattice_jittered(n));
                    // A spread of spatial selections, each of which would
                    // otherwise rebuild the whole-structure k-d tree.
                    // A small seed (a few atoms) so each spatial node's cost is
                    // dominated by the one-time, whole-structure tree build —
                    // the redundant rebuild `EvalCtx` now elides.
                    let seed = Expr::resi(1, 3);
                    scene.spheres(Expr::Within(8.0, Box::new(seed.clone())), Style::default());
                    scene.spheres(Expr::Around(6.0, Box::new(seed.clone())), Style::default());
                    scene.spheres(Expr::Expand(4.0, Box::new(seed.clone())), Style::default());
                    scene.spheres(Expr::Beyond(5.0, Box::new(seed)), Style::default());
                    scene
                },
                |scene| black_box(scene.to_geometry()),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_geometry, bench_spatial);
criterion_main!(benches);
