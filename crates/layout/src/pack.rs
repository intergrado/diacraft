//! Laying the separate parts of a diagram next to each other: as many to a row as fit a target width, then the next row.

/// Where the corner of each part goes, across and along, in rows of parts: `sizes` are the parts' sizes `(across, along)`, in the order they are placed, and the
/// spaces are kept between parts. The target width of a row is the widest part or one and a half times the root of the total area, whichever is more, so that
/// a diagram of a hundred small parts is a block and not a line. One part goes at the corner `(0, 0)`.
pub(crate) fn rows(sizes: &[(f64, f64)], gap_c: f64, gap_m: f64) -> Vec<(f64, f64)> {
    let mut corners = Vec::with_capacity(sizes.len());
    if sizes.len() <= 1 {
        corners.extend(sizes.iter().map(|_| (0.0, 0.0)));
        return corners;
    }
    let widest = sizes.iter().fold(0.0_f64, |m, s| m.max(s.0));
    let area: f64 = sizes.iter().map(|s| (s.0 + gap_c) * (s.1 + gap_m)).sum();
    let target = widest.max(1.5 * area.sqrt());
    let (mut across, mut along, mut row_height) = (0.0_f64, 0.0_f64, 0.0_f64);
    for &(w, h) in sizes {
        if across > 0.0 && across + w > target {
            along += row_height + gap_m;
            across = 0.0;
            row_height = 0.0;
        }
        corners.push((across, along));
        across += w + gap_c;
        row_height = row_height.max(h);
    }
    corners
}
