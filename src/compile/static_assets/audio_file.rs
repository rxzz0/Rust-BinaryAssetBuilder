use super::binary::BinaryData;
use super::{declaration_id, encode_words, validate_attributes};
use crate::compile::{
    CompiledAsset, Compiler, CustomData, has_content, hash_source_file, hash_xml, invalid,
    memory_stream_hash, required,
};
use roxmltree::Node;
use std::io;
use std::path::Path;

mod xas;

struct AudioFileInput<'a> {
    name: &'a str,
    id: crate::manifest::AssetId,
    file: std::path::PathBuf,
    codec: u8,
    streamed: bool,
}

impl Compiler<'_> {
    pub(in crate::compile) fn compile_audio_file(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let input = self.prepare_audio_file(node, path)?;
        input.encode(node, path, source)
    }

    fn prepare_audio_file<'a>(
        &self,
        node: Node<'a, '_>,
        path: &Path,
    ) -> io::Result<AudioFileInput<'a>> {
        let (name, id) = declaration_id(node, path)?;
        validate_attributes(
            node,
            &[
                "id",
                "File",
                "PCCompression",
                "PCSampleRate",
                "PCQuality",
                "IsStreamedOnPC",
                "SubtitleStringName",
                "GUIPreset",
                "XenonSampleRate",
                "XenonCompression",
                "XenonQuality",
                "IsStreamedOnXenon",
                "PS3SampleRate",
                "PS3Compression",
                "PS3Quality",
                "IsStreamedOnPS3",
            ],
            path,
        )?;
        if has_content(node) {
            return Err(invalid(path, "unexpected AudioFile content"));
        }
        let file = self.resolve(path, required(node, "File", path)?)?;
        let compression = node.attribute("PCCompression").unwrap_or_else(|| {
            if file
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.eq_ignore_ascii_case("sounds"))
            {
                "XAS"
            } else {
                "NONE"
            }
        });
        let codec = match compression {
            "NONE" => 2,
            "XAS" => 4,
            _ => {
                return Err(invalid(
                    path,
                    format!("unsupported PC audio compression {compression}"),
                ));
            }
        };
        let streamed = match node.attribute("IsStreamedOnPC").unwrap_or("false").trim() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return Err(invalid(path, "invalid IsStreamedOnPC")),
        };
        Ok(AudioFileInput {
            name,
            id,
            file,
            codec,
            streamed,
        })
    }
}

impl AudioFileInput<'_> {
    fn encode(self, node: Node<'_, '_>, path: &Path, source: &str) -> io::Result<CompiledAsset> {
        let Self {
            name,
            id,
            file,
            codec,
            streamed,
        } = self;
        let mut bytes = Vec::new();
        let file_hash = hash_source_file(&file, &mut bytes)?;
        let wave = Wave::parse(&bytes, &file)?;
        if let Some(rate) = node.attribute("PCSampleRate")
            && rate.trim().parse::<u16>().ok() != Some(wave.rate)
        {
            return Err(invalid(path, "AudioFile resampling is unsupported"));
        }
        let frames = wave.frames();
        if frames == 0 || frames > 0x1fff_ffff {
            return Err(invalid(&file, "AudioFile is too long"));
        }
        let mut header = vec![codec, (wave.channels - 1) << 2];
        header.extend(wave.rate.to_be_bytes());
        header.extend((frames | if streamed { 0x4000_0000 } else { 0 }).to_be_bytes());
        let cdata = if codec == 4 {
            xas::encode(&wave, streamed, &header)
        } else {
            pcm_data(&wave, streamed, &header)
        };
        let mut data = BinaryData {
            payload: vec![0; 0x20],
            relocations: Vec::new(),
            path,
        };
        let file_stem = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| invalid(&file, "invalid WAV file name"))?;
        let default_subtitle = format!("DIALOGEVENT:{file_stem}SubTitle");
        data.string(
            4,
            node.attribute("SubtitleStringName")
                .unwrap_or(&default_subtitle),
        )?;
        data.word(0xc, frames);
        data.word(0x10, u32::from(wave.rate));
        data.payload[0x1c] = wave.channels;
        if streamed {
            let offset = data.allocate(0x14, header.len())?;
            data.payload[offset..].copy_from_slice(&header);
            data.word(0x18, header.len() as u32);
        }
        data.relocations.push(u32::MAX);
        const PROCESSING_HASH: u32 = 0x8339_8e45 ^ 1_001_010;
        Ok(CompiledAsset {
            id,
            type_hash: 0x53c8_1e47,
            instance_hash: hash_xml(node, PROCESSING_HASH, path)?
                ^ memory_stream_hash(file_hash.to_le_bytes().to_vec()),
            name: format!("AudioFile:{name}"),
            source: source.to_owned(),
            payload: data.payload,
            relocations: encode_words(data.relocations),
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            custom_data: Some(CustomData::Bytes(cdata)),
            tokenized: false,
            sort_order: 39,
        })
    }
}

