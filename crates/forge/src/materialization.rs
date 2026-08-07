//! Étape 5 — Day Materialization : Timeline v6 + vespers lookahead.
//!
//! Changements v5 → v6 :
//!   - `encode_feast_flags` : paramètre `liturgical_period` supprimé.
//!     `FeastEntry.flags[10:8]` libérés (nuls).
//!   - `build_feast_registry` : n'utilise plus `season_boundaries`.
//!   - `generate_year` : `_season_boundaries` → `season_boundaries` (activé).
//!     Écrit `occurrence_flags[4:2]` (period) et `liturgical_week` pour TOUS les slots non-padding,
//!     y compris les jours sans fête propre.
//!     Suppression de la dépendance à l'inter-passe feria fictive.
//!   - `vespers_lookahead_pass`: `prec_of` retourne `0xFF` pour les slots padding
//!     (fix bug : `0` interprété à tort comme TriduumSacrum).

#![allow(missing_docs)]

use std::collections::BTreeMap;

use liturgical_calendar_core::{
    entry::{FeastEntry, TimelineEntry},
    types::{Color, LiturgicalPeriod, Nature},
};

use crate::{
    canonicalization::{SeasonBoundaries, is_leap_year},
    error::ForgeError,
    resolution::ResolvedCalendar,
};

// ─── FeastRegistryBuilder ─────────────────────────────────────────────────────

/// Constructeur du Feast Registry AOT.
///
/// Maintient `feast_id → registry_index` (1-based).
/// Index `0` = sentinel Padding, jamais assigné.
/// Déterministe : le premier `get_or_insert` pour un `feast_id` donné fixe son index.
pub(crate) struct FeastRegistryBuilder {
    index: BTreeMap<(u16, u16), u16>,
    pub entries: Vec<FeastEntry>,
}

impl FeastRegistryBuilder {
    pub fn new() -> Self {
        Self {
            index: BTreeMap::new(),
            entries: Vec::new(),
        }
    }

    /// Retourne le `registry_index` existant ou insère et assigne le suivant (1-based).
    pub fn get_or_insert(&mut self, feast_id: u16, flags: u16) -> Result<u16, ForgeError> {
        let key = (feast_id, flags);
        if let Some(&idx) = self.index.get(&key) {
            return Ok(idx);
        }
        if self.entries.len() >= u16::MAX as usize {
            return Err(ForgeError::RegistryOverflow);
        }
        let idx = self.entries.len() as u16 + 1;
        self.index.insert(key, idx);
        self.entries.push(FeastEntry { feast_id, flags });
        Ok(idx)
    }

    pub fn index_of(&self, feast_id: u16, flags: u16) -> Option<u16> {
        self.index.get(&(feast_id, flags)).copied()
    }

    pub fn registry_count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// Slice pour `vespers_lookahead_pass` et la sérialisation.
    /// `entries[registry_index − 1]` = FeastEntry pour `registry_index` donné.
    pub fn as_slice(&self) -> &[FeastEntry] {
        &self.entries
    }
}

// ─── PoolBuilder ─────────────────────────────────────────────────────────────

/// Secondary Pool — stocke des `registry_index` (1-based) dédupliqués.
pub(crate) struct PoolBuilder {
    index: BTreeMap<Vec<u16>, u16>,
    pub data: Vec<u16>,
}

impl PoolBuilder {
    pub fn new() -> Self {
        Self {
            index: BTreeMap::new(),
            data: Vec::new(),
        }
    }

    /// Insère une séquence de `registry_index` et retourne son offset (en u16).
    pub fn insert(&mut self, mut indices: Vec<u16>) -> Result<u16, ForgeError> {
        indices.sort_unstable();

        if let Some(&existing) = self.index.get(&indices) {
            return Ok(existing);
        }
        if self.data.len() + indices.len() > u16::MAX as usize {
            return Err(ForgeError::SecondaryPoolOverflow {
                pool_len: self.data.len() as u32,
                max_capacity: u16::MAX as u32,
            });
        }
        let offset = self.data.len() as u16;
        self.index.insert(indices.clone(), offset);
        self.data.extend_from_slice(&indices);
        Ok(offset)
    }
}

// ─── encode_feast_flags ───────────────────────────────────────────────────────

