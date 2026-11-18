use super::{
    BinaryData, Entry, Marshaled, Table, Target, encode_effect, finite_number, flag, invalid,
    nugget_reference, significant_children, validate_effect_attributes,
};
use roxmltree::Node;
use std::io;

const LUA_EVENT: u32 = 0x77db_65eb;
const SEED_TIBERIUM: u32 = 0x68ba_2049;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<(u32, Table)> {
    let (kind, table) = match node.tag_name().name() {
        "LuaEventNugget" => (LUA_EVENT, lua_event(node, pointer, data, out)?),
        "SeedTiberiumNugget" => (SEED_TIBERIUM, seed_tiberium(node, pointer, data, out)?),
        name => {
            return Err(invalid(
                data.path,
                format!("unsupported weapon nugget {name}"),
            ));
        }
    };
    Ok((kind, table))
}

fn lua_event(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const SEND_TO: [&str; 3] = ["SendToEnemies", "SendToAllies", "SendToNeutral"];
    let path = data.path;
    validate_effect_attributes(
        node,
        &[
            "EventName",
            "SendToEnemies",
            "SendToAllies",
            "SendToNeutral",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x34)?;
    data.word(start, LUA_EVENT);
    if let Some(name) = node.attribute("EventName") {
        data.string(start + 0x28, name)?;
    }
    let mut children = significant_children(node).peekable();
    let mut entries = vec![
        encode_effect(node, start, data, out, &mut children)?,
        Entry::named("EventName", 0x1a29_33aa, Target::Data(start + 0x28)),
    ];
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LuaEventNugget child"));
    }
    for (index, name) in SEND_TO.into_iter().enumerate() {
        let offset = start + 0x30 + index;
        data.payload[offset] = flag(node, name, false, path)?;
        entries.push(Entry::named(name, 0x77e3_f4f7, Target::Data(offset)));
    }
    Ok(Table::record(entries))
}

fn seed_tiberium(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    let path = data.path;
    validate_effect_attributes(
        node,
        &[
            "WeaponOCL",
            "TargetAsPrimaryObject",
            "FieldAmount",
            "SpawnedInFieldBonus",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x38)?;
    data.word(start, SEED_TIBERIUM);
    let ocl = nugget_reference(node.attribute("WeaponOCL"), "ObjectCreationList", path, out)?;
    data.word(start + 0x28, ocl);
    data.payload[start + 0x2c] = flag(node, "TargetAsPrimaryObject", false, path)?;
    let mut children = significant_children(node).peekable();
    let effect = encode_effect(node, start, data, out, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SeedTiberiumNugget child"));
    }
    let base = Table::record(vec![
        effect,
        Entry::named("WeaponOCL", 0x42e4_e9d1, Target::Data(start + 0x28)),
        Entry::named(
            "TargetAsPrimaryObject",
            0x77e3_f4f7,
            Target::Data(start + 0x2c),
        ),
    ]);
    let mut entries = vec![Entry::new(1, 0xc606_7c3f, Target::Table(base))];
    for (name, offset) in [
        ("FieldAmount", start + 0x30),
        ("SpawnedInFieldBonus", start + 0x34),
    ] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(offset, value.to_bits());
        entries.push(Entry::named(name, 0x6458_64f3, Target::Data(offset)));
    }
    Ok(Table::record(entries))
}