struct Wave<'a> {
    rate: u16,
    channels: u8,
    sample_bytes: usize,
    samples: &'a [u8],
}

impl<'a> Wave<'a> {
    fn parse(bytes: &'a [u8], path: &Path) -> io::Result<Self> {
        if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
            return Err(invalid(path, "expected a RIFF/WAVE file"));
        }
        let mut chunks = &bytes[12..];
        let (mut format, mut samples) = (None, None);
        while let Some((header, rest)) = chunks.split_first_chunk::<8>() {
            let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
            let (chunk, remaining) = rest
                .split_at_checked(size)
                .ok_or_else(|| invalid(path, "truncated WAV chunk"))?;
            match &header[..4] {
                b"fmt " => format = Some(chunk),
                b"data" => samples = Some(chunk),
                _ => {}
            }
            chunks = remaining.get(size % 2..).unwrap_or_default();
        }
        let format = format
            .filter(|format| format.len() >= 16)
            .ok_or_else(|| invalid(path, "missing WAV format"))?;
        let samples = samples.ok_or_else(|| invalid(path, "missing WAV samples"))?;
        let word = |offset| u16::from_le_bytes(format[offset..offset + 2].try_into().unwrap());
        let channels = word(2);
        let rate = u32::from_le_bytes(format[4..8].try_into().unwrap());
        let supported_format = word(0) == 1
            || (word(0) == 0xfffe
                && format.get(24..40)
                    == Some(b"\x01\x00\x00\x00\x00\x00\x10\x00\x80\x00\x00\xaa\x00\x38\x9b\x71"));
        let bits = word(14);
        let sample_bytes = usize::from(bits / 8);
        if !supported_format
            || !matches!(channels, 1 | 2 | 4 | 6)
            || !matches!(bits, 8 | 16)
            || usize::from(word(12)) != usize::from(channels) * sample_bytes
            || rate == 0
            || rate > u16::MAX as u32
            || !samples
                .len()
                .is_multiple_of(usize::from(channels) * sample_bytes)
        {
            return Err(invalid(
                path,
                "expected 8- or 16-bit PCM WAV with 1, 2, 4 or 6 channels",
            ));
        }
        Ok(Self {
            rate: rate as u16,
            channels: channels as u8,
            sample_bytes,
            samples,
        })
    }

    fn frames(&self) -> u32 {
        (self.samples.len() / (self.sample_bytes * usize::from(self.channels))) as u32
    }

    fn sample(&self, index: usize) -> i16 {
        let Some(&low) = self.samples.get(index * self.sample_bytes) else {
            return 0;
        };
        if self.sample_bytes == 1 {
            (i16::from(low) - 128) << 8
        } else {
            i16::from_le_bytes([low, self.samples[index * 2 + 1]])
        }
    }
}

fn pcm_data(wave: &Wave<'_>, streamed: bool, header: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    if !streamed {
        output.extend_from_slice(header);
    }
    let frame_bytes = 2 * usize::from(wave.channels);
    let block_bytes = if streamed {
        ((2044 - 8) / frame_bytes) * frame_bytes
    } else {
        (wave.frames() as usize * frame_bytes).max(frame_bytes)
    };
    let frames = wave.frames() as usize;
    let block_frames = block_bytes / frame_bytes;
    for start in (0..frames).step_by(block_frames) {
        let count = (frames - start).min(block_frames);
        let last = streamed && start + count == frames;
        let end = block_header(&mut output, count * frame_bytes, count, last);
        for index in
            start * usize::from(wave.channels)..(start + count) * usize::from(wave.channels)
        {
            let sample = wave.sample(index);
            // The SDK's PCM encoder scales signed 16-bit samples toward zero.
            output.extend((sample - sample.signum()).to_be_bytes());
        }
        output.resize(end, 0);
    }
    output
}

fn block_header(out: &mut Vec<u8>, payload: usize, frames: usize, last: bool) -> usize {
    let end = out.len() + 8 + payload;
    let end = if last { end.next_multiple_of(64) } else { end };
    let size = (end - out.len()) as u32;
    let tag = if last { 0x8000_0000 } else { 0 };
    out.extend((size | tag).to_be_bytes());
    out.extend((frames as u32).to_be_bytes());
    end
}
