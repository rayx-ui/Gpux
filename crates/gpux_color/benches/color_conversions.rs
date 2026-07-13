use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use gpux_color::{Color, ProPhotoRgb, XyzD65};
use std::time::Duration;

fn conversions(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("color-conversions");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(1));
    group.throughput(Throughput::Elements(1));

    group.bench_function(BenchmarkId::new("srgb", "cold"), |bencher| {
        bencher.iter(|| {
            let color = Color::from_xyz_d65(
                XyzD65::new(
                    std::hint::black_box(0.31),
                    std::hint::black_box(0.42),
                    std::hint::black_box(0.18),
                ),
                0.8,
            )
            .expect("finite XYZ");
            std::hint::black_box(color.to_srgb())
        });
    });

    let warm_srgb = Color::from_xyz_d65(XyzD65::new(0.31, 0.42, 0.18), 0.8).expect("finite XYZ");
    let _ = warm_srgb.to_srgb();
    group.bench_function(BenchmarkId::new("srgb", "warm"), |bencher| {
        bencher.iter(|| std::hint::black_box(warm_srgb.to_srgb()));
    });

    group.bench_function(
        BenchmarkId::new("display-p3", "cold-wide-gamut"),
        |bencher| {
            bencher.iter(|| {
                let color = Color::from_prophoto_rgb(
                    ProPhotoRgb::new(
                        std::hint::black_box(0.2),
                        std::hint::black_box(0.7),
                        std::hint::black_box(0.4),
                    ),
                    0.8,
                )
                .expect("finite ProPhoto RGB");
                std::hint::black_box(color.to_display_p3())
            });
        },
    );

    let warm_wide = Color::from_prophoto_rgb(ProPhotoRgb::new(0.2, 0.7, 0.4), 0.8)
        .expect("finite ProPhoto RGB");
    let _ = warm_wide.to_display_p3();
    group.bench_function(
        BenchmarkId::new("display-p3", "warm-wide-gamut"),
        |bencher| {
            bencher.iter(|| std::hint::black_box(warm_wide.to_display_p3()));
        },
    );

    group.bench_function("clone", |bencher| {
        bencher.iter(|| std::hint::black_box(warm_wide.clone()));
    });
    group.finish();
}

criterion_group!(benches, conversions);
criterion_main!(benches);
