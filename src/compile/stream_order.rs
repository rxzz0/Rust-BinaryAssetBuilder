use super::{
    CompiledAsset, asset_name_order, build_log::BuildLog, static_assets::game_object::Schema,
};
use crate::manifest::AssetId;
use std::collections::{HashMap, HashSet};

pub(super) fn stable_order(
    assets: &[CompiledAsset],
    references: &[Vec<AssetId>],
    schema: Option<&Schema>,
    log: Option<&BuildLog>,
) -> Vec<usize> {
    let mut order: Vec<_> = (0..assets.len()).collect();
    if assets.len() < 2 {
        return order;
    }
    if let Some(schema) = schema {
        if let Some(log) = log {
            log.line(format_args!("Sorting by schema dependencies"));
        }
        let counts = schema.dependency_counts();
        order.sort_by_cached_key(|&index| {
            let (kind, name) = assets[index].name.split_once(':').expect("asset name");
            (
                counts.get(kind).copied().unwrap_or(0),
                kind,
                asset_name_order(name),
            )
        });
    }

    if let Some(log) = log {
        log.line(format_args!("Sorting by asset dependencies"));
    }
    let positions: HashMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(position, &index)| (assets[index].id, position))
        .collect();
    let local: Vec<Vec<_>> = order
        .iter()
        .map(|&index| {
            references[index]
                .iter()
                .filter_map(|id| positions.get(id).copied())
                .collect()
        })
        .collect();
    // The schema order already satisfies most local references.
    if local
        .iter()
        .enumerate()
        .all(|(position, targets)| targets.iter().all(|&target| target <= position))
    {
        return order;
    }

    let transitive: Vec<_> = local
        .iter()
        .map(|targets| {
            let mut visited = HashSet::new();
            let mut pending = targets.clone();
            while let Some(target) = pending.pop() {
                if visited.insert(target) {
                    pending.extend(&local[target]);
                }
            }
            visited
        })
        .collect();
    let kinds: Vec<_> = order
        .iter()
        .map(|&index| assets[index].id.type_id)
        .collect();
    let mut sorted = Vec::with_capacity(order.len());
    for current in 0..order.len() {
        let mut insert_at = 0;
        let mut type_start = 0;
        let mut dependency_end = 0;
        let mut last_kind = None;
        for (position, &previous) in sorted.iter().enumerate() {
            let kind = kinds[previous];
            if last_kind != Some(kind) {
                type_start = position;
                last_kind = Some(kind);
            }
            match (
                transitive[current].contains(&previous),
                transitive[previous].contains(&current),
            ) {
                (true, false) => {
                    insert_at = position + 1;
                    dependency_end = insert_at;
                }
                (false, true) => {
                    // Keep the dependent's type together if this asset can precede the whole group.
                    if type_start > dependency_end && kinds[current] != kind {
                        insert_at = type_start;
                    }
                    break;
                }
                _ => insert_at = position + 1,
            }
        }
        sorted.insert(insert_at, current);
    }
    sorted.into_iter().map(|position| order[position]).collect()
}
