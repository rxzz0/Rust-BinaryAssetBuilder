use super::ability_buttons::{
    EXTRA_REFS, STATE_KIND, TARGET_ATTRIBUTES, button_variant, finish_targeted,
    write_target_reference,
};
use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{Marshaled, asset_reference, finite_number, validate_attributes, write_import};
use crate::compile::{invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const MARKER_FIELDS: [&str; 4] = [
    "TargetMarkerTemplate",
    "TargetMarkerFXList",
    "TargetCount",
    "MinTargetDistance",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "SpecialPower"], path)?;
    let variant = button_variant(node, path)?;
    let (kind, size) = match variant.tag_name().name() {
        "Power" => (0, 8),
        "TargetedPower" => (0xac45_ea0c, 28),
        "MultipleTargetsTargetedPower" => (0xa498_ff1f, 52),
        "MultiplePowersTargetedPower" => (0x9c56_0552, 52),
        name => {
            return Err(invalid(
                path,
                format!("unsupported PlayerPowerButtonTemplate variant {name}"),
            ));
        }
    };
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    write_import(
        required(node, "SpecialPower", path)?,
        "SpecialPowerTemplate",
        4,
        &mut data,
        &mut out,
    )?;
    let start = data.allocate(8, size)?;
    data.word(start, kind);
    let state = required(variant, "StateData", path)?;
    if kind == 0 {
        validate_leaf(variant, &["StateData"], path)?;
        write_import(state, STATE_KIND, start + 4, &mut data, &mut out)?;
    } else {
        let multiple_targets = kind == 0xa498_ff1f;
        let mut allowed = TARGET_ATTRIBUTES.to_vec();
        if multiple_targets {
            allowed.extend(MARKER_FIELDS);
        }
        validate_attributes(variant, &allowed, path)?;
        for attr in variant.attributes() {
            let value = attr.value();
            let (target, offset) = match attr.name() {
                "TargetMarkerTemplate" => ("GameObject", 28),
                "TargetMarkerFXList" => ("FXList", 32),
                _ => {
                    write_target_reference(attr, start, &mut data, &mut out)?;
                    continue;
                }
            };
            if !value.trim().is_empty() {
                let id = asset_reference(value, target, path)?;
                data.word(start + offset, id.instance_id);
                out.reference_types.push(id.type_id);
            }
        }
        let refs: &[(&str, &str, usize)] = match variant.tag_name().name() {
            "MultipleTargetsTargetedPower" => {
                let raw = variant.attribute("TargetCount").unwrap_or("1").trim();
                let count = match raw.strip_prefix("0x") {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => raw.strip_suffix(".0").unwrap_or(raw).parse(),
                }
                .map_err(|_| invalid(path, format!("invalid TargetCount {raw:?}")))?;
                data.word(start + 36, count);
                let distance = variant.attribute("MinTargetDistance").unwrap_or("0");
                data.word(
                    start + 40,
                    finite_number(distance, "MinTargetDistance", path)?.to_bits(),
                );
                &[("AdditionalRadiusCursor", "RadiusCursor", 44)]
            }
            "MultiplePowersTargetedPower" => &EXTRA_REFS,
            _ => &[],
        };
        finish_targeted(variant, start, refs, &mut data, &mut out)?;
    }
    data.relocations.push(u32::MAX);
    out.imports.push(u32::MAX);
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}
