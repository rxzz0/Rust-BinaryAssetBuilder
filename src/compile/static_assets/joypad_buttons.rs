use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::game_object::encode_inline_object_filter;
use super::global_ui::enum_value;
use super::{
    Marshaled, asset_reference, finite_number, production_queue_type, required_element, sage_uint,
    significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const STATE: &str = "ButtonSingleStateData";
const COMMAND: &str = "LogicCommand";
const POWER: &str = "SpecialPowerTemplate";
const CURSOR: &str = "RadiusCursor";

#[derive(Clone, Copy)]
enum Layout {
    Empty,
    SingleState,
    SingleStateValue(&'static str),
    Ability,
    PlayerPower,
    Targeted(&'static str, bool, bool),
    Toggle,
    Stances,
    Filter,
}

const VARIANTS: &[(&str, u32, usize, Layout)] = &[
    ("ActivateHotSpot", 0x9b99_7ffc, 8, Layout::SingleState),
    (
        "AIStrategicDirective",
        0x75bd_e74e,
        12,
        Layout::SingleStateValue("Directive"),
    ),
    ("AssembleStructure", 0xc307_f885, 12, Layout::Ability),
    ("AutoControlGroup", 0xc4d2_1bc1, 0x78, Layout::Filter),
    (
        "EnterCommandMode",
        0x216c_b87f,
        12,
        Layout::SingleStateValue("CommandMode"),
    ),
    ("Evacuate", 0xadd8_8ae2, 12, Layout::Ability),
    (
        "MultiplePowersTargetedSpecialPower",
        0x8e74_08cc,
        40,
        Layout::Targeted(COMMAND, true, false),
    ),
    ("ObjectUpgrade", 0x37ed_c3f6, 12, Layout::Ability),
    ("OpenCoopCommunicator", 0xc4e2_2487, 8, Layout::SingleState),
    ("OpenControlGroups", 0x9d43_5638, 8, Layout::SingleState),
    ("OpenPlayerPowers", 0x738a_eef4, 8, Layout::SingleState),
    ("OpenPlayerTechStore", 0xcc6a_a925, 8, Layout::SingleState),
    ("OpenStances", 0xa59d_86f6, 16, Layout::Stances),
    ("OpenUnitAbilities", 0x6bef_b673, 8, Layout::SingleState),
    ("OpenUnitsMenu", 0x1e74_8194, 8, Layout::SingleState),
    ("PlaceReadyStructure", 0x7ef2_b3a7, 4, Layout::Empty),
    (
        "PlayerControlGroup",
        0xa8ea_2a56,
        12,
        Layout::SingleStateValue("GroupShortcut"),
    ),
    ("PlayerPower", 0xb644_6129, 12, Layout::PlayerPower),
    (
        "PlayerMultiplePowersTargetedPower",
        0x0fad_eeb4,
        40,
        Layout::Targeted(POWER, true, false),
    ),
    (
        "PlayerMultipleTargetsTargetedPower",
        0xeac6_b2a7,
        48,
        Layout::Targeted(POWER, false, true),
    ),
    (
        "PlayerTargetedPower",
        0xb105_f038,
        24,
        Layout::Targeted(POWER, false, false),
    ),
    ("PlayerUpgrade", 0x2c1d_232c, 12, Layout::Ability),
    (
        "ProductionQueueType",
        0xd06d_f1ec,
        12,
        Layout::SingleStateValue("QueueType"),
    ),
    ("RedAlert", 0xc4f2_8e77, 8, Layout::SingleState),
    (
        "SetStance",
        0x1369_91e2,
        12,
        Layout::SingleStateValue("Stance"),
    ),
    ("SpecialPower", 0xe2c7_23b4, 12, Layout::Ability),
    (
        "TargetedSpecialPower",
        0x99a5_a139,
        24,
        Layout::Targeted(COMMAND, false, false),
    ),
    ("ToggleSpecialPower", 0x87cd_ad62, 16, Layout::Toggle),
    ("Unpack", 0x12b6_5b00, 12, Layout::Ability),
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let wrapper = required_element(&mut children, "Data", path)?;
    validate_attributes(wrapper, &[], path)?;
    let mut variants = significant_children(wrapper);
    let variant = variants
        .next()
        .filter(|child| child.tag_name().namespace() == Some(SAGE_NS))
        .filter(|_| children.next().is_none() && variants.next().is_none())
        .ok_or_else(|| {
            invalid(
                path,
                "JoypadCommandBarButtonTemplate requires one Data variant",
            )
        })?;
    let name = variant.tag_name().name();
    let &(_, kind, size, layout) = VARIANTS
        .iter()
        .find(|&&(variant, _, _, _)| variant == name)
        .ok_or_else(|| invalid(path, format!("unsupported joypad button variant {name}")))?;

    let mut data = BinaryData {
        payload: vec![0; 8],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    let start = data.allocate(4, size)?;
    data.word(start, kind);
    match layout {
        Layout::Empty => validate_leaf(variant, &[], path)?,
        Layout::SingleState | Layout::SingleStateValue(_) => {
            if let Layout::SingleStateValue(field) = layout {
                validate_leaf(variant, &["StateData", field], path)?;
                let value = single_value(variant, field, path)?;
                data.word(start + 8, value);
            } else {
                validate_leaf(variant, &["StateData"], path)?;
            }
            write_ref(
                required_ref(variant, "StateData", path)?,
                STATE,
                start + 4,
                &mut data,
                &mut out,
            )?;
        }
        Layout::Ability | Layout::PlayerPower => {
            let kind = if matches!(layout, Layout::Ability) {
                COMMAND
            } else {
                POWER
            };
            let primary = primary_attr(kind);
            validate_leaf(variant, &["StateData", primary], path)?;
            button_refs(variant, kind, false, false, start, &mut data, &mut out)?;
        }
        Layout::Targeted(kind, multiple_powers, multiple_targets) => {
            targeted(
                variant,
                kind,
                multiple_powers,
                multiple_targets,
                start,
                &mut data,
                &mut out,
            )?;
        }
        Layout::Toggle => {
            validate_attributes(variant, &["LogicCommand"], path)?;
            write_ref(
                required_ref(variant, "LogicCommand", path)?,
                COMMAND,
                start + 4,
                &mut data,
                &mut out,
            )?;
            let mut children = significant_children(variant).peekable();
            if refs(
                &mut children,
                "StateData",
                STATE,
                start + 8,
                &mut data,
                &mut out,
            )? == 0
                || children.next().is_some()
            {
                return Err(invalid(
                    path,
                    "ToggleSpecialPower requires StateData children",
                ));
            }
        }
        Layout::Stances => stances(variant, start, &mut data, &mut out)?,
        Layout::Filter => {
            validate_attributes(variant, &["StateData"], path)?;
            write_ref(
                required_ref(variant, "StateData", path)?,
                STATE,
                start + 4,
                &mut data,
                &mut out,
            )?;
            let mut children = significant_children(variant);
            let filter = required_element(&mut children, "ObjectFilter", path)?;
            if children.next().is_some() {
                return Err(invalid(path, "unexpected AutoControlGroup child"));
            }
            encode_inline_object_filter(filter, start + 8, &mut data, &mut out.reference_types)?;
        }
    }
    data.relocations.push(u32::MAX);
    if !out.imports.is_empty() {
        out.imports.push(u32::MAX);
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}

fn primary_attr(kind: &str) -> &'static str {
    if kind == COMMAND {
        COMMAND
    } else {
        "SpecialPower"
    }
}

fn button_refs(
    node: Node<'_, '_>,
    kind: &str,
    targeted: bool,
    multiple_targets: bool,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    let primary = primary_attr(kind);
    required_ref(node, primary, path)?;
    required_ref(node, "StateData", path)?;
    // The compiled import indices and the instance hash follow XML attribute order.
    for attr in node.attributes() {
        let name = attr.name();
        let value = attr.value();
        let strong = match name {
            "StateData" => Some((STATE, 8)),
            name if name == primary => Some((kind, 4)),
            "RadiusCursor" if targeted => Some((CURSOR, 12)),
            _ => None,
        };
        if let Some((kind, offset)) = strong {
            write_ref(value, kind, start + offset, data, out)?;
        }
        if multiple_targets && !value.trim().is_empty() {
            let weak = match name {
                "TargetMarkerTemplate" => Some(("GameObject", 24)),
                "TargetMarkerFXList" => Some(("FXList", 28)),
                _ => None,
            };
            if let Some((kind, offset)) = weak {
                let value = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(value, kind, path)?;
                data.word(start + offset, id.instance_id);
                out.reference_types.push(id.type_id);
            }
        }
    }
    Ok(())
}

fn targeted(
    node: Node<'_, '_>,
    kind: &str,
    multiple_powers: bool,
    multiple_targets: bool,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    let mut allowed = vec![
        primary_attr(kind),
        "StateData",
        "RadiusCursor",
        "ValidTargetOutline",
    ];
    if multiple_targets {
        allowed.extend([
            "TargetMarkerTemplate",
            "TargetMarkerFXList",
            "TargetCount",
            "MinTargetDistance",
        ]);
    }
    validate_attributes(node, &allowed, path)?;
    button_refs(node, kind, true, multiple_targets, start, data, out)?;
    let outline = enum_value(
        node.attribute("ValidTargetOutline")
            .unwrap_or("HOVER_ATTACK")
            .trim(),
        &[
            "UNKNOWN",
            "NONE",
            "SELECTED",
            "HOVER_SELECT",
            "HOVER_ATTACK",
        ],
        path,
    )?;
    data.word(start + 16, outline.wrapping_sub(1));
    if multiple_targets {
        data.word(
            start + 32,
            sage_uint(
                node.attribute("TargetCount").unwrap_or("1"),
                "TargetCount",
                path,
            )?,
        );
        data.word(
            start + 36,
            finite_number(
                node.attribute("MinTargetDistance").unwrap_or("0"),
                "MinTargetDistance",
                path,
            )?
            .to_bits(),
        );
    }

    let mut children = significant_children(node).peekable();
    let tint = children.next_if(|child| child.has_tag_name((SAGE_NS, "ValidTargetTintColor")));
    if multiple_powers {
        refs(
            &mut children,
            "AdditionalSpecialPowerTemplate",
            POWER,
            start + 24,
            data,
            out,
        )?;
    }
    if multiple_powers || multiple_targets {
        let offset = if multiple_powers { 32 } else { 40 };
        refs(
            &mut children,
            "AdditionalRadiusCursor",
            CURSOR,
            start + offset,
            data,
            out,
        )?;
    }
    if children.next().is_some() {
        return Err(invalid(
            path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    if let Some(tint) = tint {
        let channels = ["R", "G", "B"];
        validate_leaf(tint, &channels, path)?;
        let color = data.allocate(start + 20, 12)?;
        for (index, name) in channels.into_iter().enumerate() {
            let value = finite_number(required(tint, name, path)?, name, path)?;
            data.word(color + index * 4, value.to_bits());
        }
    }
    Ok(())
}

fn stances(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(node, &["MixedStanceStateData"], path)?;
    write_ref(
        required_ref(node, "MixedStanceStateData", path)?,
        STATE,
        start + 4,
        data,
        out,
    )?;
    let first = data.list(start + 8, significant_children(node).count(), 8)?;
    for (index, child) in significant_children(node).enumerate() {
        if !child.has_tag_name((SAGE_NS, "StanceSettings")) {
            return Err(invalid(path, "expected StanceSettings element"));
        }
        validate_leaf(child, &["Stance", "StateData"], path)?;
        let offset = first + index * 8;
        data.word(offset, stance(required(child, "Stance", path)?, path)?);
        write_ref(
            required_ref(child, "StateData", path)?,
            STATE,
            offset + 4,
            data,
            out,
        )?;
    }
    Ok(())
}

fn refs<'a, 'input: 'a>(
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    name: &str,
    kind: &str,
    count: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<usize> {
    let mut names = Vec::new();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
        if significant_children(child).next().is_none() {
            validate_attributes(child, &[], data.path)?;
            names.push("");
        } else {
            names.push(text_value(child, data.path)?);
        }
    }
    let first = data.list(count, names.len(), 4)?;
    for (index, name) in names.iter().enumerate() {
        write_ref(name, kind, first + index * 4, data, out)?;
    }
    Ok(names.len())
}

fn required_ref<'a>(node: Node<'a, '_>, name: &str, path: &Path) -> io::Result<&'a str> {
    node.attribute(name)
        .ok_or_else(|| invalid(path, format!("expected {name} attribute")))
}

fn write_ref(
    name: &str,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    if !name.trim().is_empty() {
        write_import(name, kind, offset, data, out)?;
    }
    Ok(())
}

fn single_value(node: Node<'_, '_>, name: &str, path: &Path) -> io::Result<u32> {
    if name == "GroupShortcut" {
        return sage_uint(node.attribute(name).unwrap_or("1"), name, path);
    }
    let value = required(node, name, path)?.trim();
    match name {
        "Directive" => Ok(enum_value(
            value,
            &[
                "Invalid",
                "Normal",
                "Assault",
                "HoldPosition",
                "HoldObject",
                "Help",
            ],
            path,
        )?
        .wrapping_sub(1)),
        "CommandMode" => enum_value(value, &["Sell", "TogglePower", "ToggleRepair"], path),
        "QueueType" => production_queue_type(value, name, path),
        "Stance" => stance(value, path),
        _ => unreachable!("joypad button field layout"),
    }
}

fn stance(value: &str, path: &Path) -> io::Result<u32> {
    enum_value(
        value.trim(),
        &["GUARD", "AGGRESSIVE", "HOLD_POSITION", "HOLD_FIRE"],
        path,
    )
}