/// Encode les invariants d'une fête dans `FeastEntry.flags`.
///
/// Layout v6 :
/// - bits [3:0]   → `precedence`
/// - bits [7:4]   → `color`
/// - bits [10:8]  → réservés, nuls (v5 : `liturgical_period` — supprimé en v6)
/// - bits [13:11] → `nature`
/// - bit  [14]    → `has_vigil_mass`
/// - bit  [15]    → réservé, nul
pub(crate) fn encode_feast_flags(
    precedence: u8,
    color: Color,
    nature: Nature,
    has_vigil_mass: bool,
) -> u16 {
    (precedence as u16)
        | ((color as u16)          << 4)
        // bits [10:8] réservés nuls — LiturgicalPeriod retiré de FeastEntry en v6
        | ((nature as u16)         << 11)
        | ((has_vigil_mass as u16) << 14)
}

// ─── liturgical_week_of ───────────────────────────────────────────────────────

/// Calcule l'ordinal de semaine liturgique pour un DOY dans une période.
///
/// | Période             | Retour       | Base de calcul                         |
/// |---------------------|-------------|----------------------------------------|
/// | TempusOrdinarium SI | 1–8 (env.)  | `⌊(doy − epiphania) / 7⌋ + 1`         |
/// | TempusOrdinarium SII| N–34        | `34 − ⌊(adventus − 1 − doy) / 7⌋`    |
/// | TempusAdventus      | 1–4         | `⌊(doy − adventus) / 7⌋ + 1`          |
/// | TempusNativitatis   | 1           | fixe                                   |
/// | TempusQuadragesimae | 1–5         | `⌊(doy − ash_wednesday) / 7⌋ + 1`     |
/// | DiesSancti          | 6           | fixe (Hebdomada VI Quadragesimae)      |
/// | TriduumPaschale     | 0           | pas d'ordinal applicable               |
/// | TempusPaschale      | 1–7         | `⌊(doy − easter) / 7⌋ + 1`            |
fn liturgical_week_of(doy: u16, period: LiturgicalPeriod, sb: &SeasonBoundaries) -> u8 {
    match period {
        LiturgicalPeriod::TempusOrdinarium => {
            if doy > sb.pentecost {
                // Segment II — ordinal canonique à rebours depuis l'Avent.
                // Dominica XXXIV = adventus − 7 → ordinal = 34 − ⌊(adventus−1−doy)/7⌋
                34u8.saturating_sub((sb.adventus.saturating_sub(1).saturating_sub(doy) / 7) as u8)
            } else {
                // Segment I — ordinal depuis le Baptême du Seigneur (epiphania).
                // Dominica I = epiphania → semaine 1; Dominica II = epiphania+7 → semaine 2.
                (doy.saturating_sub(sb.epiphania) / 7 + 1) as u8
            }
        }
        LiturgicalPeriod::TempusAdventus => (doy.saturating_sub(sb.adventus) / 7 + 1) as u8,
        LiturgicalPeriod::TempusNativitatis => 1,
        LiturgicalPeriod::TempusQuadragesimae => {
            (doy.saturating_sub(sb.ash_wednesday) / 7 + 1) as u8
        }
        LiturgicalPeriod::DiesSancti => 6, // Semaine Sainte = Hebdomada VI Quadragesimae
        LiturgicalPeriod::TriduumPaschale => 0,
        LiturgicalPeriod::TempusPaschale => (doy.saturating_sub(sb.easter) / 7 + 1) as u8,
    }
}

// ─── build_feast_registry (Pass 1) ────────────────────────────────────────────

