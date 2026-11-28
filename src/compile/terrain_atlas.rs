use super::static_assets::{declaration_id, significant_children, validate_attributes};
use super::texture::tga_to_dds;
use super::{
    CompiledAsset, Compiler, SAGE_NS, hash_source_file, hash_xml, invalid, memory_stream_hash,
    required,
};
use roxmltree::Node;
use std::{io, path::Path};

mod bc3_fit;

const TYPE_HASH: u32 = 0x18e9_13ea;
const TILE_SIZE: usize = 256;
const BORDER: usize = 16;
const STRIDE: usize = TILE_SIZE + 2 * BORDER;
const MIP_LEVELS: usize = 5;
const MAX_SIZE: usize = 8192;

type Pixel = [u8; 4];

impl Compiler<'_> {
    pub(super) fn compile_terrain_atlas(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let (name, id) = declaration_id(node, path)?;
        validate_attributes(node, &["id", "AtlasSize", "AllowLossyCompression"], path)?;
        let nominal = integer(node.attribute("AtlasSize").unwrap_or("2048"), path)? as usize;
        if nominal > MAX_SIZE {
            return Err(invalid(path, "terrain atlas size exceeds 8192"));
        }
        if !matches!(
            node.attribute("AllowLossyCompression")
                .unwrap_or("true")
                .trim(),
            "true" | "false" | "1" | "0"
        ) {
            return Err(invalid(path, "invalid terrain AllowLossyCompression"));
        }
        let tiles: Vec<_> = significant_children(node).collect();
        if tiles.is_empty() {
            return Err(invalid(path, "terrain atlas requires at least one tile"));
        }
        let (width, height) = atlas_dimensions(nominal, tiles.len(), path)?;
        let mut base = vec![[0; 4]; width * height];
        let mut normal = vec![[0; 4]; width * height];
        let columns = width / STRIDE;
        let mut records = Vec::with_capacity(tiles.len());
        let mut file_hashes = Vec::with_capacity(tiles.len() * 8);
        for (index, tile) in tiles.into_iter().enumerate() {
            if !tile.has_tag_name((SAGE_NS, "Tile")) {
                return Err(invalid(path, "expected terrain atlas Tile"));
            }
            validate_attributes(tile, &["TextureID", "BaseTexture", "NormalTexture"], path)?;
            for cell in significant_children(tile) {
                if !cell.has_tag_name((SAGE_NS, "Cell")) || super::has_content(cell) {
                    return Err(invalid(path, "expected terrain atlas Cell"));
                }
                validate_attributes(cell, &["IndexX", "IndexY"], path)?;
                for axis in ["IndexX", "IndexY"] {
                    required(cell, axis, path)?
                        .trim()
                        .parse::<u8>()
                        .map_err(|_| invalid(path, "invalid terrain atlas cell index"))?;
                }
            }
            let x = index % columns * STRIDE;
            let y = index / columns * STRIDE;
            for (attribute, pixels) in [("BaseTexture", &mut base), ("NormalTexture", &mut normal)]
            {
                let image = self.resolve(path, required(tile, attribute, path)?)?;
                let mut bytes = Vec::new();
                let file_hash = hash_source_file(&image, &mut bytes)?;
                file_hashes.extend(file_hash.to_le_bytes());
                let dds = tga_to_dds(&bytes).map_err(|error| invalid(&image, error))?;
                if dds[12..20] != [0, 1, 0, 0, 0, 1, 0, 0] {
                    return Err(invalid(&image, "expected a 256×256 terrain tile"));
                }
                let (tile, _) = dds[128..].as_chunks::<4>();
                copy_tile(pixels, width, x, y, tile);
            }
            let left = (x + BORDER) as u16;
            let top = (y + BORDER) as u16;
            records.push((
                integer(required(tile, "TextureID", path)?, path)?,
                [left, top, left + TILE_SIZE as u16, top + TILE_SIZE as u16],
            ));
        }
        records.sort_by_key(|&(id, _)| id);

        let mut payload = vec![0; 28];
        let base_size = write_dds(&mut payload, base, width, height, true);
        let normal_offset = payload.len() as u32;
        let normal_size = write_dds(&mut payload, normal, width, height, false);
        let tile_offset = payload.len() as u32;
        let count = records.len() as u32;
        for (id, bounds) in records {
            payload.extend(id.to_le_bytes());
            for edge in bounds {
                payload.extend(edge.to_le_bytes());
            }
        }
        for (slot, value) in payload[..28].as_chunks_mut::<4>().0.iter_mut().zip([
            0,
            count,
            tile_offset,
            28,
            base_size,
            normal_offset,
            normal_size,
        ]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let offsets = [12, 20, 8, u32::MAX];
        let relocations = offsets
            .iter()
            .flat_map(|offset| offset.to_le_bytes())
            .collect();
        let instance_hash = hash_xml(node, 0x467d_00b6, path)? ^ memory_stream_hash(file_hashes);
        Ok(CompiledAsset {
            id,
            type_hash: TYPE_HASH,
            instance_hash,
            name: format!("TerrainTextureAtlas:{name}"),
            source: source.to_owned(),
            payload,
            relocations,
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            custom_data: None,
            tokenized: false,
            sort_order: 79,
        })
    }
}

