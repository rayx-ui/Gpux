use gpux_color::{Color, ProPhotoRgb};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn warm_cached_reads_allocate_zero_times() {
    let color =
        Color::from_prophoto_rgb(ProPhotoRgb::new(0.2, 0.7, 0.4), 0.8).expect("prophoto color");
    let _ = color.to_srgb();
    let _ = color.to_display_p3();

    {
        let _profiler = dhat::Profiler::builder().testing().build();
        for _ in 0..10_000 {
            std::hint::black_box(color.to_srgb());
            std::hint::black_box(color.to_display_p3());
        }
        let stats = dhat::HeapStats::get();

        assert_eq!(
            stats.total_blocks, 0,
            "warm conversion cache reads must not allocate"
        );
        assert_eq!(
            stats.total_bytes, 0,
            "warm conversion cache reads must allocate zero bytes"
        );
    }
}
