use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::ForgeError;
use crate::lock::FeastRegistryLock;
use crate::registry::{FeastRegistry, Scope};

// ---------------------------------------------------------------------------
// Pipeline ID Allocation — Layout [SS|CC|IIIIIIIIIIII]
// ---------------------------------------------------------------------------

/// Mappe le Scope vers son invariant binaire (2 bits).
fn scope_to_bits(scope: &Scope) -> u8 {
    match scope {
        Scope::Universal => 0,
        Scope::National(_) => 1,
        Scope::Diocesan(_) => 2,
    }
}

pub fn allocate_feast_ids(
    registry: &FeastRegistry,
    lock: &mut FeastRegistryLock,
    lock_path: &Path,
) -> Result<BTreeMap<String, u16>, ForgeError> {
    let mut id_map: BTreeMap<String, u16> = BTreeMap::new();

    // Compteurs par (scope_bits, category) — next free sequence par bucket
    let mut counters: BTreeMap<(u8, u8), u16> = BTreeMap::new();

    // Slugs présents dans le YAML courant
    let current_slugs: BTreeSet<&str> = registry.iter().map(|f| f.slug.as_str()).collect();

    // Tombstone les slugs disparus du corpus
    for slug in lock.entries.keys().cloned().collect::<Vec<_>>() {
        if !current_slugs.contains(slug.as_str()) {
            lock.tombstone(&slug);
        }
    }

    // Itération lexicographique garantie (BTreeMap)
    for feast in registry.iter() {
        let slug = &feast.slug;
        let scope_bits = scope_to_bits(&feast.scope); // 0=Universal, 1=National, 2=Diocesan
        let category = feast.category;

        if let Some(lock_id) = lock.get(slug) {
            // Slug connu dans le lock — vérification conflit YAML.id
            if let Some(yaml_id) = feast.id
                && yaml_id != lock_id
            {
                return Err(ForgeError::FeastIDLockConflict {
                    slug: slug.clone(),
                    yaml_id,
                    lock_id,
                });
            }
            id_map.insert(slug.clone(), lock_id);
        } else {
            // Nouveau slug — allouer le prochain ID libre dans (scope, category)
            let next = allocate_next(&mut counters, lock, scope_bits, category)?;
            lock.pin(slug, next);
            id_map.insert(slug.clone(), next);
        }
    }

    lock.save(lock_path)?;
    Ok(id_map)
}

fn allocate_next(
    counters: &mut BTreeMap<(u8, u8), u16>,
    lock: &FeastRegistryLock,
    scope: u8,
    category: u8,
) -> Result<u16, ForgeError> {
    let counter = counters.entry((scope, category)).or_insert_with(|| {
        // Initialiser au-delà du maximum déjà pinned dans ce bucket,
        // afin de ne jamais émettre un ID déjà attribué à un slug existant.
        let max_seq = lock
            .entries
            .values()
            .copied()
            .filter(|&id| ((id >> 14) as u8) == scope && (((id >> 12) & 0x3) as u8) == category)
            .map(|id| id & 0x0FFF)
            .max()
            .unwrap_or(0);
        max_seq + 1
    });
    loop {
        let candidate = build_feast_id(scope, category, *counter);
        *counter += 1;
        if *counter > 4095 {
            return Err(ForgeError::FeastIDExhausted { scope, category });
        }
        // Sauter les IDs tombstonés
        if !lock.is_tombstoned_id(candidate) {
            return Ok(candidate);
        }
    }
}

fn build_feast_id(scope: u8, category: u8, sequence: u16) -> u16 {
    ((scope as u16) << 14) | ((category as u16) << 12) | (sequence & 0x0FFF)
}
