use super::expand565;

const MAX_CODES: [u16; 3] = [31, 63, 31];
const SHIFTS: [u16; 3] = [0, 5, 11];
const RGB_WEIGHTS: [f64; 3] = [0.082 * 0.082, 0.6094 * 0.6094, 0.3086 * 0.3086];
const EPSILON: f64 = 1e-8;

#[derive(Clone, Copy)]
struct RgbFit {
    endpoints: [u16; 2],
    indices: u32,
    error: f64,
}

pub(super) fn fit_rgb(colors: &[[f64; 3]; 16], block: &mut [u8; 16]) {
    let original = [
        u16::from_le_bytes([block[8], block[9]]),
        u16::from_le_bytes([block[10], block[11]]),
    ];
    let original_error = score(colors, original, true).error;
    let luminance = |[b, g, r]: [f64; 3]| 29.0 * b + 150.0 * g + 77.0 * r;
    let (mut light, mut dark) = (colors[0], colors[0]);
    for &color in &colors[1..] {
        if luminance(color) > luminance(light) {
            light = color;
        }
        if luminance(color) < luminance(dark) {
            dark = color;
        }
    }

    // With two source colors, both can have their own RGB565 endpoint.
    // Quantize against the expanded channel values, with ties going up.
    if colors.iter().all(|color| *color == light || *color == dark) {
        let endpoints = [pack_nearest(light), pack_nearest(dark)];
        if endpoints[0] != endpoints[1] {
            write_rgb(score(colors, endpoints, false), block);
            return;
        }
    }

    let mut best = score(colors, [pack(light), pack(dark)], true);
    for _ in 0..6 {
        let Some(endpoints) = least_squares(colors, best.indices) else {
            break;
        };
        let fit = score(colors, endpoints, true);
        if fit.error >= best.error - EPSILON {
            break;
        }
        best = fit;
    }
    // Require the fitted ramp to improve the starting encoding before
    // searching neighboring RGB565 codes. This preserves existing ties.
    if best.error >= original_error - EPSILON {
        refine_rgb_indices(colors, original, block);
    } else {
        let best = refine_rgb_endpoints(colors, best);
        // Score endpoints with the rounded ramp and write indices using
        // the truncated interpolation of the final packed endpoints.
        write_rgb(score(colors, best.endpoints, false), block);
    }
    fit_narrow_rgb(colors, block);
}

fn adjust_channel(endpoint: u16, channel: usize, adjustment: i16) -> u16 {
    let shift = SHIFTS[channel];
    let max = MAX_CODES[channel];
    let value = (endpoint >> shift) & max;
    let adjusted = value.saturating_add_signed(adjustment).min(max);
    (endpoint & !(max << shift)) | (adjusted << shift)
}

fn refine_rgb_endpoints(colors: &[[f64; 3]; 16], mut best: RgbFit) -> RgbFit {
    for _ in 0..7 {
        let mut next = best;
        for endpoint in 0..2 {
            for direction in [1, -1] {
                for channel in 0..3 {
                    let mut candidate = best.endpoints;
                    candidate[endpoint] = adjust_channel(candidate[endpoint], channel, direction);
                    if candidate[endpoint] != best.endpoints[endpoint] {
                        let candidate = score(colors, candidate, true);
                        if candidate.error < next.error - EPSILON {
                            next = candidate;
                        }
                    }
                }
            }
        }
        if next.error >= best.error - EPSILON {
            break;
        }
        best = next;
    }
    best
}