fn integer(text: &str, path: &Path) -> io::Result<u32> {
    let text = text.trim();
    text.strip_prefix("0x")
        .map_or_else(|| text.parse(), |hex| u32::from_str_radix(hex, 16))
        .map_err(|_| invalid(path, format!("invalid terrain atlas number {text:?}")))
}

fn atlas_dimensions(nominal: usize, count: usize, path: &Path) -> io::Result<(usize, usize)> {
    let nominal = nominal.next_power_of_two().max(512);
    let (mut width, mut height) = (nominal, nominal);
    while (width / STRIDE) * (height / STRIDE) < count {
        if width == height {
            width *= 2;
        } else {
            height *= 2;
        }
        if width > MAX_SIZE || height > MAX_SIZE {
            return Err(invalid(path, "terrain tiles exceed the maximum atlas size"));
        }
    }
    Ok((width, height))
}

fn copy_tile(pixels: &mut [Pixel], width: usize, x: usize, y: usize, tile: &[Pixel]) {
    for row in 0..STRIDE {
        let source_y = (row + TILE_SIZE - BORDER) % TILE_SIZE;
        let source = &tile[source_y * TILE_SIZE..(source_y + 1) * TILE_SIZE];
        let start = (y + row) * width + x;
        let destination = &mut pixels[start..start + STRIDE];
        destination[..BORDER].copy_from_slice(&source[TILE_SIZE - BORDER..]);
        destination[BORDER..BORDER + TILE_SIZE].copy_from_slice(source);
        destination[BORDER + TILE_SIZE..].copy_from_slice(&source[..BORDER]);
    }
}

