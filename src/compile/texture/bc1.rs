use super::resize;
use std::sync::OnceLock;

type Color = [f64; 3];
type Endpoint = [u16; 3];

const WEIGHTS: Color = [0.3086_f32 as f64, 0.6094_f32 as f64, 0.082_f32 as f64];
const MAX_CODE: Endpoint = [31, 63, 31];

#[derive(Clone, Copy)]
struct Candidate {
    endpoints: [Endpoint; 2],
    colors: usize,
    selectors: [u8; 16],
}

impl Candidate {
    fn new(endpoints: [Endpoint; 2], colors: usize) -> Self {
        Self {
            endpoints,
            colors,
            selectors: [0; 16],
        }
    }

    fn palette(self, rounded_thirds: bool) -> [Color; 4] {
        let [a, b] = self
            .endpoints
            .map(|color| std::array::from_fn::<_, 3, _>(|i| expand(color[i], i)));
        let mut palette = [a, b, [0; 3], [0; 3]];
        for i in 0..3 {
            if self.colors == 4 {
                let bias = u16::from(rounded_thirds);
                palette[2][i] = (2 * a[i] + b[i] + bias) / 3;
                palette[3][i] = (a[i] + 2 * b[i] + bias) / 3;
            } else {
                palette[2][i] = (a[i] + b[i]) / 2;
            }
        }
        palette.map(|color| std::array::from_fn(|i| f64::from(color[i]) * WEIGHTS[i] / 255.0))
    }

    fn pack(self, block_width: usize, block_height: usize) -> [u8; 8] {
        let [mut a, mut b] = self.endpoints.map(pack_endpoint);
        let mut selectors = self.selectors;
        if a == b {
            a = a.wrapping_add(1);
            selectors.fill(1);
        }
        let swap = (self.colors == 4 && a < b) || (self.colors == 3 && a > b);
        if swap {
            std::mem::swap(&mut a, &mut b);
        }
        let mut bits = 0u32;
        for y in 0..4 {
            for x in 0..4 {
                let selector =
                    if block_width == block_height && y >= block_height && x + 1 == block_width {
                        3
                    } else {
                        let index = y.min(block_height - 1) * block_width + x.min(block_width - 1);
                        let selector = selectors[index];
                        if swap && (self.colors == 4 || selector < 2) {
                            selector ^ 1
                        } else {
                            selector
                        }
                    };
                bits |= u32::from(selector) << (2 * (y * 4 + x));
            }
        }
        let mut bytes = [0; 8];
        bytes[..2].copy_from_slice(&a.to_le_bytes());
        bytes[2..4].copy_from_slice(&b.to_le_bytes());
        bytes[4..].copy_from_slice(&bits.to_le_bytes());
        bytes
    }
}

pub(super) fn append_levels(
    dds: &mut Vec<u8>,
    pixels: &[[u8; 4]],
    width: usize,
    height: usize,
    levels: u32,
) -> Result<(), &'static str> {
    let size: usize = (0..levels)
        .map(|level| (width >> level).max(1).div_ceil(4) * (height >> level).max(1).div_ceil(4) * 8)
        .sum();
    dds.try_reserve_exact(size)
        .map_err(|_| "DXT1 DDS is too large")?;

    append_image(dds, width, height, |index| {
        pixels[index].map(|channel| f32::from(channel) / 255.0)
    });
    let mut mip = Vec::new();
    for level in 1..levels {
        let mip_width = (width >> level).max(1);
        let mip_height = (height >> level).max(1);
        mip.clear();
        mip.try_reserve_exact(mip_width * mip_height)
            .map_err(|_| "DXT1 mip level is too large")?;
        resize::filter_mip(pixels, width, height, mip_width, mip_height, |pixel| {
            mip.push(pixel)
        });
        append_image(dds, mip_width, mip_height, |index| mip[index]);
    }
    Ok(())
}

fn append_image(dds: &mut Vec<u8>, width: usize, height: usize, pixel: impl Fn(usize) -> [f32; 4]) {
    for y in (0..height).step_by(4) {
        for x in (0..width).step_by(4) {
            let block_width = (width - x).min(4);
            let block_height = (height - y).min(4);
            let mut rgb = [[0.0; 3]; 16];
            for row in 0..block_height {
                for column in 0..block_width {
                    let [blue, green, red, _] = pixel((y + row) * width + x + column);
                    rgb[row * block_width + column] = [red, green, blue].map(f64::from);
                }
            }
            let encoded = encode(&rgb[..block_width * block_height]);
            dds.extend_from_slice(&encoded.pack(block_width, block_height));
        }
    }
}

