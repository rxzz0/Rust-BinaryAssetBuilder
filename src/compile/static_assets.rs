use super::{CompiledAsset, SAGE_NS, has_content, hash_xml, invalid, required};
use crate::hash::fast_hash;
use crate::manifest::AssetId;
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn compile_asset(
    node: Node<'_, '_>,
    path: &Path,
    source: &str,
) -> io::Result<CompiledAsset> {
    let type_name = node.tag_name().name();
    if !node.has_tag_name((SAGE_NS, "Video")) {
        return Err(invalid(
            path,
            format!("unsupported static asset type {type_name}"),
        ));
    }
    let id = required(node, "id", path)?;
    if id.contains(':') || has_content(node) {
        return Err(invalid(path, "invalid Video declaration"));
    }
    for attribute in node.attributes() {
        if attribute.namespace().is_some()
            || !matches!(
                attribute.name(),
                "id" | "Filename" | "Comment" | "Volume" | "SoundOverride"
            )
        {
            return Err(invalid(
                path,
                format!("unsupported Video attribute {}", attribute.name()),
            ));
        }
    }
    let filename = required(node, "Filename", path)?;
    let comment = node.attribute("Comment").unwrap_or("No Comment");
    if !comment
        .bytes()
        .all(|byte| byte == b' ' || byte.is_ascii_graphic())
    {
        return Err(invalid(path, "Video comment must be ASCII"));
    }
    let volume = node.attribute("Volume").unwrap_or("80").trim();
    let volume = volume
        .strip_suffix('%')
        .unwrap_or(volume)
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|volume| volume.is_finite())
        .ok_or_else(|| invalid(path, "invalid Video volume"))?
        * 0.01;
    let sound = match node.attribute("SoundOverride").unwrap_or("DUCK") {
        "DUCK" => 0,
        "NO_DUCK" => 1,
        _ => return Err(invalid(path, "invalid Video sound override")),
    };
    let type_hash = 0x8433_efa6;
    let instance_hash = hash_xml(node, type_hash ^ 4, path)?;
    let comment_offset = 28 + (filename.len() + 1).next_multiple_of(4) as u32;
    let mut payload = encode_words([
        0,
        filename.len() as u32,
        28,
        comment.len() as u32,
        comment_offset,
        volume.to_bits(),
        sound,
    ]);
    for text in [filename, comment] {
        payload.extend_from_slice(text.as_bytes());
        payload.resize((payload.len() + 1).next_multiple_of(4), 0);
    }
    Ok(CompiledAsset {
        id: AssetId {
            type_id: fast_hash(b"Video"),
            instance_id: fast_hash(id.to_ascii_lowercase().as_bytes()),
        },
        type_hash,
        instance_hash,
        name: format!("Video:{id}"),
        source: source.to_owned(),
        payload,
        relocations: encode_words([8, 16, u32::MAX]),
        imports: Vec::new(),
        references: Vec::new(),
    })
}

fn encode_words(words: impl IntoIterator<Item = u32>) -> Vec<u8> {
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}
