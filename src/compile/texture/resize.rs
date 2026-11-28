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