fn expand(code: u16, channel: usize) -> u16 {
    if channel == 1 {
        (code << 2) | (code >> 4)
    } else {
        (code << 3) | (code >> 2)
    }
}

fn pack_endpoint([r, g, b]: Endpoint) -> u16 {
    (r << 11) | (g << 5) | b
}

fn dot(a: Color, b: Color) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn subtract(a: Color, b: Color) -> Color {
    std::array::from_fn(|i| a[i] - b[i])
}

fn weighted(color: Color) -> Color {
    std::array::from_fn(|i| color[i] * WEIGHTS[i])
}

fn nearest(color: Color, palette: &[Color]) -> (usize, f64) {
    let error = |entry| {
        let diff = subtract(color, entry);
        dot(diff, diff)
    };
    let mut best = (0, error(palette[0]));
    for (index, &entry) in palette.iter().enumerate().skip(1) {
        let distance = error(entry);
        if distance < best.1 {
            best = (index, distance);
        }
    }
    best
}

fn encode(rgb: &[Color]) -> Candidate {
    let mut point = [[0.0; 3]; 16];
    let mut frequency = [0.0; 16];
    let mut members = [0; 16];
    let mut count = 0;
    for (index, &color) in rgb.iter().enumerate() {
        let color = weighted(color);
        let item = match point[..count].iter().position(|&value| value == color) {
            Some(item) => item,
            None => {
                point[count] = color;
                count += 1;
                count - 1
            }
        };
        frequency[item] += 1.0;
        members[index] = item;
    }
    let points = &point[..count];
    let frequency = &frequency[..count];
    let [slow_four, slow_three] = if count == 1 {
        let bytes = rgb[0].map(|value| (value * 255.0).round() as u8);
        if bytes[0] == bytes[1] && bytes[1] == bytes[2] {
            [gray_candidates()[usize::from(bytes[0])]; 2]
        } else {
            [solid(rgb[0], 4), solid(rgb[0], 3)]
        }
    } else {
        [4, 3].map(|colors| {
            let fit = fit_colors(points, frequency, colors);
            let mut encoded = quantize_fit(fit, points, frequency, colors);
            let unique = encoded.selectors;
            for (dest, item) in encoded.selectors.iter_mut().zip(members) {
                *dest = unique[item];
            }
            encoded
        })
    };
    let mut best = slow_four;
    let mut error = color_error(best, points, frequency);
    for candidate in [slow_three, fast(rgb, 3), fast(rgb, 4)] {
        let distance = color_error(candidate, points, frequency);
        if distance < error {
            error = distance;
            best = candidate;
        }
    }
    best
}

fn color_error(candidate: Candidate, points: &[Color], frequency: &[f64]) -> f64 {
    let palette = candidate.palette(false);
    points
        .iter()
        .zip(frequency)
        .map(|(&p, &f)| nearest(p, &palette).1 * f)
        .sum()
}

#[derive(Clone, Copy)]
struct Fit {
    endpoints: [Color; 2],
    colors: usize,
    error: f64,
}

fn fit_colors(points: &[Color], frequency: &[f64], colors: usize) -> Fit {
    if points.len() == 2 {
        let [a, b] = [points[0], points[1]];
        return Fit {
            endpoints: if a > b { [b, a] } else { [a, b] },
            colors: 2,
            error: 0.0,
        };
    }
    let total: f64 = frequency.iter().sum();
    let mut mean = [0.0; 3];
    let mut matrix = [[0.0; 3]; 3];
    for (&point, &count) in points.iter().zip(frequency) {
        for i in 0..3 {
            mean[i] += point[i] * count / total;
        }
    }
    let mut diameter = [0.0; 3];
    let mut diameter_squared = 0.0;
    for (index, (&point, &count)) in points.iter().zip(frequency).enumerate() {
        let delta = subtract(point, mean);
        for i in 0..3 {
            for j in 0..3 {
                matrix[i][j] += delta[i] * delta[j] * count;
            }
        }
        for &other in &points[..index] {
            let axis = subtract(point, other);
            let length = dot(axis, axis);
            if length > diameter_squared {
                diameter_squared = length;
                diameter = axis;
            }
        }
    }

    let axis = principal_axis(matrix, 5, false);
    if dot(axis, diameter) < 0.0 {
        diameter = diameter.map(|value| -value);
    }
    let axis_fit = refine(points, frequency, colors, axis, false);
    let diameter_fit = refine(points, frequency, colors, diameter, false);
    let best = if diameter_fit.error < axis_fit.error - 1e-13 {
        diameter_fit
    } else {
        axis_fit
    };
    if best.endpoints.iter().any(|point| {
        point
            .iter()
            .zip(WEIGHTS)
            .any(|(&value, high)| value < 0.0 || value > high)
    }) {
        refine(
            points,
            frequency,
            colors,
            subtract(best.endpoints[1], best.endpoints[0]),
            true,
        )
    } else {
        best
    }
}