fn write_dds(out: &mut Vec<u8>, pixels: Vec<Pixel>, width: usize, height: usize, bc3: bool) -> u32 {
    let start = out.len();
    let mut header = [0u32; 32];
    header[..8].copy_from_slice(&[
        u32::from_le_bytes(*b"DDS "),
        124,
        0x000a_1007,
        height as u32,
        width as u32,
        (width * height * if bc3 { 1 } else { 2 }) as u32,
        0,
        MIP_LEVELS as u32,
    ]);
    header[19] = 32;
    if bc3 {
        header[20] = 4;
        header[21] = u32::from_le_bytes(*b"DXT5");
    } else {
        header[20] = 0x40;
        header[22..26].copy_from_slice(&[16, 0x7c00, 0x03e0, 0x001f]);
    }
    header[27] = 0x0040_1008;
    out.extend(header.into_iter().flat_map(u32::to_le_bytes));
    for level in 0..MIP_LEVELS {
        let scale = 1 << level;
        let (mip_width, mip_height) = (width / scale, height / scale);
        if bc3 {
            let mip = (level != 0).then(|| {
                triangle_mip(&pixels, width, height, scale, |pixel, divisor| {
                    let rounded = pixel.map(|value| ((value + divisor / 2) / divisor) as u8);
                    (
                        rounded,
                        std::array::from_fn::<_, 3, _>(|i| {
                            (f64::from(pixel[i]) / f64::from(divisor)) as f32
                        }),
                        f64::from(pixel[3]) / f64::from(divisor),
                    )
                })
            });
            for y in (0..mip_height).step_by(4) {
                for x in (0..mip_width).step_by(4) {
                    let index = |i: usize| (y + i / 4) * mip_width + x + i % 4;
                    let block = if let Some(mip) = &mip {
                        let samples: [_; 16] = std::array::from_fn(|i| mip[index(i)]);
                        let pixels = samples.map(|(pixel, _, _)| pixel);
                        let colors = samples.map(|(_, rgb, _)| rgb.map(f64::from));
                        let mut block = encode_terrain_bc3(&pixels, &colors);
                        let alpha = samples.map(|(_, _, alpha)| alpha);
                        fit_filtered_alpha(&alpha, &mut block);
                        if !bc3_fit::fit_alpha_extremes(&alpha, &mut block) {
                            bc3_fit::fit_wide_alpha(&alpha, &mut block);
                        }
                        block
                    } else {
                        let pixels = std::array::from_fn(|i| pixels[index(i)]);
                        let colors = pixels.map(|[b, g, r, _]| [b, g, r].map(f64::from));
                        let mut block = encode_terrain_bc3(&pixels, &colors);
                        if block[0].saturating_sub(block[1]) > 8 {
                            bc3_fit::fit_wide_alpha(
                                &pixels.map(|pixel| f64::from(pixel[3])),
                                &mut block,
                            );
                        }
                        block
                    };
                    out.extend(block);
                }
            }
        } else if level == 0 {
            for pixel in &pixels {
                out.extend(normal_word(pixel.map(u32::from), 1).to_le_bytes());
            }
        } else {
            let mip = triangle_mip(&pixels, width, height, scale, normal_word);
            for word in mip {
                out.extend(word.to_le_bytes());
            }
        }
    }
    (out.len() - start) as u32
}

fn encode_terrain_bc3(pixels: &[Pixel; 16], colors: &[[f64; 3]; 16]) -> [u8; 16] {
    let mut block = encode_bc3(pixels);
    if colors[1..].iter().all(|color| *color == colors[0]) {
        if colors[0].iter().all(|value| value.fract() == 0.0) {
            fit_solid_rgb(pixels, &mut block);
        }
    } else {
        bc3_fit::fit_rgb(colors, &mut block);
    }
    block
}

fn normal_word([b, g, r, _]: [u32; 4], divisor: u32) -> u16 {
    // EA quantizes directly from the filtered values into the 5-bit DDS channels.
    let denominator = 255 * divisor;
    let quantize = |value| ((31 * value + denominator / 2) / denominator) as u16;
    quantize(r) << 10 | quantize(g) << 5 | quantize(b)
}

