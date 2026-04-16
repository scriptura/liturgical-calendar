//! Étape 5 — Day Materialization : 366 slots + vespers lookahead.

#![allow(missing_docs)]

use std::collections::BTreeMap;

use liturgical_calendar_core::{CalendarEntry, Color, LiturgicalPeriod, Nature};

use crate::{
    canonicalization::{is_leap_year, SeasonBoundaries},
    error::ForgeError,
    resolution::ResolvedCalendar,
};

// ─── PoolBuilder ─────────────────────────────────────────────────────────────

/// Constructeur du Secondary Pool avec déduplication par séquence triée de FeastIDs.
///
/// La déduplication est une contrainte d'implémentation, pas une optimisation :
/// sans elle, le pool worst-case (~78 000 entrées sur 431 ans) dépasse u16::MAX.
pub(crate) struct PoolBuilder {
    /// Clé : séquence triée de FeastIDs — garantit {A,B} ≡ {B,A}.
    index: BTreeMap<Vec<u16>, u16>,
    /// Données sérialisées — concaténation de toutes les séquences.
    pub data: Vec<u16>,
}

impl PoolBuilder {
    pub fn new() -> Self {
        Self {
            index: BTreeMap::new(),
            data:  Vec::new(),
        }
    }

    /// Insère une séquence de FeastIDs et retourne son index dans le pool.
    /// Déduplique silencieusement si la séquence (triée) existe déjà.
    pub fn insert(&mut self, mut ids: Vec<u16>) -> Result<u16, ForgeError> {
        ids.sort_unstable(); // tri canonique avant déduplication — INV-FORGE-4

        if let Some(&existing) = self.index.get(&ids) {
            return Ok(existing); // zéro duplication
        }

        // V11 : secondary_index est u16 — capacité maximale 65 535 entrées.
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
}

// ─── encode_flags ─────────────────────────────────────────────────────────────

/// Encode les 4 champs de domaine dans les bits [0:13] de `flags`.
///
/// Les bits [14:15] (HAS_VESPERAE_I, HAS_VIGILIA) sont laissés à 0 :
/// ils sont positionnés exclusivement par `vespers_lookahead_pass`.
/// Exception : le bit 15 (HAS_VIGILIA) est aussi levé sur l'entrée propre
/// de la fête qui déclare `has_vigil_mass: true` (premier signal, avant lookahead).
pub(crate) fn encode_flags(
    precedence:        u8,
    color:             Color,
    liturgical_period: LiturgicalPeriod,
    nature:            Nature,
    feast_has_vigil:   bool, // has_vigil_mass déclaré dans le YAML
) -> u16 {
    (precedence as u16)
        | ((color as u16) << 4)
        | ((liturgical_period as u16) << 8)
        | ((nature as u16) << 11)
        // bit 14 (HAS_VESPERAE_I) : jamais set ici — vespers_lookahead_pass uniquement.
        | ((feast_has_vigil as u16) << 15)
}

// ─── generate_year ────────────────────────────────────────────────────────────

/// Génère les 366 slots `CalendarEntry` pour une année résolue.
///
/// - Slot doy=59 : Padding Entry (`zeroed()`) pour années non-bissextiles.
/// - Slots vides (fêtes absorbées) : `zeroed()`.
/// - Les bits [14:15] de `flags` sont laissés à 0 — `vespers_lookahead_pass` les calcule ensuite.
///
/// `pool` est partagé entre toutes les années pour la déduplication inter-années.
pub fn generate_year(
    resolved:          ResolvedCalendar,
    pool:              &mut PoolBuilder,
    season_boundaries: &SeasonBoundaries,
) -> Result<[CalendarEntry; 366], ForgeError> {
    let year    = resolved.year;
    let is_leap = is_leap_year(year);

    let mut entries = [CalendarEntry::zeroed(); 366];

    for doy in 0u16..=365u16 {
        // doy=59 non-bissextile : Padding Entry — entries[59] reste zeroed().
        if !is_leap && doy == 59 {
            continue;
        }

        let day = match resolved.days.get(&doy) {
            Some(d) => d,
            None    => continue, // Slot vide — zeroed() correct.
        };

        // LiturgicalPeriod : cache AOT depuis SeasonBoundaries.
        let period = season_boundaries.period_of(doy);

        // V12 : secondary_count est u8 — max 255 commémorations par slot.
        let secondary_count = day.secondary_feasts.len();
        if secondary_count > u8::MAX as usize {
            return Err(ForgeError::SecondaryCountOverflow { doy, year, count: secondary_count });
        }

        let (secondary_index, sc) = if secondary_count > 0 {
            // INV-FORGE-4 : ids déjà triés par feast_id dans ResolvedDay.secondary_feasts.
            let ids: Vec<u16> = day.secondary_feasts.iter().map(|f| f.feast_id).collect();
            let idx = pool.insert(ids)?;
            (idx, secondary_count as u8)
        } else {
            // secondary_index = 0 quand secondary_count = 0 — valeur canonique.
            (0u16, 0u8)
        };

        let flags = encode_flags(
            day.primary.precedence,
            day.primary.color,
            period,
            day.primary.nature,
            day.primary.has_vigil_mass,
        );

        entries[doy as usize] = CalendarEntry {
            primary_id:      day.primary.feast_id,
            secondary_index,
            flags,
            secondary_count: sc,
            _reserved:       0,
        };
    }

    // V10 : Padding Entry obligatoire à doy=59 pour années non-bissextiles.
    if !is_leap {
        let e = &entries[59];
        if e.primary_id != 0 || e.flags != 0 || e.secondary_count != 0 {
            return Err(ForgeError::PaddingEntryMissing { year, doy: 59 });
        }
    }

    Ok(entries)
}

// ─── vespers_lookahead_pass ────────────────────────────────────────────────────

/// Passe vespérale — calcule les bits [14:15] de chaque entrée.
///
/// Opère sur le tableau APRÈS `generate_year`. Deux passes possibles sur le même entrée :
/// - bit 15 sur la fête propre : posé par `encode_flags` (has_vigil_mass).
/// - bit 14 + report du bit 15 : posés ici sur le DOY précédant la fête.
///
/// `next_year_jan1` : premier slot de l'année suivante, pour DOY=365 → DOY+1.
/// Absent pour la dernière année du corpus (2399) — bits [14:15] laissés à 0.
pub fn vespers_lookahead_pass(
    entries:         &mut [CalendarEntry; 366],
    next_year_jan1:  Option<&CalendarEntry>,
) {
    for doy in 0u16..=365u16 {
        let tomorrow: &CalendarEntry = if doy < 365 {
            &entries[doy as usize + 1]
        } else {
            match next_year_jan1 {
                Some(e) => e,
                None    => continue, // 31 déc 2399 — aucun DOY+1, bits conservés à 0.
            }
        };

        // Premières Vêpres : Solennités (Precedence ≤ 4) et Dimanches (Precedence = 7).
        let tomorrow_prec = (tomorrow.flags & 0x0F) as u8;
        let has_first_vespers = tomorrow_prec <= 4 || tomorrow_prec == 7;
        if !has_first_vespers {
            continue;
        }

        // Les Secondes Vêpres de today priment si today est de rang ≥ tomorrow.
        // Valeur numérique inférieure = rang liturgique supérieur.
        let today_prec = (entries[doy as usize].flags & 0x0F) as u8;
        if today_prec <= tomorrow_prec {
            continue; // Secondes Vêpres de today l'emportent.
        }

        // HAS_VESPERAE_I — bit 14.
        entries[doy as usize].flags |= 1 << 14;

        // HAS_VIGILIA — bit 15 reporté si la fête de demain déclare has_vigil_mass.
        // Le bit 15 de `tomorrow` a été posé par encode_flags lors de generate_year.
        if tomorrow.flags & (1 << 15) != 0 {
            entries[doy as usize].flags |= 1 << 15;
        }
    }
}
