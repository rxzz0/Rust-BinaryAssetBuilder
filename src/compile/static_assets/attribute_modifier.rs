use super::ai_strategic::{boolean_bytes, enum_index, flags};
use super::binary::BinaryData;
use super::game_object::{write_model_conditions, write_status_flags};
use super::{
    Marshaled, percentage, sage_time, schema_enums, significant_children, validate_attributes,
    write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const BOOLS: [(&str, bool); 2] = [
        ("ReplaceInCategoryIfLongest", false),
        ("IgnoreIfAnticategoryActive", false),
    ];
    let allowed: Vec<_> = [
        "id",
        "Category",
        "Duration",
        "RemoveWhenDisabledByTypes",
        "StartFX",
        "EndFX",
        "ModelConditionsSet",
        "ModelConditionsClear",
        "ObjectStatusToSet",
        "StackingLimit",
        "ArmorSetType",
        "Shader",
    ]
    .into_iter()
    .chain(BOOLS.map(|(name, _)| name))
    .collect();
    validate_attributes(node, &allowed, path)?;
    static ENUMS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let [categories, modifiers] = ENUMS.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/AssetTypeAttributeModifier.xsd"),
            ["AttributeModifierCategoryType", "AttributeType"],
        )
    });
    static DISABLED: OnceLock<[Vec<String>; 1]> = OnceLock::new();
    let disabled = DISABLED.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/Includes/GlobalGameData.xsd"),
            ["DisabledType"],
        )
    });
    static ARMOR: OnceLock<[Vec<String>; 1]> = OnceLock::new();
    let armor = ARMOR.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/AssetTypeArmorTemplate.xsd"),
            ["ArmorSetType"],
        )
    });

    let value = |field, default| node.attribute(field).unwrap_or(default);
    let mut data = BinaryData {
        payload: vec![0; 60],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.word(
        4,
        enum_index(value("Category", "NONE"), categories, "Category", path)?,
    );
    data.word(
        8,
        sage_time(value("Duration", "0s"), "Duration", path)?.to_bits(),
    );
    let disabled_field = "RemoveWhenDisabledByTypes";
    data.word(
        12,
        flags(
            value(disabled_field, ""),
            &disabled[0],
            disabled_field,
            path,
        )?,
    );
    for (field, offset) in [("ModelConditionsSet", 24), ("ModelConditionsClear", 28)] {
        if let Some(raw) = node.attribute(field) {
            let start = data.allocate(offset, 60)?;
            write_model_conditions(raw, start, &mut data)?;
        }
    }
    if let Some(raw) = node.attribute("ObjectStatusToSet") {
        let start = data.allocate(32, 28)?;
        write_status_flags(raw, start, &mut data)?;
    }
    data.word(
        36,
        value("StackingLimit", "1")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, "invalid StackingLimit"))?,
    );
    data.word(
        40,
        enum_index(
            value("ArmorSetType", "INVALID"),
            &armor[0],
            "ArmorSetType",
            path,
        )?,
    );
    for attribute in node
        .attributes()
        .filter(|attr| !attr.value().trim().is_empty())
    {
        let raw = attribute.value();
        match attribute.name() {
            "StartFX" => write_import(raw, "FXList", 16, &mut data, &mut result)?,
            "EndFX" => write_import(raw, "FXList", 20, &mut data, &mut result)?,
            "Shader" => {
                let start = data.allocate(44, 4)?;
                write_import(raw, "ShaderOverride", start, &mut data, &mut result)?;
            }
            _ => {}
        }
    }
    let entries = significant_children(node);
    let start = data.list(48, significant_children(node).count(), 8)?;
    for (i, child) in entries.enumerate() {
        if !child.has_tag_name((SAGE_NS, "Modifier")) || has_content(child) {
            return Err(invalid(path, "unexpected content in AttributeModifier"));
        }
        validate_attributes(child, &["Type", "Value"], path)?;
        let offset = start + 8 * i;
        data.word(
            offset,
            enum_index(
                child.attribute("Type").unwrap_or("NONE"),
                modifiers,
                "Modifier Type",
                path,
            )?,
        );
        data.word(
            offset + 4,
            percentage(
                child.attribute("Value").unwrap_or("0"),
                "Modifier Value",
                path,
            )?
            .to_bits(),
        );
    }
    data.word(56, boolean_bytes(node, &BOOLS, path)?);
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}