fn triangle_mip<T>(
    pixels: &[Pixel],
    width: usize,
    height: usize,
    scale: usize,
    convert: impl Fn([u32; 4], u32) -> T,
) -> Vec<T> {
    let (mip_width, mip_height) = (width / scale, height / scale);
    let weights: Vec<_> = (0..scale * 2)
        .map(|i| (2 * i.min(scale * 2 - 1 - i) + 1) as u32)
        .collect();
    let taps = |index: usize, extent: usize| {
        let origin = index * scale;
        let first = origin.saturating_sub(scale / 2);
        let skipped = (scale / 2).saturating_sub(origin);
        let end = (skipped + extent - first).min(weights.len());
        (first, &weights[skipped..end])
    };
    let columns: Vec<_> = (0..mip_width)
        .map(|x| {
            let (first, weights) = taps(x, width);
            (first, weights, weights.iter().sum::<u32>())
        })
        .collect();
    let mut horizontal = vec![[0u32; 4]; mip_width * height];
    for (source, target) in pixels
        .chunks_exact(width)
        .zip(horizontal.chunks_exact_mut(mip_width))
    {
        for (result, &(first, weights, _)) in target.iter_mut().zip(&columns) {
            for (sample, &weight) in source[first..].iter().zip(weights) {
                for (channel, &sample) in result.iter_mut().zip(sample) {
                    *channel += u32::from(sample) * weight;
                }
            }
        }
    }
    let mut output = Vec::with_capacity(mip_width * mip_height);
    for y in 0..mip_height {
        let (first, weights) = taps(y, height);
        let vertical_divisor = weights.iter().sum::<u32>();
        for (x, &(_, _, horizontal_divisor)) in columns.iter().enumerate() {
            let mut channels = [0u32; 4];
            for (tap, &weight) in weights.iter().enumerate() {
                for (channel, &sample) in channels
                    .iter_mut()
                    .zip(&horizontal[(first + tap) * mip_width + x])
                {
                    *channel += sample * weight;
                }
            }
            output.push(convert(channels, horizontal_divisor * vertical_divisor));
        }
    }
    output
}

pub(super) fn encode_bc3(pixels: &[Pixel; 16]) -> [u8; 16] {
    if pixels[1..].iter().all(|pixel| *pixel == pixels[0]) {
        let mut block = [0; 16];
        let alpha = pixels[0][3];
        block[..2].copy_from_slice(&[alpha, alpha.saturating_add(1)]);
        block[8..10].copy_from_slice(&rgb565(pixels[0]).to_le_bytes());
        if pixels[0][..3] == [0, 0, 0] {
            block[8..].copy_from_slice(&BLACK_BC3_COLOR);
        }
        return block;
    }
    let mut alpha_min = u8::MAX;
    let mut alpha_max = 0;
    let mut darkest = pixels[0];
    let mut lightest = pixels[0];
    let mut min_luma = luminance(darkest);
    let mut max_luma = min_luma;
    for &pixel in pixels {
        alpha_min = alpha_min.min(pixel[3]);
        alpha_max = alpha_max.max(pixel[3]);
        let luma = luminance(pixel);
        if luma < min_luma {
            min_luma = luma;
            darkest = pixel;
        }
        if luma > max_luma {
            max_luma = luma;
            lightest = pixel;
        }
    }
    let alpha_palette = [
        alpha_max,
        alpha_min,
        blend(alpha_max, alpha_min, 6, 7),
        blend(alpha_max, alpha_min, 5, 7),
        blend(alpha_max, alpha_min, 4, 7),
        blend(alpha_max, alpha_min, 3, 7),
        blend(alpha_max, alpha_min, 2, 7),
        blend(alpha_max, alpha_min, 1, 7),
    ];
    let (mut color0, mut color1) = (rgb565(lightest), rgb565(darkest));
    if color0 < color1 {
        std::mem::swap(&mut color0, &mut color1);
    }
    let c0 = expand565(color0);
    let c1 = expand565(color1);
    let palette = [
        c0,
        c1,
        std::array::from_fn(|i| blend(c0[i], c1[i], 2, 3)),
        std::array::from_fn(|i| blend(c0[i], c1[i], 1, 3)),
    ];
    let mut alpha_indices = 0u64;
    let mut color_indices = 0u32;
    let alpha_span = alpha_max - alpha_min;
    for (i, &[b, g, r, alpha]) in pixels.iter().enumerate() {
        let alpha_index = if alpha == alpha_max {
            0
        } else if alpha == alpha_min {
            1
        } else if alpha_span <= 7 {
            // Every interior alpha is representable after rounding the ramp.
            // EA selects the last matching ramp entry when several round alike.
            let distance = u16::from(alpha_max - alpha);
            1 + usize::from((7 * distance + 3) / u16::from(alpha_span))
        } else {
            (0..8)
                .min_by_key(|&i| alpha.abs_diff(alpha_palette[i]))
                .unwrap()
        };
        alpha_indices |= (alpha_index as u64) << (3 * i);
        let index = (0..4)
            .min_by_key(|&i| {
                let [db, dg, dr] = palette[i];
                distance(b, db) + distance(g, dg) + distance(r, dr)
            })
            .unwrap();
        color_indices |= (index as u32) << (2 * i);
    }
    let mut block = [0; 16];
    let alpha1 = if alpha_max == alpha_min {
        alpha_max.saturating_add(1)
    } else {
        alpha_min
    };
    block[..2].copy_from_slice(&[alpha_max, alpha1]);
    block[2..8].copy_from_slice(&alpha_indices.to_le_bytes()[..6]);
    if max_luma == 0 {
        block[8..].copy_from_slice(&BLACK_BC3_COLOR);
    } else {
        block[8..10].copy_from_slice(&color0.to_le_bytes());
        block[10..12].copy_from_slice(&color1.to_le_bytes());
        block[12..].copy_from_slice(&color_indices.to_le_bytes());
    }
    block
}