fn principal_axis(mut matrix: [Color; 3], iterations: usize, single_precision: bool) -> Color {
    let round = |x| if single_precision { x as f32 as f64 } else { x };
    for _ in 0..iterations {
        let square: [Color; 3] =
            std::array::from_fn(|i| std::array::from_fn(|j| round(dot(matrix[i], matrix[j]))));
        let norm = square.iter().map(|&row| dot(row, row)).sum::<f64>().sqrt();
        if norm == 0.0 {
            break;
        }
        matrix = square.map(|row| row.map(|value| round(value / norm)));
    }
    let mut largest = 0;
    for i in 1..3 {
        if matrix[i][i] > matrix[largest][largest] {
            largest = i;
        }
    }
    let axis = matrix[largest];
    let norm = dot(axis, axis).sqrt();
    if norm == 0.0 {
        axis
    } else {
        axis.map(|value| round(value / norm))
    }
}

fn refine(points: &[Color], weights: &[f64], colors: usize, mut axis: Color, clipped: bool) -> Fit {
    let mut orders = [[0; 16]; 16];
    let mut best = Fit {
        endpoints: [points[0], points[1]],
        colors: 2,
        error: f64::INFINITY,
    };
    let limit = if clipped { 8 } else { 16 };
    for step in 0..limit {
        let mut order = std::array::from_fn(|i| i);
        order[..points.len()]
            .sort_by(|&a, &b| dot(points[a], axis).total_cmp(&dot(points[b], axis)));
        if orders[..step].contains(&order) {
            break;
        }
        orders[step] = order;
        let candidate = fit_order(points, weights, &order[..points.len()], colors, clipped);
        axis = subtract(candidate.endpoints[1], candidate.endpoints[0]);
        if clipped || candidate.error < best.error - 1e-13 {
            best = candidate;
        }
    }
    best
}

#[derive(Clone, Copy, Default)]
struct Sum {
    weight: f64,
    color: Color,
}

impl Sum {
    fn between(start: Self, end: Self) -> Self {
        Self {
            weight: end.weight - start.weight,
            color: subtract(end.color, start.color),
        }
    }
}

