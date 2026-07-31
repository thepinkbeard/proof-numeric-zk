use criterion::{black_box, criterion_group, criterion_main, Criterion};
use numeric_core::{LimbNumber, RoundingMode};

fn multiplication(c: &mut Criterion) {
    let a = LimbNumber::<16, 4>::new([1000, 2000, 3000, 4000], false).unwrap();
    let b = LimbNumber::<16, 4>::new([4000, 3000, 2000, 1000], false).unwrap();
    c.bench_function("limbs_16x4_mul_nearest_even", |bencher| {
        bencher.iter(|| {
            black_box(
                black_box(&a)
                    .mul(black_box(&b), RoundingMode::NearestTiesToEven)
                    .unwrap(),
            )
        })
    });
}

criterion_group!(benches, multiplication);
criterion_main!(benches);