fn fit_filtered_alpha(samples: &[f64; 16], block: &mut [u8; 16]) {
    let high = samples.iter().copied().fold(0.0, f64::max);
    let low = samples.iter().copied().fold(255.0, f64::min);
    let span = high - low;
    if span == 0.0 || span > 8.0 || samples.iter().all(|sample| sample.fract() == 0.0) {
        return;
    }

    // Use the fractional samples to assign the ramp positions, then refit
    // its endpoints by least squares before quantizing them to bytes.
    let positions = samples.map(|alpha| (7.0 * (high - alpha) / span).round() as u8);
    let mean_position = positions.iter().map(|&p| f64::from(p)).sum::<f64>() / 16.0;
    let mean_alpha = samples.iter().sum::<f64>() / 16.0;
    let (mut variance, mut covariance) = (0.0, 0.0);
    for (&position, &sample) in positions.iter().zip(samples) {
        let offset = f64::from(position) - mean_position;
        variance += offset * offset;
        covariance += offset * (sample - mean_alpha);
    }
    let slope = covariance / variance;
    let high = (mean_alpha - slope * mean_position).round() as u8;
    let low = (mean_alpha + slope * (7.0 - mean_position)).round() as u8;
    if high <= low || block[0] <= block[1] {
        return;
    }

    let previous = u64::from_le_bytes(std::array::from_fn(
        |i| {
            if i < 6 { block[2 + i] } else { 0 }
        },
    ));
    let (mut fitted, mut rounded_gain, mut fractional_gain) = (0u64, 0.0, 0.0);
    let alpha_at = |high, low, position| {
        (f64::from(high) * (7.0 - position) + f64::from(low) * position) / 7.0
    };
    for (i, (&position, &sample)) in positions.iter().zip(samples).enumerate() {
        let old_index = ((previous >> (3 * i)) & 7) as usize;
        let old = alpha_at(
            block[0],
            block[1],
            f64::from([0, 7, 1, 2, 3, 4, 5, 6][old_index]),
        );
        let new = alpha_at(high, low, f64::from(position));
        rounded_gain += (old.round() - sample).powi(2) - (new.round() - sample).powi(2);
        fractional_gain += (old - sample).powi(2) - (new - sample).powi(2);
        fitted |= [0u64, 2, 3, 4, 5, 6, 7, 1][usize::from(position)] << (3 * i);
    }
    // When the decoded error ties, a two-cluster fit can still choose a
    // more representative pair of endpoints for the fractional samples.
    if rounded_gain > 1e-9
        || (rounded_gain.abs() <= 1e-9
            && fractional_gain > 1e-9
            && positions.iter().all(|&p| p == 0 || p == 7))
    {
        block[..2].copy_from_slice(&[high, low]);
        block[2..8].copy_from_slice(&fitted.to_le_bytes()[..6]);
    }
}