fn fit_order(
    points: &[Color],
    weights: &[f64],
    order: &[usize],
    colors: usize,
    clipped: bool,
) -> Fit {
    let count = order.len();
    let mut prefix = [Sum::default(); 17];
    for (i, &index) in order.iter().enumerate() {
        let mut next = prefix[i];
        next.weight += weights[index];
        for channel in 0..3 {
            next.color[channel] += points[index][channel] * weights[index];
        }
        prefix[i + 1] = next;
    }

    // Adjacent cuts may coincide. An empty interior palette slot still changes the fit.
    let mut best = Fit {
        endpoints: [points[0], points[1]],
        colors: 2,
        error: f64::INFINITY,
    };
    for first in (1..count).rev() {
        let second_end = if colors == 4 { count } else { count - 1 };
        for second in (first..=second_end).rev() {
            let third_start = if colors == 4 { second } else { count };
            for third in (third_start..=count).rev() {
                let cuts = [0, first, second, third, count];
                let last = cuts
                    .windows(2)
                    .rposition(|range| range[0] != range[1])
                    .unwrap();
                let occupied = cuts.windows(2).filter(|range| range[0] != range[1]).count();
                let mut aa = 0.0;
                let mut ab = 0.0;
                let mut bb = 0.0;
                let mut x = [0.0; 3];
                let mut y = [0.0; 3];
                for i in 0..=last {
                    let sum = Sum::between(prefix[cuts[i]], prefix[cuts[i + 1]]);
                    let u = (last - i) as f64;
                    let v = i as f64;
                    aa += u * u * sum.weight;
                    ab += u * v * sum.weight;
                    bb += v * v * sum.weight;
                    for j in 0..3 {
                        x[j] += u * sum.color[j];
                        y[j] += v * sum.color[j];
                    }
                }
                let inverse = last as f64 / (aa * bb - ab * ab);
                let mut a = std::array::from_fn(|i| (bb * x[i] - ab * y[i]) * inverse);
                let mut b = std::array::from_fn(|i| (aa * y[i] - ab * x[i]) * inverse);
                let mut error = 0.0;
                let scale = last as f64;
                for i in 0..3 {
                    let objective = |a, b| {
                        (aa * a * a + 2.0 * ab * a * b + bb * b * b) / scale
                            - 2.0 * (x[i] * a + y[i] * b)
                    };
                    if clipped {
                        let fixed_a = a[i].clamp(0.0, WEIGHTS[i]);
                        let fixed_b = b[i].clamp(0.0, WEIGHTS[i]);
                        let other_b = ((scale * y[i] - ab * fixed_a) / bb).clamp(0.0, WEIGHTS[i]);
                        let other_a = ((scale * x[i] - ab * fixed_b) / aa).clamp(0.0, WEIGHTS[i]);
                        (a[i], b[i]) = if objective(fixed_a, other_b) < objective(other_a, fixed_b)
                        {
                            (fixed_a, other_b)
                        } else {
                            (other_a, fixed_b)
                        };
                    }
                    error += objective(a[i], b[i]) / scale;
                }
                if error < best.error - 1e-13 {
                    best = Fit {
                        endpoints: [a, b],
                        colors: if occupied == 2 { 2 } else { last + 1 },
                        error,
                    };
                }
            }
        }
    }
    best
}
fn quantize_fit(fit: Fit, points: &[Color], frequency: &[f64], requested: usize) -> Candidate {
    let [a, b] = fit.endpoints;
    let extrapolate = |pos: f64| std::array::from_fn(|i| a[i] + pos * (b[i] - a[i]));
    let mut best = (Candidate::new([[0; 3]; 2], 4), f64::INFINITY);
    let mut consider = |colors, lo, hi| {
        let result = quantize(
            [extrapolate(lo), extrapolate(hi)],
            points,
            frequency,
            colors,
        );
        if result.1 < best.1 {
            best = result;
        }
    };
    consider(fit.colors.max(3), 0.0, 1.0);
    if fit.colors == 2 {
        consider(3, -1.0, 1.0);
        consider(3, 0.0, 2.0);
    }
    if requested == 4 {
        if fit.colors == 2 {
            consider(4, 0.0, 1.0);
            consider(4, 0.0, 3.0);
            consider(4, -2.0, 1.0);
            consider(4, -1.0, 2.0);
        }
        if fit.colors <= 3 {
            consider(4, -0.5, 1.0);
            consider(4, 0.0, 1.5);
        }
    }
    best.0
}

fn quantize(
    endpoints: [Color; 2],
    points: &[Color],
    weights: &[f64],
    colors: usize,
) -> (Candidate, f64) {
    let lower = endpoints.map(|point| {
        std::array::from_fn::<_, 3, _>(|i| {
            let byte = point[i] * (255.0 / WEIGHTS[i]);
            let code = ((byte as u16) >> if i == 1 { 2 } else { 3 }).min(MAX_CODE[i]);
            if f64::from(expand(code, i)) > byte {
                code.saturating_sub(1)
            } else {
                code
            }
        })
    });
    let mut best = Candidate::new(lower, colors);
    let mut best_rank = f64::INFINITY;
    let probability = 1.0 / weights.iter().sum::<f64>();
    let pair = |a: f64, b: f64| a + b - (a - b).abs();
    for mask in 0..64 {
        let endpoints = std::array::from_fn(|endpoint| {
            std::array::from_fn(|i| {
                (lower[endpoint][i] + ((mask >> (i * 2 + endpoint)) & 1)).min(MAX_CODE[i])
            })
        });
        let candidate = Candidate::new(endpoints, colors);
        let palette = candidate.palette(true);
        let mut rank = 0.0;
        for (&point, &weight) in points.iter().zip(weights).rev() {
            let frequency = weight * probability;
            let error = palette.map(|entry| {
                let d = subtract(point, entry);
                d[0] * d[0] * frequency + d[1] * d[1] * frequency + d[2] * d[2] * frequency
            });
            // This form of the minimum preserves rounding on nearly black blocks.
            let first = pair(error[0], error[1]);
            let second = if colors == 4 {
                pair(error[2], error[3])
            } else {
                2.0 * error[2]
            };
            rank += pair(first, second) * 0.25;
        }
        if rank < best_rank {
            best = candidate;
            best_rank = rank;
        }
    }
    let palette = best.palette(true);
    let mut total = 0.0;
    for (i, (&point, &weight)) in points.iter().zip(weights).enumerate() {
        let (index, distance) = nearest(point, &palette[..colors]);
        best.selectors[i] = index as u8;
        total += distance * weight;
    }
    (best, total)
}