/// Pass 1 — collecte tous les `feast_id` du corpus et construit le Feast Registry.
///
/// v6 : `season_boundaries` non utilisé (LiturgicalPeriod retiré de `encode_feast_flags`).
pub(crate) fn build_feast_registry<'a>(
    all_inputs: &[(&'a ResolvedCalendar, &'a SeasonBoundaries)],
) -> Result<FeastRegistryBuilder, ForgeError> {
    let mut builder = FeastRegistryBuilder::new();

    for &(resolved, _season_boundaries) in all_inputs {
        let is_leap = is_leap_year(resolved.year);

        for (&doy, day) in &resolved.days {
            if !is_leap && doy == 59 {
                continue;
            }

            let primary_flags = encode_feast_flags(
                day.primary.precedence,
                day.primary.color,
                day.primary.nature,
                day.primary.has_vigil_mass,
            );
            builder.get_or_insert(day.primary.feast_id, primary_flags)?;

            for secondary in &day.secondary_feasts {
                let sec_flags = encode_feast_flags(
                    secondary.precedence,
                    secondary.color,
                    secondary.nature,
                    secondary.has_vigil_mass,
                );
                builder.get_or_insert(secondary.feast_id, sec_flags)?;
            }
        }
    }

    Ok(builder)
}

// ─── generate_year (Pass 2) ───────────────────────────────────────────────────

/// Pass 2 — génère les 366 `TimelineEntry` pour une année résolue.
///
/// v6 : pour tout slot non-padding, écrit `occurrence_flags[4:2]` (LiturgicalPeriod)
/// et `liturgical_week`, y compris pour les jours sans fête propre (`primary_index = 0`).
/// Le DOY 59 des années non-bissextiles reste entièrement nul (sentinel Padding pur).
///
/// `occurrence_flags[1:0]` (vesperae_i, vigilia) sont laissés à 0 ici ;
/// `vespers_lookahead_pass` les positionne en passe ultérieure (OR-safe).
pub(crate) fn generate_year(
    resolved: &ResolvedCalendar,
    pool: &mut PoolBuilder,
    season_boundaries: &SeasonBoundaries,
    feast_registry: &FeastRegistryBuilder,
) -> Result<[TimelineEntry; 366], ForgeError> {
    let year = resolved.year;
    let is_leap = is_leap_year(year);

    let mut entries = [TimelineEntry::zeroed(); 366];

    for doy in 0u16..=365u16 {
        // DOY 59 non-bissextile : sentinel Padding pur — aucune donnée écrite.
        if !is_leap && doy == 59 {
            continue;
        }

        let period = season_boundaries.period_of(doy);
        let week = liturgical_week_of(doy, period, season_boundaries);
        let period_bits = (period as u8 & 0x07) << 2;

        let day = match resolved.days.get(&doy) {
            Some(d) => d,
            None => {
                // Slot sans fête propre — primitives temporelles uniquement.
                // primary_index = 0 ; period et week renseignés pour le Runtime.
                entries[doy as usize] = TimelineEntry {
                    primary_index: 0,
                    secondary_offset: 0,
                    occurrence_flags: period_bits,
                    secondary_count: 0,
                    liturgical_week: week,
                    _reserved: 0,
                };
                continue;
            }
        };

        let secondary_count = day.secondary_feasts.len();
        if secondary_count > u8::MAX as usize {
            return Err(ForgeError::SecondaryCountOverflow {
                doy,
                year,
                count: secondary_count,
            });
        }

        let primary_flags = encode_feast_flags(
            day.primary.precedence,
            day.primary.color,
            day.primary.nature,
            day.primary.has_vigil_mass,
        );
        let primary_index = feast_registry
            .index_of(day.primary.feast_id, primary_flags)
            .ok_or(ForgeError::FeastNotInRegistry {
                feast_id: day.primary.feast_id,
                year,
                doy,
            })?;

        let (secondary_offset, sc) = if secondary_count > 0 {
            let indices: Vec<u16> = day
                .secondary_feasts
                .iter()
                .map(|f| {
                    let sec_flags =
                        encode_feast_flags(f.precedence, f.color, f.nature, f.has_vigil_mass);
                    feast_registry.index_of(f.feast_id, sec_flags).ok_or(
                        ForgeError::FeastNotInRegistry {
                            feast_id: f.feast_id,
                            year,
                            doy,
                        },
                    )
                })
                .collect::<Result<_, _>>()?;
            let offset = pool.insert(indices)?;
            (offset, secondary_count as u8)
        } else {
            (0u16, 0u8)
        };

        entries[doy as usize] = TimelineEntry {
            primary_index,
            secondary_offset,
            occurrence_flags: period_bits, // bits [1:0] à 0 — écrits par vespers_lookahead_pass
            secondary_count: sc,
            liturgical_week: week,
            _reserved: 0,
        };
    }

    // Invariant Padding : DOY 59 doit être entièrement nul pour les années non-bissextiles.
    if !is_leap {
        let e = &entries[59];
        if e.primary_index != 0
            || e.secondary_count != 0
            || e.occurrence_flags != 0
            || e.liturgical_week != 0
        {
            return Err(ForgeError::PaddingEntryMissing { year, doy: 59 });
        }
    }

    Ok(entries)
}