fn fit_narrow_rgb(colors: &[[f64; 3]; 16], block: &mut [u8; 16]) {
    // A narrow two-channel block can stall when changing one endpoint at a
    // time. Try both endpoints together when the samples span at most one
    // step on the finest RGB565 channel.
    let mut max = colors[0];
    let mut min = colors[0];
    for color in &colors[1..] {
        for (c, &sample) in color.iter().enumerate() {
            max[c] = max[c].max(sample);
            min[c] = min[c].min(sample);
        }
    }
    if (0..3).any(|c| max[c] - min[c] > 4.0) {
        return;
    }
    let varying = std::array::from_fn(|c| max[c] > min[c]);
    let (first, second, constant) = match varying {
        [false, true, true] => (1, 2, 0),
        [true, false, true] => (0, 2, 1),
        [true, true, false] => (0, 1, 2),
        _ => return,
    };
    let high = pack(max);
    let low = pack(min);
    let offsets = [0, 1, -1];
    let original = [
        u16::from_le_bytes([block[8], block[9]]),
        u16::from_le_bytes([block[10], block[11]]),
    ];
    let mut best = score(colors, original, true);
    for high_first in offsets {
        let a = adjust_channel(high, first, high_first);
        for low_first in offsets {
            let b = adjust_channel(low, first, low_first);
            for high_second in offsets {
                let high = adjust_channel(a, second, high_second);
                for low_second in offsets {
                    let low = adjust_channel(b, second, low_second);
                    let candidate = score(colors, [high, low], true);
                    if candidate.error < best.error - EPSILON {
                        best = candidate;
                    }
                }
            }
        }
    }

    // The interpolated ramp can represent a constant channel more closely
    // when the two endpoints straddle its value on the RGB565 grid.
    let [high, low] = best.endpoints;
    for high_offset in offsets {
        let high = adjust_channel(high, constant, high_offset);
        for low_offset in offsets {
            let low = adjust_channel(low, constant, low_offset);
            let candidate = score(colors, [high, low], true);
            if candidate.error < best.error - EPSILON {
                best = candidate;
            }
        }
    }
    if best.endpoints != original {
        write_rgb(score(colors, best.endpoints, false), block);
    }
}

fn write_rgb(best: RgbFit, block: &mut [u8; 16]) {
    block[8..10].copy_from_slice(&best.endpoints[0].to_le_bytes());
    block[10..12].copy_from_slice(&best.endpoints[1].to_le_bytes());
    block[12..].copy_from_slice(&best.indices.to_le_bytes());
}

fn pack_nearest(color: [f64; 3]) -> u16 {
    let initial = pack(color);
    let mut result = 0;
    for (i, &sample) in color.iter().enumerate() {
        let shift = SHIFTS[i];
        let max = MAX_CODES[i];
        let initial = (initial >> shift) & max;
        let mut best = initial;
        let mut best_error = f64::INFINITY;
        for code in initial.saturating_sub(1)..=(initial + 1).min(max) {
            let expanded = expand565(code << shift)[i];
            let error = (f64::from(expanded) - sample).abs();
            if error <= best_error {
                best = code;
                best_error = error;
            }
        }
        result |= best << shift;
    }
    result
}

fn pack(colors: [f64; 3]) -> u16 {
    let channel = |i: usize| {
        let value = colors[i].clamp(0.0, 255.0) * f64::from(MAX_CODES[i]) / 255.0;
        (value.round() as u16) << SHIFTS[i]
    };
    channel(0) | channel(1) | channel(2)
}

fn least_squares(colors: &[[f64; 3]; 16], indices: u32) -> Option<[u16; 2]> {
    let (mut aa, mut ab, mut bb) = (0u32, 0u32, 0u32);
    let mut x = [0.0; 3];
    let mut y = [0.0; 3];
    for (i, color) in colors.iter().enumerate() {
        let a = [3, 0, 2, 1][((indices >> (2 * i)) & 3) as usize];
        let b = 3 - a;
        aa += a * a;
        ab += a * b;
        bb += b * b;
        for c in 0..3 {
            x[c] += f64::from(a) * color[c];
            y[c] += f64::from(b) * color[c];
        }
    }
    let determinant = aa * bb - ab * ab;
    if determinant == 0 {
        return None;
    }
    let scale = 3.0 / f64::from(determinant);
    Some([
        pack(std::array::from_fn(|c| {
            scale * (x[c] * f64::from(bb) - y[c] * f64::from(ab))
        })),
        pack(std::array::from_fn(|c| {
            scale * (y[c] * f64::from(aa) - x[c] * f64::from(ab))
        })),
    ])
}

fn rgb_palette(endpoints: [u16; 2], rounded: bool) -> [[f64; 3]; 4] {
    let [high, low] = endpoints.map(|value| expand565(value).map(u16::from));
    let bias = u16::from(rounded);
    [
        high,
        low,
        std::array::from_fn(|i| (2 * high[i] + low[i] + bias) / 3),
        std::array::from_fn(|i| (high[i] + 2 * low[i] + bias) / 3),
    ]
    .map(|color| color.map(f64::from))
}