// For each byte that the interpolated ramp can represent exactly, find the
// closest pair of endpoint codes. Keeping the table local to terrain preserves
// the separate DXT1 texture path.
const fn solid_rgb_pairs(bits: u16) -> [(u16, u16); 256] {
    let mut pairs = [(u16::MAX, 0); 256];
    let mut a = 0u16;
    while a < 1 << bits {
        let first = (a << (8 - bits)) | (a >> (2 * bits - 8));
        let mut b = 0;
        while b < 1 << bits {
            let second = (b << (8 - bits)) | (b >> (2 * bits - 8));
            let value = ((2 * first + second + 1) / 3) as usize;
            let (old_a, old_b) = pairs[value];
            if old_a == u16::MAX || a.abs_diff(b) < old_a.abs_diff(old_b) {
                pairs[value] = (a, b);
            }
            b += 1;
        }
        a += 1;
    }
    pairs
}

fn fit_solid_rgb(pixels: &[Pixel; 16], block: &mut [u8; 16]) {
    const FIVE_BIT: [(u16, u16); 256] = solid_rgb_pairs(5);
    const SIX_BIT: [(u16, u16); 256] = solid_rgb_pairs(6);

    let [b, g, r, _] = pixels[0];
    if (b == g && g == r) || !pixels[1..].iter().all(|pixel| pixel[..3] == [b, g, r]) {
        return;
    }
    let channels = [
        FIVE_BIT[usize::from(b)],
        SIX_BIT[usize::from(g)],
        FIVE_BIT[usize::from(r)],
    ];
    // This fit covers chromatic solids represented by interpolation in all
    // three channels. The single-endpoint cases have separate tie behavior.
    if channels.iter().any(|&(a, b)| a == u16::MAX || a == b) {
        return;
    }
    let [(b0, b1), (g0, g1), (r0, r1)] = channels;
    let (a, b) = (r0 << 11 | g0 << 5 | b0, r1 << 11 | g1 << 5 | b1);
    let (high, low, indices) = if a > b { (a, b, 0xaa) } else { (b, a, 0xff) };
    block[8..10].copy_from_slice(&high.to_le_bytes());
    block[10..12].copy_from_slice(&low.to_le_bytes());
    block[12..].fill(indices);
}

// EA stores black as endpoint 1 with every texel selecting that endpoint.
const BLACK_BC3_COLOR: [u8; 8] = [1, 0, 0, 0, 0x55, 0x55, 0x55, 0x55];

fn luminance([b, g, r, _]: Pixel) -> u32 {
    29 * u32::from(b) + 150 * u32::from(g) + 77 * u32::from(r)
}

fn blend(a: u8, b: u8, weight: u16, total: u16) -> u8 {
    ((u16::from(a) * weight + u16::from(b) * (total - weight)) / total) as u8
}

fn distance(a: u8, b: u8) -> u32 {
    u32::from(a.abs_diff(b)).pow(2)
}

fn quantize(value: u8, max: u16) -> u16 {
    (u16::from(value) * max + 127) / 255
}

fn rgb565([b, g, r, _]: Pixel) -> u16 {
    quantize(r, 31) << 11 | quantize(g, 63) << 5 | quantize(b, 31)
}

