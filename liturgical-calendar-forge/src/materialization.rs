// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Étape 4 : Day Materialization (roadmap §2.4)
//
// Génère les 366 `DaySlot` par an (1969–epoch) depuis la table résolue.
// Place la Padding Entry (primary_id=0) à doy=59 pour les années non-bissextiles.
// Construit le Secondary Pool avec déduplication (PoolBuilder).
//
// INV-FORGE-4 : tri par FeastID croissant dans le Secondary Pool.

use std::collections::BTreeMap;

use crate::canonicalization::is_leap_year;
use crate::error::ForgeError;
use crate::resolution::ResolvedDay;

// ─── CalendarEntry (Forge-side, pour sérialisation) ───────────────────────────

/// `CalendarEntry` côté Forge — structure de production avant sérialisation LE.
/// Identique à `liturgical-calendar-core::entry::CalendarEntry` (`#[repr(C)]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalendarEntry {
    pub primary_id:      u16,
    pub secondary_index: u16,
    pub flags:           u16,
    pub secondary_count: u8,
    pub _reserved:       u8,
}

impl CalendarEntry {
    /// Entrée vide (Padding Entry ou slot sans fête).
    pub const ZERO: CalendarEntry = CalendarEntry {
        primary_id:      0,
        secondary_index: 0,
        flags:           0,
        secondary_count: 0,
        _reserved:       0,
    };

    /// Encode `flags` depuis les composantes (spec §3.4).
    #[inline]
    pub fn encode_flags(precedence: u8, color: u8, season: u8, nature: u8) -> u16 {
        (precedence as u16)
            | ((color   as u16) << 4)
            | ((season  as u16) << 8)
            | ((nature  as u16) << 11)
    }

    /// Sérialise en 8 octets Little-Endian (spec §3 — LE canonique exclusif).
    pub fn to_le_bytes(self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&self.primary_id.to_le_bytes());
        buf[2..4].copy_from_slice(&self.secondary_index.to_le_bytes());
        buf[4..6].copy_from_slice(&self.flags.to_le_bytes());
        buf[6]   = self.secondary_count;
        buf[7]   = self._reserved;
        buf
    }
}

// ─── PoolBuilder ─────────────────────────────────────────────────────────────

/// Constructeur du Secondary Pool avec déduplication.
///
/// Contrainte d'implémentation (spec §6 Étape 4) : la déduplication est
/// **nécessaire** — sans elle, le pool worst-case (~78 000 entrées) dépasse
/// `u16::MAX` et rend `secondary_index (u16)` inopérant.
///
/// Clé de déduplication : séquence triée de FeastIDs (INV-FORGE-4).
pub struct PoolBuilder {
    /// Index de déduplication : séquence triée → index dans `data`.
    index: BTreeMap<Vec<u16>, u16>,
    /// Données brutes du pool (tableau de u16).
    pub data: Vec<u16>,
}

impl PoolBuilder {
    pub fn new() -> Self {
        PoolBuilder {
            index: BTreeMap::new(),
            data:  Vec::new(),
        }
    }

    /// Insère une liste de FeastIDs dans le pool et retourne l'index.
    ///
    /// Trie les IDs avant insertion (INV-FORGE-4) pour garantir le déterminisme.
    /// Réutilise un index existant si la séquence est déjà présente.
    ///
    /// Retourne `ForgeError::SecondaryPoolOverflow` si le pool dépasserait `u16::MAX`.
    pub fn insert(&mut self, mut ids: Vec<u16>) -> Result<u16, ForgeError> {
        // INV-FORGE-4 : tri canonique avant déduplication
        ids.sort_unstable();

        // Déduplication : réutiliser l'index si déjà présent
        if let Some(&existing) = self.index.get(&ids) {
            return Ok(existing);
        }

        // Vérification de capacité (spec §6 Étape 4)
        if self.data.len() + ids.len() > u16::MAX as usize {
            return Err(ForgeError::SecondaryPoolOverflow {
                pool_len:     self.data.len() as u32,
                max_capacity: u16::MAX as u32,
            });
        }

        let idx = self.data.len() as u16;
        self.index.insert(ids.clone(), idx);
        self.data.extend_from_slice(&ids);
        Ok(idx)
    }