fn rgb_error(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [b, g, r] = std::array::from_fn(|c| (a[c] - b[c]).powi(2) * RGB_WEIGHTS[c]);
    b + g + r
}

fn refine_rgb_indices(colors: &[[f64; 3]; 16], endpoints: [u16; 2], block: &mut [u8; 16]) {
    let indices = u32::from_le_bytes(block[12..].try_into().unwrap());
    let palette = rgb_palette(endpoints, false);
    let previous_error: f64 = colors
        .iter()
        .enumerate()
        .map(|(texel, &color)| rgb_error(color, palette[((indices >> (2 * texel)) & 3) as usize]))
        .sum();
    let fit = score(colors, endpoints, false);
    if fit.error < previous_error - EPSILON {
        block[12..].copy_from_slice(&fit.indices.to_le_bytes());
    }
}

fn score(colors: &[[f64; 3]; 16], mut endpoints: [u16; 2], rounded: bool) -> RgbFit {
    if endpoints[0] < endpoints[1] {
        endpoints.swap(0, 1);
    }
    let palette = rgb_palette(endpoints, rounded);
    let mut best = RgbFit {
        endpoints,
        indices: 0,
        error: 0.0,
    };
    for (i, &color) in colors.iter().enumerate() {
        let (mut first, mut index) = (f64::INFINITY, 0);
        for (code, &rgb) in palette.iter().enumerate() {
            let error = rgb_error(rgb, color);
            if error < first {
                first = error;
                index = code as u32;
            }
        }
        best.error += first;
        best.indices |= index << (2 * i);
    }
    best
}

pub(super) fn fit_alpha_extremes(alpha: &[f64; 16], block: &mut [u8; 16]) -> bool {
    if !alpha.iter().any(|&a| a == 0.0 || a == 255.0) || block[0] <= block[1] {
        return false;
    }
    let (mut low, mut high) = (255.0f64, 0.0f64);
    for &a in alpha.iter().filter(|&&a| a > 0.0 && a < 255.0) {
        low = low.min(a);
        high = high.max(a);
    }
    if low.round() >= high.round() {
        return false;
    }

    let old = read_alpha_indices(block);
    let mut indices = 0u64;
    let mut gain = 0.0;
    for (i, &a) in alpha.iter().enumerate() {
        let index = if a == low {
            0
        } else if a == high {
            1
        } else if a == 0.0 {
            6
        } else if a == 255.0 {
            7
        } else {
            return false;
        };
        let old_index = alpha_index(old, i);
        let position = [0.0, 7.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0][old_index];
        let previous =
            (f64::from(block[0]) * (7.0 - position) + f64::from(block[1]) * position) / 7.0;
        gain += (previous - a).powi(2) - (a.round() - a).powi(2);
        indices |= index << (3 * i);
    }
    if gain > EPSILON {
        block[..2].copy_from_slice(&[low.round() as u8, high.round() as u8]);
        block[2..8].copy_from_slice(&indices.to_le_bytes()[..6]);
        return true;
    }
    false
}

#[derive(Clone, Copy)]
struct AlphaFit {
    endpoints: [f64; 2],
    indices: u64,
    rounded_error: f64,
    fractional_error: f64,
}

