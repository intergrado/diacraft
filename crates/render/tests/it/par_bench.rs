//! Timings of a frame at several thread counts, by hand:
//! `BENCH_THREADS=1,4,16,31 [BENCH_SHADOWS=1] [BENCH_BIG=1] cargo test -p diacraft-render --release --test it par_bench -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use diacraft_render::{Parallel, Renderer, View};
use kurbo::Affine;

use crate::frame_time::diagram_on;

#[test]
#[ignore = "a timing, run by hand"]
fn par_bench_frames() {
    let shadows = std::env::var("BENCH_SHADOWS").is_ok();
    let big = std::env::var("BENCH_BIG").is_ok();
    let (doc, scale) = if big { (diagram_on(80, 75, shadows, 129.0, 83.5), 14.0) } else { (diagram_on(36, 30, shadows, 60.0, 40.0), 30.0) };
    let frames = std::env::var("BENCH_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let counts: Vec<usize> =
        std::env::var("BENCH_THREADS").unwrap_or_else(|_| "1".to_owned()).split(',').filter_map(|c| c.trim().parse().ok()).collect();
    let view = View { to_px: Affine::new([scale, 0.0, 0.0, -scale, 0.0, 1200.0]), width: 1800, height: 1200 };
    let shapes: usize = doc.page(1).map_or(0, |p| p.shapes.len());
    for threads in counts {
        let mut renderer = Renderer::new();
        renderer.set_parallel(Parallel::eager(threads));
        let mut least = Duration::MAX;
        for _ in 0..frames {
            let started = Instant::now();
            let r = renderer.render(&doc, 1, &view);
            least = least.min(started.elapsed());
            assert_eq!(r.rgba_premul.len(), 1800 * 1200 * 4);
        }
        let stats = renderer.par_stats();
        eprintln!(
            "PAR BENCH: {shapes} shapes, shadows {shadows}, {threads} threads: least of {frames}: {least:?}; shared walks {}, in order {}; mean walk {:?}, mean raster {:?}, mean abandoned {:?}",
            stats.walks_shared,
            stats.walks_in_order,
            stats.walk_time / u32::try_from(frames).unwrap_or(1),
            stats.raster_time / u32::try_from(frames).unwrap_or(1),
            stats.abandoned_time / u32::try_from(frames).unwrap_or(1)
        );
    }
}
