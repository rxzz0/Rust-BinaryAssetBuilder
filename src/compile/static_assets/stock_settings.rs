use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, finite_number, percentage, sage_int, sage_time, sage_uint, significant_children,
    text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

fn binary(size: usize, path: &Path) -> BinaryData<'_> {
    BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    }
}

fn children<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Vec<Node<'a, 'input>>> {
    significant_children(node)
        .map(|child| {
            if child.has_tag_name((SAGE_NS, name)) {
                Ok(child)
            } else {
                Err(invalid(path, format!("expected {name} child")))
            }
        })
        .collect()
}

fn leaf_text(node: Node<'_, '_>, path: &Path) -> io::Result<String> {
    validate_attributes(node, &[], path)?;
    if node.children().any(|child| child.is_element()) {
        return Err(invalid(
            path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    Ok(node
        .children()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .collect())
}
pub(super) fn mesh(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let rules = children(node, "Rule", path)?;
    let mut data = binary(12, path);
    let start = data.list(4, rules.len(), 12)?;
    for (index, rule) in rules.into_iter().enumerate() {
        validate_leaf(rule, &["MeshID", "Instances"], path)?;
        let offset = start + index * 12;
        if let Some(mesh) = rule.attribute("MeshID") {
            data.string(offset, mesh)?;
        }
        data.word(
            offset + 8,
            sage_int(
                rule.attribute("Instances").unwrap_or("0"),
                "Instances",
                path,
            )?,
        );
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn mouse(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let drag = "DragTolerance";
    let threshold = "DragThreshold";
    let in_game = "InGameUIMaxDoubleClickDistance";
    let apt = "AptHitRegionMaxDoubleClickDistance";
    validate_leaf(node, &["id", drag, threshold, in_game, apt], path)?;
    let mut data = binary(20, path);
    data.word(
        4,
        sage_int(node.attribute(drag).unwrap_or("48"), drag, path)?,
    );
    data.word(
        8,
        sage_time(
            node.attribute(threshold).unwrap_or("100ms"),
            threshold,
            path,
        )?
        .to_bits(),
    );
    data.word(
        12,
        sage_uint(node.attribute(in_game).unwrap_or("20"), in_game, path)?,
    );
    data.word(
        16,
        sage_uint(node.attribute(apt).unwrap_or("10"), apt, path)?,
    );
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn multiplayer(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const MODE: &str = "GameStatsMode";
    const MODES: &[&str] = &[
        "CAMPAIGN_GAME",
        "SKIRMISH_GAME",
        "LAN_GAME",
        "ONLINE_UNRANKED_GAME",
        "ONLINE_RANKED_1V1_GAME",
        "ONLINE_RANKED_2V2_GAME",
        "ONLINE_CLAN_1V1_GAME",
        "ONLINE_CLAN_2V2_GAME",
    ];
    validate_attributes(node, &["id"], path)?;
    let mut data = binary(44, path);
    let mut fields = significant_children(node);
    let mut next = |name| {
        let field = fields
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| invalid(path, format!("expected MultiplayerSettings {name}")))?;
        text_value(field, path)
    };
    for (index, name) in [
        "InitialCreditsVeryLow",
        "InitialCreditsLow",
        "InitialCreditsMedium",
        "InitialCreditsHigh",
        "InitialCreditsVeryHigh",
        "StartCountdownTimer",
        "MaxBeaconsPerPlayer",
    ]
    .into_iter()
    .enumerate()
    {
        data.word(4 + index * 4, sage_int(next(name)?, name, path)?);
    }
    for (index, name) in [
        "UseShroud",
        "ShowRandomPlayerTemplate",
        "ShowRandomStartPos",
        "ShowRandomColor",
    ]
    .into_iter()
    .enumerate()
    {
        data.payload[40 + index] = match next(name)?.trim() {
            "true" | "True" | "1" => 1,
            "false" | "False" | "0" => 0,
            value => return Err(invalid(path, format!("invalid {name} {value:?}"))),
        };
    }

    let modes: Vec<_> = fields.collect();
    if modes.len() > MODES.len()
        || modes
            .iter()
            .any(|item| !item.has_tag_name((SAGE_NS, "GameStatsModeData")))
    {
        return Err(invalid(
            path,
            "expected up to eight GameStatsModeData entries",
        ));
    }
    let mode_start = data.list(32, modes.len(), 16)?;
    for (index, mode) in modes.into_iter().enumerate() {
        validate_attributes(mode, &["id", MODE], path)?;
        required(mode, "id", path)?;
        let offset = mode_start + index * 16;
        if let Some(value) = mode.attribute(MODE) {
            data.word(offset + 4, enum_value(value, MODES, path)?);
        }
        let maps = children(mode, "AvailableMapName", path)?;
        if maps.is_empty() {
            return Err(invalid(path, "expected at least one AvailableMapName"));
        }
        let map_start = data.list(offset + 8, maps.len(), 8)?;
        for (index, map) in maps.into_iter().enumerate() {
            let name = leaf_text(map, path)?;
            data.string(map_start + index * 8, &name)?;
        }
    }
    data.relocations.sort_unstable();
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn unit_overlay(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const REFERENCES: &[&str] = &[
        "AmmoPipImage",
        "AmmoPipFrameImage",
        "ContainPipImage",
        "ContainPipFrameImage",
        "GarrisonPipImage",
        "UnitTypeIconBackgroundImage",
        "TargetIndicatorImage",
        "TargetIndicatorGlowImage",
        "PowerIconImageSequence",
        "RepairIconImageSequence",
        "InfiltratedIconImageSequence",
        "HealUnitIconImageSequence",
        "RepairVehicleIconImageSequence",
    ];
    const SCALES: &[(&str, usize)] = &[
        ("AmmoPipScale", 0x38),
        ("ContainPipScale", 0x40),
        ("GarrisonPipScale", 0x44),
        ("UnitTypeIconScale", 0x48),
        ("StatusIconScale", 0x4c),
        ("VeterancyIconScale", 0x50),
        ("TargetIndicatorScale", 0x54),
        ("HealingIconScale", 0x58),
    ];
    let allowed: Vec<_> = ["id", "AmmoPipRange", "HealingIconDisplayTime"]
        .into_iter()
        .chain(REFERENCES.iter().copied())
        .chain(SCALES.iter().map(|&(name, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = binary(0x68, path);
    let mut result = Marshaled::default();
    for attr in node.attributes() {
        if let Some(index) = REFERENCES.iter().position(|&name| name == attr.name())
            && !attr.value().trim().is_empty()
        {
            let target = if index < 8 {
                "PackedTextureImage"
            } else {
                "ImageSequence"
            };
            write_import(attr.value(), target, 4 + index * 4, &mut data, &mut result)?;
        }
    }
    for &(name, offset) in SCALES {
        data.word(
            offset,
            percentage(node.attribute(name).unwrap_or("100%"), name, path)?.to_bits(),
        );
    }
    let range = "AmmoPipRange";
    data.word(
        0x3c,
        sage_int(node.attribute(range).unwrap_or("0"), range, path)?,
    );
    let time = "HealingIconDisplayTime";
    data.word(
        0x5c,
        sage_time(node.attribute(time).unwrap_or("3s"), time, path)?.to_bits(),
    );

    let factions = children(node, "FactionVeterancy", path)?;
    let start = data.list(0x60, factions.len(), 12)?;
    for (index, faction) in factions.into_iter().enumerate() {
        validate_attributes(faction, &["Faction"], path)?;
        let name = required(faction, "Faction", path)?;
        let number = if name == "RANDOM" {
            u32::MAX
        } else {
            enum_value(name, &["Allies", "Soviet", "Japan", "Neutral"], path)?
        };
        let offset = start + index * 12;
        data.word(offset, number);
        let icons = children(faction, "RankIcon", path)?;
        let start = data.list(offset + 4, icons.len(), 8)?;
        for (index, icon) in icons.into_iter().enumerate() {
            validate_leaf(icon, &["Rank", "Image"], path)?;
            let offset = start + index * 8;
            data.word(
                offset,
                sage_int(required(icon, "Rank", path)?, "Rank", path)?,
            );
            let image = icon
                .attribute("Image")
                .ok_or_else(|| invalid(path, "missing RankIcon Image attribute"))?;
            if !image.trim().is_empty() {
                write_import(
                    image,
                    "PackedTextureImage",
                    offset + 4,
                    &mut data,
                    &mut result,
                )?;
            }
        }
    }
    Ok(data.finish(result))
}

pub(super) fn red_alert(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const UINTS: &[(&str, &str, usize)] = &[
        ("OrangeAreaThreatThreshold", "0", 0x04),
        ("RedAreaThreatThreshold", "0", 0x08),
        ("MaxAllowedThreat", "0", 0x0c),
        ("YellowAreaThreatDecayPerSecond", "5", 0x18),
        ("OrangeAreaThreatDecayPerSecond", "5", 0x1c),
        ("RedAreaThreatDecayPerSecond", "5", 0x20),
        ("RedAlertBuildingDamagePerSecond", "0", 0x24),
        ("RedAlertUpdatedBuildingDamagePerSecond", "0", 0x28),
    ];
    const REALS: &[(&str, usize)] = &[
        ("KillThreatMultiplier", 0x10),
        ("DeathThreatMultiplier", 0x14),
        ("YellowAlertTechPointMultiplier", 0x3c),
        ("OrangeAlertTechPointMultiplier", 0x40),
        ("RedAlertTechPointMultiplier", 0x44),
    ];
    const FLAGS: &[(&str, bool)] = &[
        ("DisableNonRepairability", false),
        ("RedAlertDisableStructureBuilding", true),
        ("RedAlertShowAllUnitsAndStructures", true),
    ];
    const EVA: &[&str] = &[
        "EvaEventToPlayOnRedAlertInitiateOwner",
        "EvaEventToPlayOnRedAlertInitiateAlly",
        "EvaEventToPlayOnRedAlertInitiateEnemy",
    ];
    const EFFECTS: &[&str] = &[
        "YellowAreaEffect",
        "OrangeAreaEffect",
        "RedAreaEffect",
        "RedAlertEffect",
    ];
    const TIME: &str = "RedAlertBuildingDamageIncreaseTime";
    let allowed: Vec<_> = ["id", TIME]
        .into_iter()
        .chain(UINTS.iter().map(|&(name, _, _)| name))
        .chain(REALS.iter().map(|&(name, _)| name))
        .chain(FLAGS.iter().map(|&(name, _)| name))
        .chain(EVA.iter().copied())
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = binary(0x5c, path);
    let mut result = Marshaled::default();
    for &(name, default, offset) in UINTS {
        data.word(
            offset,
            sage_uint(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    for &(name, offset) in REALS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or("1.0"), name, path)?.to_bits(),
        );
    }
    for (index, &(name, default)) in FLAGS.iter().enumerate() {
        data.payload[0x58 + index] = boolean(node, name, default, path)?;
    }
    data.word(
        0x2c,
        sage_time(node.attribute(TIME).unwrap_or("0s"), TIME, path)?.to_bits(),
    );
    for attr in node.attributes() {
        if let Some(index) = EVA.iter().position(|&name| name == attr.name())
            && !attr.value().trim().is_empty()
        {
            write_import(
                attr.value(),
                "EvaEvent",
                0x30 + index * 4,
                &mut data,
                &mut result,
            )?;
        }
    }
    let mut seen = [false; 4];
    for effect in significant_children(node) {
        let index = EFFECTS
            .iter()
            .position(|&name| effect.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| invalid(path, "unexpected RedAlertTemplate child"))?;
        if std::mem::replace(&mut seen[index], true) {
            return Err(invalid(path, format!("duplicate {}", EFFECTS[index])));
        }
        let value = leaf_text(effect, path)?;
        let offset = data.allocate(0x48 + index * 4, 4)?;
        if !value.trim().is_empty() {
            write_import(&value, "AttributeModifier", offset, &mut data, &mut result)?;
        }
    }
    Ok(data.finish(result))
}