pub(super) fn fit_wide_alpha(samples: &[f64; 16], block: &mut [u8; 16]) {
    let [high, low] = [block[0], block[1]].map(f64::from);
    if high - low < 8.0 {
        return;
    }
    let (mut min, mut max) = (255.0f64, 0.0f64);
    let (mut interior_min, mut interior_max) = (255.0f64, 0.0f64);
    for &sample in samples {
        min = min.min(sample);
        max = max.max(sample);
        if sample > 0.0 && sample < 255.0 {
            interior_min = interior_min.min(sample);
            interior_max = interior_max.max(sample);
        }
    }
    if max - min <= 8.0 {
        return;
    }

    let eight = optimize_alpha(samples, [max, min], false);
    let six = if interior_min < interior_max {
        [interior_min, interior_max]
    } else {
        [0.0, 255.0]
    };
    let six = optimize_alpha(samples, six, true);
    let fit = if six.endpoints[0] < six.endpoints[1]
        && six.fractional_error < eight.fractional_error - EPSILON
    {
        six
    } else {
        eight
    };

    let palette = alpha_palette([high, low], false).map(f64::round);
    let old_indices = read_alpha_indices(block);
    let previous_error: f64 = samples
        .iter()
        .enumerate()
        .map(|(i, &sample)| (palette[alpha_index(old_indices, i)] - sample).powi(2))
        .sum();
    // A change within the squared error of a half-alpha step can come from
    // rounding alone. Keep the existing ramp on that boundary.
    if fit.endpoints[0] != fit.endpoints[1] && fit.rounded_error + 0.25 + EPSILON < previous_error {
        block[..2].copy_from_slice(&fit.endpoints.map(|endpoint| endpoint as u8));
        block[2..8].copy_from_slice(&fit.indices.to_le_bytes()[..6]);
    }
}

fn optimize_alpha(samples: &[f64; 16], mut endpoints: [f64; 2], six: bool) -> AlphaFit {
    for _ in 0..4 {
        let fit = score_alpha(samples, endpoints, six);
        let Some(next) = least_squares_alpha(samples, fit.indices, six) else {
            break;
        };
        endpoints = next;
    }
    score_alpha(
        samples,
        endpoints.map(|value| (value + 0.5 + 1e-9).floor()),
        six,
    )
}

fn least_squares_alpha(samples: &[f64; 16], indices: u64, six: bool) -> Option<[f64; 2]> {
    let steps = if six { 5 } else { 7 };
    let (mut aa, mut ab, mut bb) = (0u32, 0u32, 0u32);
    let (mut x, mut y) = (0.0, 0.0);
    for (i, &sample) in samples.iter().enumerate() {
        let index = alpha_index(indices, i);
        if six && index >= 6 {
            continue;
        }
        let b = [0, steps, 1, 2, 3, 4, 5, 6][index];
        let a = steps - b;
        aa += a * a;
        ab += a * b;
        bb += b * b;
        x += f64::from(a) * sample;
        y += f64::from(b) * sample;
    }
    let determinant = aa * bb - ab * ab;
    if determinant == 0 {
        return None;
    }
    let scale = f64::from(steps) / f64::from(determinant);
    let a = scale * (f64::from(bb) * x - f64::from(ab) * y);
    let b = scale * (f64::from(aa) * y - f64::from(ab) * x);
    (if six { a < b } else { a > b }).then(|| [a.clamp(0.0, 255.0), b.clamp(0.0, 255.0)])
}

fn alpha_index(bits: u64, texel: usize) -> usize {
    ((bits >> (3 * texel)) & 7) as usize
}

fn read_alpha_indices(block: &[u8; 16]) -> u64 {
    let mut bytes = [0; 8];
    bytes[..6].copy_from_slice(&block[2..8]);
    u64::from_le_bytes(bytes)
}

fn alpha_palette([a, b]: [f64; 2], six: bool) -> [f64; 8] {
    let steps = if six { 5 } else { 7 };
    let mut values = [a, b, 0.0, 0.0, 0.0, 0.0, 0.0, 255.0];
    for i in 1..steps {
        values[i + 1] = (a * (steps - i) as f64 + b * i as f64) / steps as f64;
    }
    values
}

fn score_alpha(samples: &[f64; 16], endpoints: [f64; 2], six: bool) -> AlphaFit {
    let real = alpha_palette(endpoints, six);
    let rounded = real.map(f64::round);
    let mut fit = AlphaFit {
        endpoints,
        indices: 0,
        rounded_error: 0.0,
        fractional_error: 0.0,
    };
    for (i, &sample) in samples.iter().enumerate() {
        let (mut best, mut index) = (f64::INFINITY, 0);
        for (candidate, &alpha) in rounded.iter().enumerate() {
            let error = (alpha - sample).powi(2);
            if error < best {
                best = error;
                index = candidate;
            }
        }
        fit.indices |= (index as u64) << (3 * i);
        fit.rounded_error += best;
        fit.fractional_error += (real[index] - sample).powi(2);
    }
    fit
}
