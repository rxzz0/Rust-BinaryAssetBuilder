use super::super::binary::BinaryData;
use super::{CompressionSettings, Frame};
use std::io;

const ORIENTATION: u32 = 3;
const VISIBILITY: u32 = 4;
const TIME_CODED: u32 = 0x60d8_cf69;

pub(super) struct Timecoded {
    frames: Vec<Frame>,
    vector_len: usize,
}

pub(super) fn compress(
    frames: &[Frame],
    kind: u32,
    vector_len: usize,
    settings: &CompressionSettings,
) -> Timecoded {
    let mut frames = frames.to_vec();
    let target = ((frames.len() as f64 * f64::from(settings.force_reduction_rate)) as usize).max(1);
    let error = match kind {
        ORIENTATION => settings.max_rotation_error,
        VISIBILITY => settings.max_visibility_error,
        _ => settings.max_translation_error,
    };

    // A removal can affect only the two triples immediately preceding its slot.
    let mut scan_from = 0;
    if error >= 0.0 {
        while frames.len() >= 3 {
            let Some(index) = (scan_from..frames.len() - 2)
                .find(|&index| removable(&frames[index..index + 3], kind, vector_len, error))
            else {
                break;
            };
            frames.remove(index + 1);
            scan_from = index.saturating_sub(1);
        }
    }
    let within_tolerance = frames.len() == 2;
    while frames.len() > target && frames.len() >= 3 {
        let Some(index) = smallest_error(&frames, kind, vector_len) else {
            break;
        };
        frames.remove(index);
    }
    if frames.len() == 2
        && !frames[1].binary_move
        && (frames.len() > target
            || (within_tolerance || kind == ORIENTATION)
                && error >= 0.0
                && (kind != ORIENTATION
                    || angle_error(frames[0].values, frames[1].values) <= f64::from(error)))
    {
        frames.pop();
    }
    Timecoded { frames, vector_len }
}

fn removable(frames: &[Frame], kind: u32, vector_len: usize, tolerance: f32) -> bool {
    if frames[0].binary_move || frames[2].binary_move {
        return false;
    }
    let error = interpolation_error(frames, kind, vector_len);
    if kind == ORIENTATION {
        error <= tolerance
    } else {
        error < tolerance
    }
}

fn smallest_error(frames: &[Frame], kind: u32, vector_len: usize) -> Option<usize> {
    frames
        .windows(3)
        .enumerate()
        .filter(|(_, frames)| !(frames[0].binary_move || frames[2].binary_move))
        .map(|(index, frames)| (index + 1, interpolation_error(frames, kind, vector_len)))
        .min_by(|first, second| first.1.total_cmp(&second.1))
        .map(|(index, _)| index)
}

fn interpolation_error(frames: &[Frame], kind: u32, vector_len: usize) -> f32 {
    let start = frames[0];
    let middle = frames[1];
    let end = frames[2];
    let ratio = (middle.position - start.position) as f32 / (end.position - start.position) as f32;
    if kind == ORIENTATION {
        angle_error(middle.values, slerp(start.values, end.values, ratio)) as f32
    } else {
        // EA's timecoded reducer measures scalar interpolation against the last sample.
        start.values[..vector_len]
            .iter()
            .zip(end.values)
            .map(|(&start, end)| (start + ratio * (end - start) - end).abs())
            .fold(0.0, f32::max)
    }
}

fn dot(first: [f32; 4], second: [f32; 4]) -> f32 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2] + first[3] * second[3]
}

fn slerp(first: [f32; 4], second: [f32; 4], ratio: f32) -> [f32; 4] {
    let product = dot(first, second);
    let sign = if product < 0.0 { -1.0 } else { 1.0 };
    let (inverse, opposite) = if product.abs() < 1.0 {
        let angle = product.abs().acos();
        let inverse_sine = 1.0 / angle.sin();
        (
            ((1.0 - ratio) * angle).sin() * inverse_sine,
            (ratio * angle).sin() * inverse_sine * sign,
        )
    } else {
        (1.0 - ratio, ratio * sign)
    };
    std::array::from_fn(|index| inverse * first[index] + opposite * second[index])
}

fn angle_error(first: [f32; 4], second: [f32; 4]) -> f64 {
    // EA tests the scalar part of the product with the conjugate.
    let cosine = dot(first, second).abs();
    f64::from(cosine.min(1.0)).acos() * 2.0
}

impl Timecoded {
    pub(super) fn estimate_size(&self) -> usize {
        let count = self.frames.len();
        count.saturating_mul(count.saturating_mul(4).saturating_add(2))
    }

    pub(super) fn encode(
        &self,
        kind: u32,
        pivot: u16,
        pointer: usize,
        data: &mut BinaryData<'_>,
    ) -> io::Result<()> {
        let start = data.allocate(pointer, 0x24)?;
        let count = self.frames.len();
        let width = if count == 0 { 1 } else { self.vector_len };
        for (offset, value) in [
            (0, TIME_CODED),
            (4, kind),
            (8, count as u32),
            (12, width as u32),
            (16, u32::from(pivot)),
        ] {
            data.word(start + offset, value);
        }
        let values = data.list(start + 0x1c, count * width, 4)?;
        if count == 0 {
            return Ok(());
        }
        let codes = data.allocate(start + 0x18, (count * 2).next_multiple_of(4))?;
        data.word(start + 0x14, count as u32);
        for (index, frame) in self.frames.iter().enumerate() {
            let code = frame.position as u16 | if frame.binary_move { 0x8000 } else { 0 };
            data.payload[codes + index * 2..codes + index * 2 + 2]
                .copy_from_slice(&code.to_le_bytes());
            for (component, value) in frame.values[..width].iter().enumerate() {
                let offset = values + (index * width + component) * 4;
                data.word(offset, value.to_bits());
            }
        }
        Ok(())
    }
}
