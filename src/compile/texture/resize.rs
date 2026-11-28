pub(super) fn uncompressed_dds(
    mut decoded: Vec<u8>,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, &'static str> {
    let output_width = width.next_power_of_two();
    let output_height = height.next_power_of_two();
    let size = output_width * output_height * 4;
    let mut dds = Vec::new();
    dds.try_reserve_exact(128 + size)
        .map_err(|_| "resized DDS is too large")?;
    for (index, value) in [(12, output_height), (16, output_width), (20, size)] {
        decoded[index..index + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    dds.extend_from_slice(&decoded[..128]);

    let columns = triangle_taps(width, output_width);
    let rows = triangle_taps(height, output_height);
    let pixels = decoded[128..].as_chunks::<4>().0;
    let dither = [[15, 7, 13, 5], [3, 11, 1, 9], [12, 4, 14, 6], [0, 8, 2, 10]];
    let normalized: [f32; 256] = std::array::from_fn(|value| value as f32 * (1.0 / 255.0));
    for (y, row) in rows.iter().enumerate() {
        for (x, column) in columns.iter().enumerate() {
            let mut result = [0.0f32; 4];
            for &(source_y, vertical) in row {
                for &(source_x, horizontal) in column {
                    let weight = f64::from(vertical * horizontal);
                    let sample = pixels[source_y * width + source_x];
                    for (value, channel) in result.iter_mut().zip(sample) {
                        *value = (f64::from(*value)
                            + weight * f64::from(normalized[channel as usize]))
                            as f32;
                    }
                }
            }
            let scan_x = if y % 2 == 0 { x } else { output_width - 1 - x };
            let bias = (f64::from(dither[y % 4][scan_x % 4]) + 0.5) / 16.0;
            dds.extend(result.map(|value| (f64::from(value) * 255.0 + bias) as u8));
        }
    }
    Ok(dds)
}

pub(super) fn append_uncompressed_mips(
    mut dds: Vec<u8>,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, &'static str> {
    let levels = width.max(height).ilog2() + 1;
    let extra_size: usize = (1..levels)
        .map(|level| (width >> level).max(1) * (height >> level).max(1) * 4)
        .sum();
    dds.try_reserve_exact(extra_size)
        .map_err(|_| "mipmapped DDS is too large")?;
    for (offset, value) in [(8, 0xa1007u32), (28, levels), (108, 0x401008)] {
        dds[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    // Each level is filtered directly from the base image, before rounding to bytes.
    let base_end = dds.len();
    dds.resize(base_end + extra_size, 0);
    let (base, mipmaps) = dds.split_at_mut(base_end);
    let pixels = base[128..].as_chunks::<4>().0;
    let mut destination = mipmaps.as_chunks_mut::<4>().0.iter_mut();
    for level in 1..levels {
        filter_mip(
            pixels,
            width,
            height,
            (width >> level).max(1),
            (height >> level).max(1),
            |pixel| {
                *destination.next().unwrap() =
                    pixel.map(|value| (f64::from(value) * 255.0 + 0.5) as u8);
            },
        );
    }
    Ok(dds)
}

pub(super) fn filter_mip(
    pixels: &[[u8; 4]],
    width: usize,
    height: usize,
    output_width: usize,
    output_height: usize,
    mut write: impl FnMut([f32; 4]),
) {
    let normalized: [f32; 256] = std::array::from_fn(|value| value as f32 / 255.0);
    let mut row = vec![[0.0; 4]; width];
    let columns = mip_taps(width, output_width);
    let rows = mip_taps(height, output_height);
    for (first_y, weights) in rows {
        row.fill([0.0; 4]);
        for (y, weight) in (first_y..).zip(weights) {
            for (output, pixel) in row.iter_mut().zip(&pixels[y * width..][..width]) {
                for (value, &channel) in output.iter_mut().zip(pixel) {
                    *value = accumulate(*value, normalized[channel as usize], weight);
                }
            }
        }
        for (first_x, weights) in &columns {
            let mut output = [0.0; 4];
            for (&pixel, &weight) in row[*first_x..].iter().zip(weights) {
                for (value, channel) in output.iter_mut().zip(pixel) {
                    *value = accumulate(*value, channel, weight);
                }
            }
            write(output);
        }
    }
}

fn accumulate(sum: f32, sample: f32, weight: f64) -> f32 {
    (f64::from(sum) + f64::from(sample) * weight) as f32
}

fn mip_taps(source: usize, destination: usize) -> Vec<(usize, Vec<f64>)> {
    let scale = source / destination;
    (0..destination)
        .map(|index| {
            let start = index * scale;
            let first = start.saturating_sub(scale / 2);
            let end = (start + scale + scale / 2).min(source);
            let center = 2 * start + scale;
            let mut weights: Vec<_> = (first..end)
                .map(|x| (2 * scale - (2 * x + 1).abs_diff(center)) as f64)
                .collect();
            let normalization = 1.0 / weights.iter().sum::<f64>();
            for weight in &mut weights {
                *weight *= normalization;
            }
            (first, weights)
        })
        .collect()
}

fn triangle_taps(source_size: usize, output_size: usize) -> Vec<Vec<(usize, f32)>> {
    let ratio = output_size as f32 / source_size as f32;
    let half_inverse = f64::from(0.5 / ratio);
    let mut columns = vec![Vec::new(); output_size];
    // Integrate each half of the source pixel's triangle across the destination cells.
    // In source order, the rounded 32-bit tap weights also match EA's accumulation order.
    for source in 0..source_size {
        let mut taps: Vec<(usize, f32)> = Vec::with_capacity(5);
        for half in 0..2 {
            let origin = source as f64 + f64::from(half) - 0.5;
            let start = (origin * f64::from(ratio)) as f32;
            let end = start + ratio;
            for cell in start.floor() as i32..end.ceil() as i32 {
                let left = f64::from(start).max(f64::from(cell));
                let right = f64::from(end).min(f64::from(cell + 1));
                let rising = ((left + right) * half_inverse - origin) as f32;
                let strength = if half == 0 {
                    f64::from(rising)
                } else {
                    1.0 - f64::from(rising)
                };
                let weight = (right - left) * strength;
                let target = cell.rem_euclid(output_size as i32) as usize;
                if let Some((previous_target, previous_weight)) = taps.last_mut()
                    && *previous_target == target
                {
                    *previous_weight = (f64::from(*previous_weight) + weight) as f32;
                } else {
                    taps.push((target, weight as f32));
                }
            }
        }
        for (target, weight) in taps {
            if weight > 0.00001 {
                columns[target].push((source, weight));
            }
        }
    }
    columns
}
