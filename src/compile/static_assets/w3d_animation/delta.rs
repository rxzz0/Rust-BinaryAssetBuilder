use super::super::binary::BinaryData;
use super::{CompressionSettings, Frame};
use std::{io, sync::OnceLock};

const BLOCK_FRAMES: usize = 16;
const CHANNEL_KIND: u32 = 0x77f9_7305;
const CHANNEL_SIZE: usize = 0x2c;

pub(super) struct Delta {
    initial: [f32; 4],
    vector_len: usize,
    frame_count: usize,
    scale: f32,
    bits: u32,
    bytes: Vec<u8>,
}

struct Block {
    deltas: [i8; BLOCK_FRAMES],
    last: f32,
    total_error: f32,
    max_error: f32,
}

fn quantize(values: &[f32; BLOCK_FRAMES], first: f32, scale: f32, limit: i32) -> Block {
    let inverse_scale = 1.0 / scale;
    let mut block = Block {
        deltas: [0; BLOCK_FRAMES],
        last: first,
        total_error: 0.0,
        max_error: 0.0,
    };
    for (out, &value) in block.deltas.iter_mut().zip(values) {
        let distance = value - block.last;
        let magnitude = (f64::from(distance.abs()) * f64::from(inverse_scale) + 0.5).floor() as i32;
        let signed = if value < block.last {
            -magnitude
        } else {
            magnitude
        };
        let delta = signed.clamp(-limit, limit - 1);
        *out = delta as i8;
        block.last += delta as f32 * scale;
        let error = (block.last - value).abs();
        block.total_error += error;
        block.max_error = block.max_error.max(error);
    }
    block
}

fn filters() -> &'static [f32; 256] {
    static FILTERS: OnceLock<[f32; 256]> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let mut filters = [0.0; 256];
        filters[..16].copy_from_slice(&[
            1e-8,
            1e-7,
            1e-6,
            1e-5,
            1e-4,
            1e-3,
            0.01,
            0.1,
            1.0,
            10.0,
            100.0,
            1000.0,
            10_000.0,
            100_000.0,
            1_000_000.0,
            10_000_000.0,
        ]);
        for (position, filter) in filters[16..].iter_mut().enumerate() {
            let fraction = (f64::from(position as u32) / 240.0) as f32;
            let radians = 90.0 * f64::from(fraction) * f64::from(std::f32::consts::PI) / 180.0;
            *filter = (1.0 - radians.sin()) as f32;
        }
        filters
    })
}

pub(super) fn compress(
    frames: &[Frame],
    _kind: u32,
    vector_len: usize,
    settings: &CompressionSettings,
) -> Option<Delta> {
    if frames.len() < 2 {
        return None;
    }
    let frame_count = frames.len().div_ceil(BLOCK_FRAMES) * BLOCK_FRAMES;
    let initial = frames[0].values;
    let mut max_delta: f32 = 0.0;
    for pair in frames.windows(2) {
        for axis in 0..vector_len {
            max_delta = max_delta.max((pair[1].values[axis] - pair[0].values[axis]).abs());
        }
    }
    let scale = max_delta / 7.0;
    let blocks = frame_count / BLOCK_FRAMES * vector_len;
    if scale == 0.0 {
        // All quantized steps are zero. Check the original values in case the scale underflowed.
        if frames.iter().any(|frame| {
            frame.values[..vector_len]
                .iter()
                .zip(initial)
                .any(|(&value, first)| (value - first).abs() > settings.max_adaptive_delta_error)
        }) {
            return None;
        }
        return Some(Delta {
            initial,
            vector_len,
            frame_count,
            scale,
            bits: 4,
            bytes: vec![0; blocks * 9],
        });
    }
    if !scale.is_finite() {
        return None;
    }
    for (bits, limit, byte_count) in [(4, 8, 9), (8, 128, 17)] {
        let mut bytes = Vec::with_capacity(blocks * byte_count);
        let mut state = initial;
        let mut excess_error = false;
        for first in (1..=frame_count).step_by(BLOCK_FRAMES) {
            for (axis, current) in state[..vector_len].iter_mut().enumerate() {
                let mut previous = *current;
                let values = std::array::from_fn(|index| {
                    if let Some(frame) = frames.get(first + index) {
                        previous = frame.values[axis];
                    }
                    previous
                });
                let (filter, block) = filters()
                    .iter()
                    .enumerate()
                    .filter_map(|(filter, &factor)| {
                        let quantum = factor * scale / if bits == 4 { 1.0 } else { 16.0 };
                        (quantum.is_finite() && quantum > 0.0)
                            .then(|| (filter, quantize(&values, *current, quantum, limit)))
                    })
                    .min_by(|(_, left), (_, right)| {
                        left.total_error.total_cmp(&right.total_error)
                    })?;
                if block.max_error > settings.max_adaptive_delta_error {
                    excess_error = true;
                    break;
                }
                *current = block.last;
                bytes.push(filter as u8);
                if bits == 4 {
                    bytes.extend(
                        block
                            .deltas
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| (pair[0] as u8 & 0x0f) | (pair[1] as u8) << 4),
                    );
                } else {
                    bytes.extend(block.deltas.map(|delta| delta as u8));
                }
            }
            if excess_error {
                break;
            }
        }
        if !excess_error {
            return Some(Delta {
                initial,
                vector_len,
                frame_count,
                scale,
                bits,
                bytes,
            });
        }
    }
    None
}

impl Delta {
    pub(super) fn estimate_size(&self) -> usize {
        // Stock selection truncates bits per step to whole bytes and excludes filter bytes.
        self.vector_len * 4
            + self.vector_len * (self.bits as usize / 8) * self.bytes.len()
                / if self.bits == 4 { 9 } else { 17 }
    }

    pub(super) fn encode(
        &self,
        kind: u32,
        pivot: u16,
        pointer: usize,
        data: &mut BinaryData<'_>,
    ) -> io::Result<()> {
        let start = data.allocate(pointer, CHANNEL_SIZE)?;
        for (offset, word) in [
            (0, CHANNEL_KIND),
            (4, kind),
            (8, self.frame_count as u32),
            (12, self.vector_len as u32),
            (16, u32::from(pivot)),
            (20, self.scale.to_bits()),
            (32, self.bits),
        ] {
            data.word(start + offset, word);
        }
        let initial = data.list(start + 24, self.vector_len, 4)?;
        for (index, value) in self.initial[..self.vector_len].iter().enumerate() {
            data.word(initial + index * 4, value.to_bits());
        }
        data.word(start + 36, self.bytes.len() as u32);
        let compressed = data.allocate(start + 40, self.bytes.len().next_multiple_of(4))?;
        data.payload[compressed..compressed + self.bytes.len()].copy_from_slice(&self.bytes);
        Ok(())
    }
}
