//! Setting the shapes of one layer across the flow: each wants a place, and the ones next to each other stay in their order and apart by a given space.

/// Puts items in a row, in their order: item `i` wants its middle at `want[i]`, with the weight `weight[i]`, and item `i + 1` has its middle at least `sep[i]` past
/// item `i`'s. The places given are the ones that keep the order and the spaces and are nearest the wants, by the sum of the weights times the squares of the
/// distances (the pool-adjacent-violators method: a shape that would be crowded out of its place joins the ones in its way, and they go to the mean of what they
/// want). One pass, and no more work than the number of items.
///
/// `want` and `weight` are as long as the row, and `sep` is one shorter; `out` takes the places, and is as long as the row too. A slice of another length is not
/// used past the shortest (the caller builds all four from the same row).
pub(crate) fn place_row(want: &[f64], weight: &[f64], sep: &[f64], out: &mut [f64]) {
    let n = want.len().min(weight.len()).min(out.len());
    if n == 0 {
        return;
    }
    // The least distance of each item from the first, and what each wants once that is taken off: the places `y` that must not decrease.
    let mut offsets = Vec::with_capacity(n);
    let mut at = 0.0;
    for i in 0..n {
        if i > 0 {
            at += sep.get(i - 1).copied().unwrap_or(0.0).max(0.0);
        }
        offsets.push(at);
    }
    // Blocks of neighbours that share one place: the weight, the weighted sum of what they want, and how many.
    let mut blocks: Vec<(f64, f64, usize)> = Vec::new();
    for (i, offset) in offsets.iter().enumerate() {
        let w = weight.get(i).copied().unwrap_or(1.0).max(1.0e-9);
        let u = want.get(i).copied().unwrap_or(0.0) - offset;
        blocks.push((w, w * u, 1));
        while blocks.len() >= 2 {
            let (Some(top), Some(below)) = (blocks.last().copied(), blocks.get(blocks.len().wrapping_sub(2)).copied()) else { break };
            if below.1 / below.0 <= top.1 / top.0 {
                break;
            }
            blocks.truncate(blocks.len() - 2);
            blocks.push((below.0 + top.0, below.1 + top.1, below.2 + top.2));
        }
    }
    let mut i = 0;
    for (w, wu, count) in blocks {
        let mean = wu / w;
        for _ in 0..count {
            if let (Some(slot), Some(offset)) = (out.get_mut(i), offsets.get(i)) {
                *slot = mean + offset;
            }
            i += 1;
        }
    }
}