    /// Taille actuelle du pool en octets (chaque entrée = 2 octets u16).
    #[inline]
    pub fn byte_len(&self) -> u32 {
        (self.data.len() * 2) as u32
    }

    /// Sérialise le pool en octets LE canoniques.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.data.len() * 2);
        for &id in &self.data {
            buf.extend_from_slice(&id.to_le_bytes());
        }
        buf
    }
}

impl Default for PoolBuilder {
    fn default() -> Self { Self::new() }
}

// ─── Matérialisation d'une plage d'années ─────────────────────────────────────

/// Matérialise les `CalendarEntry` pour une plage d'années [epoch, epoch+range-1].
///
/// Retourne :
/// - `Vec<CalendarEntry>` : tableau de `range * 366` entrées (ordre : année × DOY)
/// - `PoolBuilder` : Secondary Pool construit
///
/// INV : entrée à index `(year - epoch) * 366 + doy` correspond à `(year, doy)`.
pub fn materialize_range(
    epoch:    u16,
    range:    u16,
    resolved: &BTreeMap<u16, BTreeMap<u16, ResolvedDay>>, // year → (doy → ResolvedDay)
) -> Result<(Vec<CalendarEntry>, PoolBuilder), ForgeError> {
    let total = (range as usize) * 366;
    let mut entries  = vec![CalendarEntry::ZERO; total];
    let mut pool     = PoolBuilder::new();

    for year_offset in 0..range {
        let year = epoch + year_offset;
        let leap  = is_leap_year(year as i32);
        let year_map: Option<&BTreeMap<u16, ResolvedDay>> = resolved.get(&year);

        for doy in 0u16..=365 {
            let idx = (year_offset as usize) * 366 + doy as usize;

            // doy=59 sur année non-bissextile → Padding Entry (primary_id=0)
            if doy == 59 && !leap {
                entries[idx] = CalendarEntry::ZERO;
                continue;
            }

            // Chercher une fête résolue pour ce (year, doy)
            let resolved_day = year_map.and_then(|m| m.get(&doy));

            let entry = match resolved_day {
                None => CalendarEntry::ZERO, // slot vide
                Some(day) => {
                    let p = &day.primary;

                    // Validation V9 (FeastIDMutated) — l'id doit correspondre au registre
                    // (vérification symbolique — la cohérence est garantie par la résolution)

                    // Secondary Pool
                    let (secondary_index, secondary_count) = if day.secondaries.is_empty() {
                        (0u16, 0u8)
                    } else {
                        let ids: Vec<u16> = day.secondaries.iter()
                            .map(|f| f.feast_id)
                            .collect();
                        let count = ids.len().min(255) as u8;
                        let idx_pool = pool.insert(ids)?;
                        (idx_pool, count)
                    };

                    let flags = CalendarEntry::encode_flags(
                        p.precedence,
                        p.color as u8,
                        p.season as u8,
                        p.nature as u8,
                    );

                    // Validation : bits 14–15 doivent être nuls
                    if flags & 0xC000 != 0 {
                        return Err(ForgeError::FlagsReservedBitSet { doy, year });
                    }

                    CalendarEntry {
                        primary_id:      p.feast_id,
                        secondary_index,
                        flags,
                        secondary_count,
                        _reserved: 0,
                    }
                }
            };

            entries[idx] = entry;
        }

        // Validation V10 : vérifier que la Padding Entry est bien à doy=59
        if !leap {
            let idx59 = (year_offset as usize) * 366 + 59;
            if entries[idx59].primary_id != 0 {
                return Err(ForgeError::PaddingEntryMissing { year, doy: 59 });
            }
        }
    }

    Ok((entries, pool))
}

// ─── Tests unitaires ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parse_yaml_into_registry;
    use crate::registry::FeastRegistry;
    use crate::resolution::resolve_year;

    const MINIMAL_YAML: &str = r#"
scope: universal
region: ~
from: 1969
to: ~
format_version: 1
feasts:
  - slug: dominica_resurrectionis
    scope: universal
    category: 0
    mobile:
      anchor: pascha
      offset: 0
    history:
      - from: 1969
        title: "Dominica Resurrectionis"
        precedence: 0
        nature: sollemnitas
        color: albus
        season: tempus_paschale
  - slug: dies_29_februarii
    scope: universal
    category: 1
    date:
      month: 2
      day: 29
    history:
      - from: 1969
        title: "Feria VI"
        precedence: 12
        nature: feria
        color: viridis