fn expand565(value: u16) -> [u8; 3] {
    let b = (value & 31) as u8;
    let g = (value >> 5 & 63) as u8;
    let r = (value >> 11) as u8;
    [b << 3 | b >> 2, g << 2 | g >> 4, r << 3 | r >> 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_atlas_wraps_tile_edges_and_writes_dds_mips() {
        let path = Path::new("atlas.xml");
        assert_eq!(atlas_dimensions(512, 1, path).unwrap(), (512, 512));
        assert_eq!(atlas_dimensions(512, 3, path).unwrap(), (1024, 512));
        assert_eq!(atlas_dimensions(512, 4, path).unwrap(), (1024, 1024));

        let tile: Vec<Pixel> = (0..TILE_SIZE)
            .flat_map(|y| (0..TILE_SIZE).map(move |x| [x as u8, y as u8, 0, 255]))
            .collect();
        let mut atlas = vec![[0; 4]; STRIDE * STRIDE];
        copy_tile(&mut atlas, STRIDE, 0, 0, &tile);
        assert_eq!(atlas[0], [240, 240, 0, 255]);
        assert_eq!(atlas[BORDER * STRIDE + BORDER], [0, 0, 0, 255]);
        assert_eq!(atlas[STRIDE * STRIDE - 1], [15, 15, 0, 255]);

        let mut dds = Vec::new();
        let pixels = vec![[0, 255, 255, 255]; 64 * 64];
        assert_eq!(write_dds(&mut dds, pixels, 64, 64, false), 11_040);
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[28..32], &5u32.to_le_bytes());
        assert_eq!(&dds[92..104], &[0, 124, 0, 0, 224, 3, 0, 0, 31, 0, 0, 0]);
        assert!(
            dds[128..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|word| *word == [224, 127])
        );
    }

    #[test]
    fn terrain_normal_mips_trim_edge_taps_and_quantize_after_filtering() {
        let row = [
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 210, 255],
            [0, 0, 0, 255],
        ];
        let pixels = row.repeat(4);
        let mip = triangle_mip(&pixels, 4, 4, 2, normal_word);
        assert_eq!(mip[0], 4 << 10);
        assert_eq!(normal_word([61 * 64 + 32, 0, 0, 0], 64), 7);
    }

    #[test]
    fn bc3_matches_ea_for_empty_blocks_and_constant_alpha() {
        assert_eq!(
            encode_bc3(&[[0; 4]; 16]),
            [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0x55, 0x55, 0x55, 0x55]
        );
        assert_eq!(
            encode_bc3(&[[0, 0, 0, 255]; 16]),
            [
                255, 255, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0x55, 0x55, 0x55, 0x55
            ]
        );
        let pixels = std::array::from_fn(|i| [i as u8, 0, 0, 80]);
        assert_eq!(&encode_bc3(&pixels)[..8], &[80, 81, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn bc3_matches_ea_for_a_narrow_alpha_ramp() {
        let pixels = std::array::from_fn(|i| [0, 0, 0, 32 + (i % 4) as u8]);
        assert_eq!(
            &encode_bc3(&pixels)[..8],
            &[0x23, 0x20, 0x31, 0x11, 0x13, 0x31, 0x11, 0x13]
        );
    }

    #[test]
    fn bc3_fits_fractional_mip_alpha_before_rounding_endpoints() {
        let mut samples = [33.0, 34.0, 33.0, 30.0].repeat(4);
        samples[12..].copy_from_slice(&[28.875, 29.75, 28.875, 26.25]);
        let cases = [
            (
                samples.try_into().unwrap(),
                [0x22, 0x1b, 0x82, 0x2a, 0xa8, 0x82, 0xea, 0x3a],
            ),
            (
                std::array::from_fn(|i| match i {
                    12 => 29.15625,
                    13.. => 29.3125,
                    i if i % 4 == 0 => 33.0 + 9.0 / 28.0,
                    _ => 33.5,
                }),
                [0x21, 0x1d, 0, 0, 0, 0, 0x90, 0x24],
            ),
        ];
        for (samples, expected) in cases {
            let mut block = encode_bc3(&samples.map(|alpha: f64| [0, 0, 0, alpha.round() as u8]));
            fit_filtered_alpha(&samples, &mut block);
            assert_eq!(&block[..8], &expected);
        }
    }

    #[test]
    fn bc3_fits_solid_color_with_rgb565_interpolation() {
        let pixels = std::array::from_fn(|i| [32, 64, 164, i as u8]);
        let mut block = encode_bc3(&pixels);
        let alpha = block[..8].to_vec();
        fit_solid_rgb(&pixels, &mut block);
        assert_eq!(block[..8], alpha);
        assert_eq!(
            &block[8..],
            &[0x46, 0xb2, 0xe3, 0x99, 0xff, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn terrain_bc3_fits_a_varying_color_block() {
        let colors = [
            [96, 64, 116],
            [99, 64, 138],
            [102, 64, 160],
            [105, 65, 181],
            [103, 81, 138],
            [106, 81, 160],
            [109, 81, 182],
            [112, 82, 203],
            [110, 98, 160],
            [113, 98, 182],
            [116, 98, 204],
            [119, 99, 226],
            [117, 115, 181],
            [120, 115, 203],
            [123, 115, 226],
            [126, 116, 248],
        ];
        let pixels = colors.map(|[b, g, r]| [b, g, r, 255]);
        let block = encode_terrain_bc3(&pixels, &colors.map(|color| color.map(f64::from)));
        assert_eq!(
            &block[8..],
            &[0x8f, 0xd3, 0xec, 0x89, 0xd5, 0xbf, 0x2a, 0x00]
        );
    }

    #[test]
    fn terrain_bc3_fits_wide_alpha_with_reserved_extremes() {
        let alpha = [
            253, 254, 255, 0, 253, 254, 255, 0, 254, 255, 0, 1, 254, 255, 0, 1,
        ];
        let mut block = encode_bc3(&alpha.map(|a| [0, 0, 0, a]));
        bc3_fit::fit_wide_alpha(&alpha.map(f64::from), &mut block);
        assert_eq!(&block[..8], &[1, 254, 0xc9, 0x9d, 0xdc, 0xb9, 0x91, 0x1b]);
    }

    #[test]
    fn terrain_bc3_rounds_two_color_endpoints_in_rgb565_space() {
        let colors = std::array::from_fn(|i| {
            if i % 4 == 0 {
                [4.0, 8.0, 20.5]
            } else {
                [0.0; 3]
            }
        });
        let pixels = colors.map(|color: [f64; 3]| {
            let [b, g, r] = color.map(|value| value.round() as u8);
            [b, g, r, 255]
        });
        let block = encode_terrain_bc3(&pixels, &colors);
        assert_eq!(&block[8..], &[0x41, 0x18, 0, 0, 0x54, 0x54, 0x54, 0x54]);
    }

    #[test]
    fn terrain_bc3_fits_both_endpoints_of_a_narrow_color_block() {
        let pixels = std::array::from_fn(|i| [158, 240 + (i % 4) as u8, 240 + (i / 4) as u8, 255]);
        let colors = pixels.map(|[b, g, r, _]| [b, g, r].map(f64::from));
        let block = encode_terrain_bc3(&pixels, &colors);
        assert_eq!(
            &block[8..],
            &[0x73, 0xf7, 0x93, 0xef, 0x5f, 0x7f, 0x7e, 0x7e]
        );
    }

    #[test]
    fn terrain_bc3_can_interpolate_a_constant_channel() {
        let pixels = std::array::from_fn(|i| [158, 60 + (i % 4) as u8, 240 + (i / 4) as u8, 255]);
        let colors = pixels.map(|[b, g, r, _]| [b, g, r].map(f64::from));
        let block = encode_terrain_bc3(&pixels, &colors);
        assert_eq!(
            &block[8..],
            &[0x14, 0xf2, 0xf3, 0xe9, 0xfd, 0xbf, 0xbf, 0xaf]
        );
    }
}
