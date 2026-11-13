use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, hash_name, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields};
use super::spawn::{UPGRADE, id_list, weak_reference};
use super::tech_upgrade::encode_anim;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x5f06_ba81;
const BOOL: u32 = 0x77e3_f4f7;
const TIME: u32 = 0x9986_69e8;
const UPGRADE_REF: u32 = 0x26fa_cfc7;
const ANIM: u32 = 0x6b74_6af6;
const NUGGET_FIELDS: &[(&str, u32, usize)] = &[
    ("FadeTime", TIME, 0x10),
    ("WaitBeforeFadeTime", TIME, 0x14),
    ("SkipFadeOnCreate", BOOL, 0x28),
    ("HideSubObjectsOnRemove", BOOL, 0x29),
    ("UnHideSubObjectsOnRemove", BOOL, 0x2a),
    ("Upgrade", UPGRADE_REF, 0x18),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let anim = children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration")));
    let mut upgrades = [Vec::new(), Vec::new()];
    for (name, ids) in ["TriggeredBy", "ConflictsWith"]
        .into_iter()
        .zip(&mut upgrades)
    {
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            ids.push(weak_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        }
    }
    let nuggets: Vec<_> = children.collect();
    if nuggets.is_empty()
        || nuggets
            .iter()
            .any(|child| !child.has_tag_name((SAGE_NS, "Nugget")))
    {
        return Err(invalid(
            path,
            "SubObjectsUpgrade requires one or more Nugget elements",
        ));
    }

    let start = data.allocate(pointer, 0x2c)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, _, offset) in UPGRADE {
        data.payload[start + offset] = boolean(node, name, name == "WaypointModeTerminal", path)?;
    }
    let count = start + 0x24;
    let first = data.list(count, nuggets.len(), 0x2c)?;
    let mut items = Vec::with_capacity(nuggets.len());
    for (i, nugget) in nuggets.into_iter().enumerate() {
        let table = encode_nugget(nugget, first + i * 0x2c, data, references, reference_types)?;
        items.push((0, Target::Table(table)));
    }

    let anim = match anim {
        Some(node) => Target::Table(encode_anim(node, start + 8, data)?),
        None => Target::Null,
    };
    let [triggers, conflicts] = upgrades;
    let triggers = id_list(data, start + 0xc, triggers)?;
    let conflicts = id_list(data, start + 0x14, conflicts)?;
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut upgrade = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    upgrade.extend(fields(start, UPGRADE));
    upgrade.extend([
        Entry::named("CustomAnimAndDuration", ANIM, anim),
        Entry::named("TriggeredBy", UPGRADE_REF, Target::Table(triggers)),
        Entry::named("ConflictsWith", UPGRADE_REF, Target::Table(conflicts)),
    ]);
    Ok((
        KIND,
        Table::record(vec![
            base(0xae60_ae98, Table::record(upgrade)),
            Entry::named(
                "Nugget",
                0x7f67_c232,
                Target::Table(Table::array(count, items)),
            ),
        ]),
    ))
}

fn encode_nugget(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = ["ShowSubObjects", "HideSubObjects"]
        .into_iter()
        .chain(NUGGET_FIELDS.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let mut textures = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "UpgradeTexture")))
    {
        textures.push(child);
    }
    let anim = children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SubObjectsUpgrade Nugget child"));
    }

    let value = required(node, "Upgrade", path)?;
    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(plain, "UpgradeTemplate", path)?;
    reference_types.push(id.type_id);
    data.word(start + 0x18, id.instance_id);

    let mut entries = Vec::new();
    for (name, offset) in [("ShowSubObjects", 0), ("HideSubObjects", 8)] {
        let values = node.attribute(name).unwrap_or("").split_ascii_whitespace();
        let count = start + offset;
        let first = data.list(count, values.clone().count(), 8)?;
        let mut items = Vec::new();
        for (i, value) in values.enumerate() {
            let offset = first + i * 8;
            data.string(offset, value)?;
            items.push((0, Target::Data(offset)));
        }
        entries.push(Entry::named(
            name,
            0xe798_0c56,
            Target::Table(Table::array(count, items)),
        ));
    }
    for &(name, kind, offset) in NUGGET_FIELDS {
        if kind == BOOL {
            data.payload[start + offset] = boolean(node, name, false, path)?;
        } else if kind == TIME {
            let default = if name == "FadeTime" { "0.5s" } else { "0s" };
            let time = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
            data.word(start + offset, time.to_bits());
        }
    }
    entries.extend(fields(start, NUGGET_FIELDS));

    let count = start + 0x1c;
    let first = data.list(count, textures.len(), 8)?;
    let mut items = Vec::with_capacity(textures.len());
    for (i, texture) in textures.into_iter().enumerate() {
        let offset = first + i * 8;
        validate_attributes(texture, &["Original", "New"], path)?;
        if has_content(texture) {
            return Err(invalid(path, "unexpected UpgradeTexture content"));
        }
        for attr in texture.attributes() {
            let value = attr.value();
            if value.trim().is_empty() {
                continue;
            }
            let plain = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(plain, "Texture", path)?;
            references.push(id.into());
            reference_types.push(id.type_id);
            let field = if attr.name() == "Original" { 0 } else { 4 };
            data.word(offset + field, references.len() as u32);
        }
        let table = Table::record(
            fields(
                offset,
                &[("Original", 0x4c3e_3938, 0), ("New", 0x4c3e_3938, 4)],
            )
            .collect(),
        );
        items.push((0, Target::Table(table)));
    }
    entries.push(Entry::named(
        "UpgradeTexture",
        0x6e5d_40fb,
        Target::Table(Table::array(count, items)),
    ));
    let anim = match anim {
        Some(node) => Target::Table(encode_anim(node, start + 0x24, data)?),
        None => Target::Null,
    };
    entries.push(Entry::named("CustomAnimAndDuration", ANIM, anim));
    Ok(Table::record(entries))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn subobjects_upgrade_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-subobjects.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-subobjects.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-subobjects.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-subobjects.relo");
        let manifest = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-subobjects.manifest"
        ))
        .unwrap();
        let expected = manifest
            .assets
            .iter()
            .find(|asset| asset.id.type_id == 0x942f_ff2d)
            .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.references.iter())
        );
        assert_eq!(actual.instance_hash, expected.instance_hash);
    }
}