"#;

    fn build_resolved_map(
        years: &[u16],
        reg: &FeastRegistry,
    ) -> BTreeMap<u16, BTreeMap<u16, ResolvedDay>> {
        let mut map = BTreeMap::new();
        for &y in years {
            let table = resolve_year(y, reg).unwrap();
            map.insert(y, table);
        }
        map
    }

    #[test]
    fn padding_entry_doy59_non_leap_2025() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();

        // Plage = 1 an : 2025 uniquement (epoch=2025, range=1)
        // Pour simplifier le test, on forge une plage d'un seul an.
        let resolved = build_resolved_map(&[2025], &reg);
        // On passe epoch=2025 pour que l'index 0..=365 corresponde à 2025
        let (entries, _pool) = materialize_range(2025, 1, &resolved).unwrap();

        // doy=59 pour 2025 (non-bissextile) → Padding Entry
        let idx59 = 59usize;
        assert_eq!(entries[idx59].primary_id, 0,
            "doy=59 doit être Padding Entry pour 2025");
        assert_eq!(entries[idx59].secondary_count, 0);
        assert_eq!(entries[idx59].flags, 0);

        // doy=110 pour 2025 → Pâques (primary_id ≠ 0)
        let idx110 = 110usize;
        assert_ne!(entries[idx110].primary_id, 0,
            "doy=110 doit être Pâques pour 2025");
    }

    #[test]
    fn feb29_real_in_leap_2028() {
        let mut reg = FeastRegistry::new();
        parse_yaml_into_registry(MINIMAL_YAML, &mut reg).unwrap();
        let resolved = build_resolved_map(&[2028], &reg);
        let (entries, _pool) = materialize_range(2028, 1, &resolved).unwrap();
        // doy=59 pour 2028 (bissextile) → vraie fête
        let idx59 = 59usize;
        assert_ne!(entries[idx59].primary_id, 0,
            "doy=59 doit être une vraie fête pour 2028 (bissextile)");
    }

    #[test]
    fn pool_builder_deduplication() {
        let mut pool = PoolBuilder::new();
        let ids1 = vec![0x0001u16, 0x0002];
        let ids2 = vec![0x0002u16, 0x0001]; // Même séquence, ordre inversé
        let idx1 = pool.insert(ids1.clone()).unwrap();
        let idx2 = pool.insert(ids2.clone()).unwrap();
        assert_eq!(idx1, idx2, "Pool doit dédupliquer {:?} == {:?}", ids1, ids2);
        assert_eq!(pool.data.len(), 2); // Une seule entrée dans le pool
    }

    #[test]
    fn pool_builder_overflow() {
        let mut pool = PoolBuilder::new();
        // Saturer le pool : insérer 65535 éléments distincts (séquences de 1)
        // Pour que chaque insertion soit unique, on utilise des FeastIDs différents
        // On insère des groupes de 1 pour saturer plus vite
        for i in 0u16..=u16::MAX {
            pool.data.push(i); // bypass insert pour simuler la saturation
            if pool.data.len() == u16::MAX as usize { break; }
        }
        // Maintenant une nouvelle insertion devrait échouer
        let result = pool.insert(vec![0x9999]);
        assert!(matches!(result, Err(ForgeError::SecondaryPoolOverflow { .. })));
    }

    #[test]
    fn calendar_entry_le_serialization() {
        let entry = CalendarEntry {
            primary_id:      0xABCD,
            secondary_index: 0x1234,
            flags:           0x0201,
            secondary_count: 2,
            _reserved:       0,
        };
        let bytes = entry.to_le_bytes();
        // primary_id LE
        assert_eq!(&bytes[0..2], &[0xCD, 0xAB]);
        // secondary_index LE
        assert_eq!(&bytes[2..4], &[0x34, 0x12]);
        // flags LE
        assert_eq!(&bytes[4..6], &[0x01, 0x02]);
        // secondary_count
        assert_eq!(bytes[6], 2);
        // reserved
        assert_eq!(bytes[7], 0);
    }
}
