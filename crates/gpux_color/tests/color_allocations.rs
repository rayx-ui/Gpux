use gpux_color::{Color, ProPhotoRgb};

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[test]
fn warm_cached_reads_allocate_zero_times() {
    let color =
        Color::from_prophoto_rgb(ProPhotoRgb::new(0.2, 0.7, 0.4), 0.8).expect("prophoto color");
    let _ = color.to_srgb();
    let _ = color.to_display_p3();

    const READS: u64 = 10_000;
    {
        let _profiler = dhat::Profiler::builder().testing().build();
        for _ in 0..READS {
            std::hint::black_box(color.to_srgb());
            std::hint::black_box(color.to_display_p3());
        }
        let stats = dhat::HeapStats::get();

        // The global allocator also counts the test harness's own threads, which have been
        // seen allocating two blocks while this loop runs. A read that allocated would do so at
        // least once per call, and a cache that grew would reallocate a dozen times or more.
        assert!(
            stats.total_blocks <= 4,
            "warm conversion cache reads must not allocate, but {} blocks ({} bytes) were \
             allocated over {} reads",
            stats.total_blocks,
            stats.total_bytes,
            2 * READS
        );
    }
}