fn gray_candidates() -> &'static [Candidate; 256] {
    static TABLE: OnceLock<[Candidate; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut output = [Candidate::new([[0; 3]; 2], 4); 256];
        let mut scores = [(u16::MAX, u16::MAX); 256];
        for (colors, selector, a_weight, b_weight) in
            [(4, 0, 1, 0), (4, 2, 2, 1), (3, 2, 1, 1), (4, 3, 1, 2)]
        {
            let mut ramp = [[(u16::MAX, 0, 0); 256]; 2];
            for channel in 0..2 {
                for b in 0..=MAX_CODE[channel] {
                    for a in 0..=MAX_CODE[channel] {
                        let value = (a_weight * expand(a, channel) + b_weight * expand(b, channel))
                            / (a_weight + b_weight);
                        for (target, best) in ramp[channel].iter_mut().enumerate() {
                            let error = (target as u16).abs_diff(value);
                            if error < best.0 {
                                *best = (error, a, b);
                            }
                        }
                    }
                }
            }
            for i in 0..256 {
                let (dr, ar, br) = ramp[0][i];
                let (dg, ag, bg) = ramp[1][i];
                let endpoints = [[ar, ag, ar], [br, bg, br]];
                let lower = pack_endpoint(endpoints[0]).min(pack_endpoint(endpoints[1]));
                let score = (2 * dr * dr + dg * dg, lower);
                if score < scores[i] {
                    scores[i] = score;
                    output[i] = Candidate::new(endpoints, colors);
                    output[i].selectors.fill(selector);
                }
            }
        }
        output
    })
}

fn solid(rgb: Color, requested: usize) -> Candidate {
    let mut best = (Candidate::new([[0; 3]; 2], requested), f64::INFINITY);
    let target = rgb.map(|value| value * 255.0);
    for (mode, grid) in solid_grid().iter().enumerate().take(requested - 1) {
        let mut endpoints = [[0; 3]; 2];
        let mut distance = 0.0;
        for i in 0..3 {
            let mut best_channel = f64::INFINITY;
            for (decoded, pair) in grid[i].iter().enumerate() {
                if let Some([a, b]) = *pair {
                    let delta = (decoded as f64 - target[i]).abs();
                    if delta < best_channel {
                        best_channel = delta;
                        endpoints[0][i] = a;
                        endpoints[1][i] = b;
                    }
                }
            }
            distance += (best_channel * WEIGHTS[i]).powi(2);
        }
        if distance < best.1 {
            let colors = [requested, 3, 4][mode];
            let mut candidate = Candidate::new(endpoints, colors);
            candidate.selectors.fill((mode + 1) as u8);
            best = (candidate, distance);
        }
    }
    best.0
}

type SolidGrid = [[[Option<[u16; 2]>; 256]; 3]; 3];

fn solid_grid() -> &'static SolidGrid {
    static GRID: OnceLock<SolidGrid> = OnceLock::new();
    GRID.get_or_init(|| {
        let mut grid: SolidGrid = [[[None; 256]; 3]; 3];
        for (mode, [a_weight, b_weight]) in [[1, 0], [1, 1], [1, 2]].into_iter().enumerate() {
            for i in 0..3 {
                for a in (0..=MAX_CODE[i]).rev() {
                    for b in (0..=MAX_CODE[i]).rev() {
                        let aa = expand(a, i);
                        let bb = expand(b, i);
                        let value = (a_weight * aa + b_weight * bb + u16::from(mode == 2))
                            / (a_weight + b_weight);
                        let pair = &mut grid[mode][i][usize::from(value)];
                        if pair.is_none_or(|[x, y]| {
                            aa.abs_diff(bb) < expand(x, i).abs_diff(expand(y, i))
                        }) {
                            *pair = Some([a, b]);
                        }
                    }
                }
            }
        }
        grid
    })
}