// ─── vespers_lookahead_pass ────────────────────────────────────────────────────

/// Passe vespérale — positionne `occurrence_flags[1:0]` de chaque `TimelineEntry`.
///
/// Opère APRÈS `generate_year`. Les bits [7:2] (`LiturgicalPeriod` + réservés)
/// sont préservés par l'opérateur `|=`.
///
/// Fix v6 : `prec_of` retourne `0xFF` (hors domaine [0,12]) pour les slots
/// `primary_index == 0`. Cela corrige le bug v5 où `0` était interprété comme
/// TriduumSacrum, corrompant les `occurrence_flags` du 28 février en année commune.
///
/// Règles (inchangées) :
///   - bit 0 (HAS_VESPERAE_I) : si `tomorrow_prec ≤ 3 || tomorrow_prec == 5`
///     ET `today_prec ≥ tomorrow_prec || today_prec == 0` (Triduum).
///   - bit 1 (HAS_VIGILIA)    : reporté depuis `FeastEntry.has_vigil_mass()` de demain.
pub(crate) fn vespers_lookahead_pass(
    entries: &mut [TimelineEntry; 366],
    feast_registry: &[FeastEntry],
    next_year_jan1: Option<&TimelineEntry>,
) {
    /// Extrait la préséance depuis le Feast Registry.
    /// Retourne `0xFF` pour les slots padding (`primary_index == 0`) :
    /// valeur hors domaine [0,12] garantissant qu'un slot vide ne bloque
    /// jamais les Premières Vêpres du lendemain et n'est pas confondu
    /// avec TriduumSacrum (prec 0).
    #[inline]
    fn prec_of(e: &TimelineEntry, r: &[FeastEntry]) -> u8 {
        if e.primary_index == 0 {
            return 0xFF;
        }
        r.get(e.primary_index as usize - 1)
            .map_or(0xFF, |fe| (fe.flags & 0x0F) as u8)
    }

    #[inline]
    fn vigil_of(e: &TimelineEntry, r: &[FeastEntry]) -> bool {
        if e.primary_index == 0 {
            return false;
        }
        r.get(e.primary_index as usize - 1)
            .is_some_and(|fe| fe.flags & (1 << 14) != 0)
    }

    for doy in 0u16..=365u16 {
        let (tomorrow_prec, tomorrow_has_vigil) = if doy < 365 {
            let t = &entries[doy as usize + 1];
            (prec_of(t, feast_registry), vigil_of(t, feast_registry))
        } else {
            match next_year_jan1 {
                Some(e) => (prec_of(e, feast_registry), vigil_of(e, feast_registry)),
                None => continue,
            }
        };

        // Demain doit être une fête à préséance élevée pour mériter des Premières Vêpres.
        // 0xFF (padding) → has_first_vespers = false, correctement ignoré.
        let has_first_vespers = tomorrow_prec <= 3 || tomorrow_prec == 5;
        if !has_first_vespers {
            continue;
        }

        let today_prec = prec_of(&entries[doy as usize], feast_registry);
        // Condition de blocage : aujourd'hui a une fête plus importante que demain
        // (today_prec < tomorrow_prec, i.e. numéro plus faible = rang plus élevé),
        // SAUF TriduumSacrum (today_prec == 0) qui cède toujours ses Vêpres.
        // 0xFF (padding) ne bloque jamais : 0xFF < tomorrow_prec est false (0xFF > 12).
        if today_prec != 0 && today_prec < tomorrow_prec {
            continue;
        }

        // Préserve les bits [7:2], positionne les bits [1:0].
        let mut occ = entries[doy as usize].occurrence_flags;
        occ |= 1 << 0;
        if tomorrow_has_vigil {
            occ |= 1 << 1;
        }
        entries[doy as usize].occurrence_flags = occ;
    }
}