fn fast(rgb: &[Color], colors: usize) -> Candidate {
    let round = |x: f64| x as f32 as f64;
    let dot_single =
        |a: Color, b: Color| round(round(round(a[0] * b[0]) + a[1] * b[1]) + a[2] * b[2]);
    let mut points = [[0.0; 3]; 16];
    for (destination, &color) in points.iter_mut().zip(rgb) {
        *destination = weighted(color).map(round);
    }
    let points = &points[..rgb.len()];
    let solid = || {
        let endpoint = std::array::from_fn(|i| (rgb[0][i] * f64::from(MAX_CODE[i])).round() as u16);
        Candidate::new([endpoint; 2], 4)
    };
    let threshold = if colors == 3 { 1.0 / 256.0 } else { 0.0 };
    if points.windows(2).all(|pair| {
        subtract(pair[0], pair[1])
            .iter()
            .all(|x| x.abs() <= threshold)
    }) {
        return solid();
    }
    let mut mean = [0.0; 3];
    for point in points {
        for i in 0..3 {
            mean[i] = round(mean[i] + point[i]);
        }
    }
    let inv = (1.0f32 / points.len() as f32) as f64;
    mean = mean.map(|x| round(x * inv));

    let mut matrix = [[0.0; 3]; 3];
    for &point in points {
        let delta = subtract(point, mean).map(round);
        for i in 0..3 {
            for j in 0..3 {
                matrix[i][j] = round(matrix[i][j] + delta[i] * delta[j]);
            }
        }
    }
    let axis = principal_axis(matrix, 2, true);
    if axis == [0.0; 3] {
        return solid();
    }
    let mut extent = [f64::NEG_INFINITY, f64::INFINITY];
    for &point in points {
        let pos = dot_single(subtract(point, mean).map(round), axis);
        extent[0] = extent[0].max(pos);
        extent[1] = extent[1].min(pos);
    }
    let mut ends = extent.map(|pos| std::array::from_fn(|i| round(mean[i] + pos * axis[i])));
    for i in 0..3 {
        for edge in [0.0, WEIGHTS[i]] {
            let outside = ends.map(|point| {
                if edge == 0.0 {
                    point[i] < edge
                } else {
                    point[i] > edge
                }
            });
            if outside[0] != outside[1] {
                let index = usize::from(outside[1]);
                let delta = subtract(ends[1 - index], ends[index]);
                let t = (edge - ends[index][i]) / delta[i];
                for (value, component) in ends[index].iter_mut().zip(delta) {
                    *value = round(*value + t * component);
                }
            }
        }
    }
    let scaled = ends.map(|point| {
        std::array::from_fn::<_, 3, _>(|i| {
            round(round(point[i] / WEIGHTS[i]) * f64::from(MAX_CODE[i]))
                .clamp(0.0, f64::from(MAX_CODE[i]))
        })
    });
    let mut endpoints = scaled.map(|point| point.map(|x| x.round_ties_even() as u16));
    for i in 0..3 {
        if endpoints[0][i] == endpoints[1][i] {
            endpoints[0][i] = scaled[0][i].ceil() as u16;
            endpoints[1][i] = scaled[1][i].floor() as u16;
        }
    }
    let [lo, hi] = endpoints.map(pack_endpoint);
    if lo == hi {
        return solid();
    }
    if lo < hi {
        endpoints.swap(0, 1);
    }
    let [a, b] = endpoints.map(|p| {
        std::array::from_fn(|i| round(round(f64::from(p[i]) / f64::from(MAX_CODE[i])) * WEIGHTS[i]))
    });
    let axis = subtract(b, a).map(round);
    let length = dot(axis, axis);
    let indices: &[u8] = if colors == 4 {
        &[0, 2, 3, 1]
    } else {
        &[0, 2, 1]
    };
    let mut candidate = Candidate::new(endpoints, colors);
    for (i, &point) in points.iter().enumerate() {
        let pos =
            round(dot_single(subtract(point, a).map(round), axis) / length).clamp(0.0, 1.0) as f32;
        candidate.selectors[i] = indices[(pos * (colors - 1) as f32).round_ties_even() as usize];
    }
    candidate
}
