//! Étape 4 — Conflict Resolution : pipeline 5 passes.
//!
//! Hypothèses sur les types Session A (ajuster si divergence) :
//!   FeastRegistry::feasts       : BTreeMap<String, FeastDef>
//!   FeastDef::feast_id          : u16
//!   FeastDef::active_version_for(year) -> Option<&FeastVersionDef>
//!   FeastVersionDef::precedence : Precedence
//!   FeastVersionDef::nature     : Nature
//!   FeastVersionDef::color      : Color
//!   FeastVersionDef::has_vigil_mass : bool
//!   FeastVersionDef::date       : Option<(u8, u8)>   — (month, day)
//!   FeastVersionDef::mobile     : Option<MobileDef>
//!   MobileDef::anchor           : String
//!   MobileDef::offset           : i32
//!   MobileDef::ordinal          : Option<u8>         — tempus_ordinarium uniquement
//!   TransferRule::collides      : Vec<String>
//!   TransferRule::target        : TransferTarget
//!   TransferTarget              : Offset(u32) | Date{m,d} | Mobile{anchor,offset}
//!   CanonicalizedYear::pre_resolved_transfers : BTreeMap<(String,String), u16>
//!   SeasonBoundaries::period_of(&self, doy: u16) -> LiturgicalPeriod
//!
//! v6 : suppression de l'inter-passe 4/5.
//! Les jours sans fête propre (y compris DOY 59 des années bissextiles) sont
//! désormais matérialisés dans `generate_year` comme slots padding portant
//! uniquement `LiturgicalPeriod` et `liturgical_week` — sans ferie fictive
//! dans le Feast Registry.

#![allow(missing_docs)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

// --- Imports Core (Types binaires optimisés) ---
use liturgical_calendar_core::{
    Color as CoreColor, LiturgicalPeriod as CorePeriod, Nature as CoreNature,
};

// --- Import Registry (Contrat YAML / Ingestion) ---
// On aliase pour ne pas percuter CorePeriod
use crate::registry::LiturgicalPeriod as RegistryPeriod;

use crate::{
    canonicalization::{
        CanonicalizedYear, MONTH_STARTS, is_leap_year, resolve_tempus_ordinarium_dispatch,
        resolve_tempus_ordinarium_post_epiphaniam,
    },
    error::ForgeError,
    registry::{
        FeastDef, FeastRegistry, Scope, Temporality as RegistryTemporality, TransferTarget,
    },
};

// ─── Nouvel outil de conversion ───────────────────────────────────────────────

/// Transforme la période du Registre vers le type binaire du Core.
pub(crate) fn period_to_core(p: &RegistryPeriod) -> CorePeriod {
    match p {
        RegistryPeriod::TempusOrdinarium => CorePeriod::TempusOrdinarium,
        RegistryPeriod::TempusAdventus => CorePeriod::TempusAdventus,
        RegistryPeriod::TempusNativitatis => CorePeriod::TempusNativitatis,
        RegistryPeriod::TempusQuadragesimae => CorePeriod::TempusQuadragesimae,
        RegistryPeriod::TriduumPaschale => CorePeriod::TriduumPaschale,
        RegistryPeriod::TempusPaschale => CorePeriod::TempusPaschale,
        RegistryPeriod::DiesSancti => CorePeriod::DiesSancti,
    }
}

// ─── FeastIdMap ───────────────────────────────────────────────────────────────

/// `slug → FeastID` alloué. INV-FORGE-2 : BTreeMap.
pub(crate) type FeastIdMap = BTreeMap<String, u16>;

// ─── Conversions registry → Core ─────────────────────────────────────────────

fn color_to_core(c: &crate::registry::Color) -> CoreColor {
    use crate::registry::Color as R;
    match c {
        R::Albus => CoreColor::Albus,
        R::Rubeus => CoreColor::Rubeus,
        R::Viridis => CoreColor::Viridis,
        R::Violaceus => CoreColor::Violaceus,
        R::Rosaceus => CoreColor::Rosaceus,
        R::Niger => CoreColor::Niger,
        R::Aureus => CoreColor::Albus, // réservé Core v2.0 — fallback Albus
    }
}

fn nature_to_core(n: &crate::registry::Nature) -> CoreNature {
    use crate::registry::Nature as R;
    match n {
        R::Sollemnitas => CoreNature::Sollemnitas,
        R::Festum => CoreNature::Festum,
        R::Dominica => CoreNature::Dominica,
        R::Memoria => CoreNature::Memoria,
        R::Commemoratio => CoreNature::Commemoratio,
        R::Feria => CoreNature::Feria,
    }
}

// ─── Cycle ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cycle {
    Temporal = 0,
    Sanctoral = 1,
}

// ─── ResolutionKey ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResolutionKey {
    pub sort_weight: u16,
    pub feast_id: u16,
}

// ─── PlacedFeast ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlacedFeast {
    pub slug: String,
    pub feast_id: u16,
    pub scope_bits: u8,
    pub precedence: u8,
    pub class: u8,
    pub nature: CoreNature,
    pub color: CoreColor,
    pub period: Option<CorePeriod>,
    pub has_vigil_mass: bool,
    pub cycle: Cycle,
}

impl PlacedFeast {
    #[inline]
    fn key(&self) -> ResolutionKey {
        ResolutionKey {
            sort_weight: (self.precedence as u16) * 256 + (self.class as u16),
            feast_id: self.feast_id,
        }
    }
}

impl PartialOrd for PlacedFeast {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PlacedFeast {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

// ─── ResolvedDay / ResolvedCalendar ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ResolvedDay {
    pub primary: PlacedFeast,
    pub secondary_feasts: Vec<PlacedFeast>,
}

pub(crate) struct ResolvedCalendar {
    pub year: u16,
    pub days: BTreeMap<u16, ResolvedDay>,
}

// ─── TransferQueue ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Eq, PartialEq)]
struct TransferEntry {
    doy_current: u16,
    feast_id: u16,
    depth: u8,
    feast: PlacedFeast,
}

impl Ord for TransferEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.doy_current, self.feast_id).cmp(&(other.doy_current, other.feast_id))
    }
}
impl PartialOrd for TransferEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const MAX_TRANSFER_DEPTH: u8 = 7;

struct TransferQueue {
    pending: BTreeSet<TransferEntry>,
}

impl TransferQueue {
    fn new() -> Self {
        Self {
            pending: BTreeSet::new(),
        }
    }

    fn enqueue(
        &mut self,
        doy_src: u16,
        feast: PlacedFeast,
        depth: u8,
        year: u16,
    ) -> Result<(), ForgeError> {
        if depth > MAX_TRANSFER_DEPTH {
            return Err(ForgeError::TransferFailed {
                slug: feast.slug.clone(),
                origin_doy: doy_src.saturating_sub(depth as u16),
                blocked_at: doy_src,
                year,
            });
        }
        self.pending.insert(TransferEntry {
            doy_current: doy_src,
            feast_id: feast.feast_id,
            depth,
            feast,
        });
        Ok(())
    }

    fn pop_first(&mut self) -> Option<TransferEntry> {
        let e = self.pending.iter().next()?.clone();
        self.pending.remove(&e);
        Some(e)
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

// ─── Déclassement saisonnier — §3.4 ─────────────────────────────────────────

pub(crate) fn should_demote_to_commemoratio(
    feast: &PlacedFeast,
    period: CorePeriod,
    doy: u16,
) -> bool {
    if feast.nature != CoreNature::Memoria {
        return false;
    }
    match period {
        CorePeriod::TempusQuadragesimae | CorePeriod::TriduumPaschale | CorePeriod::DiesSancti => {
            true
        }
        CorePeriod::TempusAdventus => {
            // NUALC n°16 : déclassement limité au 17–24 décembre.
            // `doy` est en pseudo-DOY (slot 59 réservé au 29 fév., boucle 0..=365) :
            // même référentiel que MONTH_STARTS — aucune translation nécessaire.
            let dec_17 = MONTH_STARTS[11] + 17 - 1;
            let dec_24 = MONTH_STARTS[11] + 24 - 1;
            doy >= dec_17 && doy <= dec_24
        }
        _ => false,
    }
}

// ─── DOY depuis FeastDef.temporality ─────────────────────────────────────────

fn feast_doy(feast_def: &FeastDef, anchors: &BTreeMap<String, u16>, year: u16) -> Option<u16> {
    match feast_def.temporality.as_ref()? {
        RegistryTemporality::Fixed { month, day } => {
            Some(MONTH_STARTS[*month as usize - 1] + *day as u16 - 1)
        }
        RegistryTemporality::Mobile { anchor, offset } => {
            let anchor_doy = *anchors.get(anchor.as_str())? as i32;
            let mut doy = anchor_doy + offset;
            if !is_leap_year(year) {
                if anchor_doy >= 59 && doy < 59 {
                    doy -= 1;
                } else if anchor_doy < 59 && doy >= 59 {
                    doy += 1;
                }
            }
            (0..=365).contains(&doy).then_some(doy as u16)
        }
        RegistryTemporality::Ordinal { ordinal } => {
            let adventus = *anchors.get("adventus")?;
            let post_epiphaniam = resolve_tempus_ordinarium_post_epiphaniam(year);
            Some(resolve_tempus_ordinarium_dispatch(
                year,
                post_epiphaniam,
                adventus,
                *ordinal,
            ))
        }
    }
}

fn feast_cycle(feast_def: &FeastDef) -> Cycle {
    match feast_def
        .temporality
        .as_ref()
        .expect("temporality absente après merge")
    {
        RegistryTemporality::Fixed { .. } => Cycle::Sanctoral,
        RegistryTemporality::Mobile { .. } | RegistryTemporality::Ordinal { .. } => Cycle::Temporal,
    }
}

// ─── Élection canonique ───────────────────────────────────────────────────────

fn elect(
    mut candidates: Vec<PlacedFeast>,
    period: CorePeriod,
    doy: u16,
) -> (PlacedFeast, Vec<PlacedFeast>, Vec<PlacedFeast>) {
    // §3.4 — Mutation in-place avant tri canonique.
    for feast in &mut candidates {
        if should_demote_to_commemoratio(feast, period, doy) {
            feast.precedence = 11;
            feast.nature = CoreNature::Commemoratio;
        }
    }

    // Tri descendant : le candidat de clé minimale (priorité canonique maximale)
    // se retrouve en queue — extraction via .pop() en O(1) strict, zéro memmove.
    candidates.sort_unstable_by_key(|b| std::cmp::Reverse(b.key()));
    let primary = candidates.pop().expect("elect: vecteur de candidats vide");

    // Avec tri descendant, secondaires (prec. ≥ 6, clé haute) précèdent
    // les candidats au transfert (prec. 0–5, clé basse).
    // partition_point est valide : le prédicat prec >= 6 est vrai sur un
    // préfixe contigu après tri descendant par clé (prec * 256 + class).
    let split = candidates.partition_point(|f| f.precedence >= 6);

    // Drain depuis la queue (split..) : zéro déplacement des secondaires.
    // Vide si split == len (cas majoritaire) : aucune allocation.
    let to_transfer: Vec<PlacedFeast> = candidates
        .drain(split..)
        .filter(|f| f.nature != CoreNature::Dominica && f.nature != CoreNature::Feria)
        .collect();

    // Inversion O(n) in-place : restitue l'ordre canonique ascendant
    // (clé minimale en tête) attendu par le sérialiseur .kald.
    candidates.reverse();

    (primary, candidates, to_transfer)
}

// ─── resolve_year ─────────────────────────────────────────────────────────────

pub(crate) fn resolve_year(
    canonicalized: CanonicalizedYear,
    registry: &FeastRegistry,
    feast_ids: &FeastIdMap,
) -> Result<ResolvedCalendar, ForgeError> {
    let year = canonicalized.year;
    let is_leap = is_leap_year(year);

    // ── PASSE 1 ───────────────────────────────────────────────────────────────

    let mut slots: BTreeMap<u16, Vec<PlacedFeast>> = BTreeMap::new();

    for feast_def in registry.iter() {
        let version = match feast_def.active_version_for(year) {
            Some(v) => v,
            None => continue,
        };

        let doy = match feast_doy(feast_def, &canonicalized.anchors, canonicalized.year) {
            Some(d) => d,
            None => continue,
        };

        if !is_leap && doy == 59 {
            continue;
        }

        let feast_id = match feast_ids.get(&feast_def.slug) {
            Some(&id) => id,
            None => continue,
        };

        let scope_bits: u8 = match &feast_def.scope {
            Scope::Universal => 0,
            Scope::National(_) => 1,
            Scope::Diocesan(_) => 2,
        };

        let cycle = feast_cycle(feast_def);

        let precedence = version.precedence.ok_or_else(|| {
            eprintln!(
                "ERREUR: Champ 'precedence' manquant pour le slug: {}",
                feast_def.slug
            );
            ForgeError::MissingResolvedField {
                feast_id,
                year,
                doy,
                field: "precedence",
            }
        })?;

        let nature_val = version.nature.as_ref().ok_or_else(|| {
            eprintln!(
                "ERREUR: Champ 'nature' manquant pour le slug: {}",
                feast_def.slug
            );
            ForgeError::MissingResolvedField {
                feast_id,
                year,
                doy,
                field: "nature",
            }
        })?;

        let color_val = version.color.as_ref().ok_or_else(|| {
            eprintln!(
                "ERREUR: Champ 'color' manquant pour le slug: {}",
                feast_def.slug
            );
            ForgeError::MissingResolvedField {
                feast_id,
                year,
                doy,
                field: "color",
            }
        })?;

        let class: u8 = feast_def.class.ok_or_else(|| {
            eprintln!(
                "ERREUR: Champ 'class' manquant pour le slug: {}",
                feast_def.slug
            );
            ForgeError::MissingResolvedField {
                feast_id,
                year,
                doy,
                field: "class",
            }
        })? as u8;

        slots.entry(doy).or_default().push(PlacedFeast {
            slug: feast_def.slug.clone(),
            feast_id,
            scope_bits,
            precedence,
            class,
            nature: nature_to_core(nature_val),
            color: color_to_core(color_val),
            period: version.period.as_ref().map(period_to_core),
            has_vigil_mass: version.has_vigil_mass,
            cycle,
        });
    }

    // ── PASSE 2 ───────────────────────────────────────────────────────────────

    for (&doy, candidates) in slots.iter_mut() {
        // V7a : TriduumSacrum (0) et SollemnitatesMaiores (1) — uniques par construction.
        {
            let very_high: Vec<_> = candidates.iter().filter(|f| f.precedence <= 1).collect();
            if very_high.len() >= 2 {
                return Err(ForgeError::SolemnityCollision {
                    slug_a: very_high[0].slug.clone(),
                    slug_b: very_high[1].slug.clone(),
                    precedence: very_high[0].precedence,
                    doy,
                    year,
                });
            }
        }

        // V7b : SollemnitatesGenerales (2) et SollemnitatesPropria (3).
        {
            let solemn: Vec<_> = candidates
                .iter()
                .filter(|f| f.precedence >= 2 && f.precedence <= 3)
                .collect();
            for i in 0..solemn.len() {
                for j in (i + 1)..solemn.len() {
                    if solemn[i].scope_bits == solemn[j].scope_bits
                        && solemn[i].class == solemn[j].class
                    {
                        return Err(ForgeError::SolemnityCollision {
                            slug_a: solemn[i].slug.clone(),
                            slug_b: solemn[j].slug.clone(),
                            precedence: solemn[i].precedence,
                            doy,
                            year,
                        });
                    }
                }
            }
        }

        // §3.1 — scope le plus local prime pour les Solennités.
        if candidates.iter().filter(|f| f.precedence <= 3).count() >= 2 {
            let max_scope = candidates
                .iter()
                .filter(|f| f.precedence <= 3)
                .map(|f| f.scope_bits)
                .max()
                .unwrap_or(0);
            candidates.retain(|f| !(f.precedence <= 3 && f.scope_bits < max_scope));
        }
    }

    // ── PASSE 3 ───────────────────────────────────────────────────────────────

    let mut resolved_days: BTreeMap<u16, ResolvedDay> = BTreeMap::new();
    let mut transfer_queue = TransferQueue::new();
    let mut pending_inserts: BTreeMap<u16, Vec<PlacedFeast>> = BTreeMap::new();
    let mut retrograde_inserts: Vec<(u16, PlacedFeast)> = Vec::new();

    for doy in 0u16..=365u16 {
        let mut candidates: Vec<PlacedFeast> = slots.remove(&doy).unwrap_or_default();
        if let Some(fwd) = pending_inserts.remove(&doy) {
            candidates.extend(fwd);
        }
        if candidates.is_empty() {
            continue;
        }

        let period = canonicalized.season_boundaries.period_of(doy);
        let (primary, secondary_feasts, to_transfer) = elect(candidates, period, doy);

        for feast in to_transfer {
            let active_rule = registry
                .get(&feast.slug)
                .and_then(|def| def.active_version_for(year))
                .and_then(|ver| {
                    ver.transfers
                        .iter()
                        .find(|t| t.collides.iter().any(|c| c == &primary.slug))
                });

            if let Some(rule) = active_rule {
                let matched_collides = rule
                    .collides
                    .iter()
                    .find(|c| *c == &primary.slug)
                    .cloned()
                    .unwrap();
                let pre_key = (feast.slug.clone(), matched_collides);
                if let Some(&doy_dst) = canonicalized.pre_resolved_transfers.get(&pre_key) {
                    if doy_dst <= doy {
                        retrograde_inserts.push((doy_dst, feast));
                    } else {
                        pending_inserts.entry(doy_dst).or_default().push(feast);
                    }
                    continue;
                }

                let doy_dst: u16 = match &rule.target {
                    TransferTarget::Offset(n) => doy + *n as u16,
                    TransferTarget::Date { month, day } => {
                        MONTH_STARTS[*month as usize - 1] + *day as u16 - 1
                    }
                    TransferTarget::Mobile { .. } => {
                        transfer_queue.enqueue(doy, feast, 0, year)?;
                        continue;
                    }
                };

                if doy_dst <= doy {
                    retrograde_inserts.push((doy_dst, feast));
                } else {
                    pending_inserts.entry(doy_dst).or_default().push(feast);
                }
            } else {
                transfer_queue.enqueue(doy, feast, 0, year)?;
            }
        }

        resolved_days.insert(
            doy,
            ResolvedDay {
                primary,
                secondary_feasts,
            },
        );
    }

    retrograde_inserts.sort_unstable_by_key(|(d, _)| *d);
    for (doy_dst, feast) in retrograde_inserts {
        let period = canonicalized.season_boundaries.period_of(doy_dst);
        if let Some(day) = resolved_days.get_mut(&doy_dst) {
            let mut all = vec![day.primary.clone(), feast];
            all.extend(day.secondary_feasts.clone());
            let (new_primary, new_secondary, _) = elect(all, period, doy_dst);
            day.primary = new_primary;
            day.secondary_feasts = new_secondary;
        } else {
            resolved_days.insert(
                doy_dst,
                ResolvedDay {
                    primary: feast,
                    secondary_feasts: Vec::new(),
                },
            );
        }
    }

    // ── PASSE 4 ───────────────────────────────────────────────────────────────

    while let Some(entry) = transfer_queue.pop_first() {
        let TransferEntry {
            doy_current,
            feast,
            depth,
            ..
        } = entry;
        let mut placed = false;

        let window_end = (doy_current + 7).min(365);
        for doy_dst in (doy_current + 1)..=window_end {
            let slot_free = match resolved_days.get(&doy_dst) {
                Some(day) => day.primary.precedence > feast.precedence,
                None => true,
            };
            if !slot_free {
                continue;
            }

            let period = canonicalized.season_boundaries.period_of(doy_dst);
            let mut all = vec![feast.clone()];
            if let Some(existing) = resolved_days.remove(&doy_dst) {
                all.push(existing.primary);
                all.extend(existing.secondary_feasts);
            }
            let (new_primary, new_secondary, displaced) = elect(all, period, doy_dst);
            for d in displaced {
                transfer_queue.enqueue(doy_dst, d, depth + 1, year)?;
            }
            resolved_days.insert(
                doy_dst,
                ResolvedDay {
                    primary: new_primary,
                    secondary_feasts: new_secondary,
                },
            );
            placed = true;
            break;
        }

        if !placed {
            return Err(ForgeError::TransferFailed {
                slug: feast.slug.clone(),
                origin_doy: doy_current.saturating_sub(depth as u16),
                blocked_at: doy_current,
                year,
            });
        }
    }

    debug_assert!(
        transfer_queue.is_empty(),
        "TransferQueue non vide après Passe 4"
    );

    // ── PASSE 5 ───────────────────────────────────────────────────────────────
    //
    // v6 : inter-passe 4/5 supprimée.
    // Les jours sans fête (DOY 59 bissextile inclus) restent absents de
    // `resolved_days`. `generate_year` les matérialise comme slots padding
    // portant `LiturgicalPeriod` et `liturgical_week` — sans entrée Registre.

    for (&doy, day) in &resolved_days {
        if let Some(&expected_id) = feast_ids.get(&day.primary.slug)
            && expected_id != day.primary.feast_id
        {
            return Err(ForgeError::FeastIDMutated {
                slug: day.primary.slug.clone(),
                expected_id,
                found_id: day.primary.feast_id,
                doy,
                year,
            });
        }
    }

    Ok(ResolvedCalendar {
        year,
        days: resolved_days,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonicalization::MONTH_STARTS;

    /// Construit une PlacedFeast de nature Memoria avec la precedence donnée.
    /// Zéro-allocation hors du String du slug.
    fn make_memoria(slug: &str, feast_id: u16, precedence: u8) -> PlacedFeast {
        PlacedFeast {
            slug: slug.to_string(),
            feast_id,
            scope_bits: 0,
            precedence,
            class: 0,
            nature: CoreNature::Memoria,
            color: CoreColor::Albus,
            period: None,
            has_vigil_mass: false,
            cycle: Cycle::Sanctoral,
        }
    }

    // ── Avent tardif — borne basse : 14 déc. hors plage → pas de mutation ────

    /// Saint Jean de la Croix (14 déc.) est AVANT la plage 17–24 déc.
    /// Invariant : aucune mutation de precedence ni de nature.
    #[test]
    fn test_adventus_tardivus_boundary_ioannis_a_cruce() {
        // DOY = MONTH_STARTS[11] + 14 - 1 — identique bissextile et non-bissextile.
        // DOY 59 absent en non-bissextile ; les autres slots ne sont pas décalés.
        let doy_dec_14 = MONTH_STARTS[11] + 14 - 1;

        let ioannis = make_memoria("ioannis_a_cruce", 1, 9);
        let (primary, _, _) = elect(vec![ioannis], CorePeriod::TempusAdventus, doy_dec_14);

        assert_eq!(
            primary.precedence, 9u8,
            "Invariant brisé : precedence dégradée hors de la plage 17–24 déc."
        );
        assert_eq!(
            primary.nature,
            CoreNature::Memoria,
            "Invariant brisé : nature mutée en Commemoratio hors de la plage 17–24 déc."
        );
    }

    // ── Avent tardif — borne haute : 17 déc. inclus → mutation obligatoire ───

    /// Premier jour de la plage canonique : doit déclencher le déclassement.
    #[test]
    fn test_adventus_tardivus_dec17_demoted() {
        let doy_dec_17 = MONTH_STARTS[11] + 17 - 1; // borne inférieure inclusive
        let feast = make_memoria("o_sapientia", 2, 9);
        let (primary, _, _) = elect(vec![feast], CorePeriod::TempusAdventus, doy_dec_17);

        assert_eq!(
            primary.precedence, 11u8,
            "Invariant brisé : precedence non mutée à 11 au 17 déc."
        );
        assert_eq!(
            primary.nature,
            CoreNature::Commemoratio,
            "Invariant brisé : nature non mutée en Commemoratio au 17 déc."
        );
    }

    // ── Carême — déclassement MemoriaeObligatoriaGenerales (prec. 9) ─────────

    /// Perpétue et Félicité (7 mars, TempusQuadragesimae).
    /// Invariant : precedence → 11, nature → Commemoratio.
    #[test]
    fn test_quadragesimae_demotion_perpetua_et_felicitas() {
        // DOY = MONTH_STARTS[2] + 7 - 1 = 66. Pas de correction bissextile.
        let doy_mar_7 = MONTH_STARTS[2] + 7 - 1;
        let perpetua = make_memoria("perpetua_et_felicitas", 3, 9);
        let (primary, _, _) = elect(vec![perpetua], CorePeriod::TempusQuadragesimae, doy_mar_7);

        assert_eq!(
            primary.precedence, 11u8,
            "Invariant brisé : precedence non dégradée à 11 (MemoriaeAdLibitum) en Carême."
        );
        assert_eq!(
            primary.nature,
            CoreNature::Commemoratio,
            "Invariant brisé : nature non mutée en Commemoratio en Carême."
        );
    }

    // ── Carême — déclassement MemoriaeObligatoriaePropria (prec. 10) ─────────

    /// Même règle pour les mémoires propres : prec. 10 → 11.
    #[test]
    fn test_quadragesimae_demotion_propria() {
        let doy_mar_7 = MONTH_STARTS[2] + 7 - 1;
        let feast = make_memoria("test_propria", 4, 10);
        let (primary, _, _) = elect(vec![feast], CorePeriod::TempusQuadragesimae, doy_mar_7);

        assert_eq!(
            primary.precedence, 11u8,
            "Invariant brisé : MemoriaeObligatoriaePropria non dégradée en Carême."
        );
        assert_eq!(primary.nature, CoreNature::Commemoratio);
    }

    // ── Carême — Déclassement des MemoriaeAdLibitum (prec. 11) ───────────────

    /// Une mémoire facultative (11) en Carême doit basculer en Commemoratio.
    #[test]
    fn test_ad_libitum_becomes_commemoratio_in_lnt() {
        let doy_mar_7 = MONTH_STARTS[2] + 7 - 1;
        let feast = make_memoria("test_ad_libitum", 5, 11);
        let (primary, _, _) = elect(vec![feast], CorePeriod::TempusQuadragesimae, doy_mar_7);

        assert_eq!(
            primary.precedence, 11u8,
            "Invariant : la précédence d'une mémoire facultative reste fixée à 11."
        );
        assert_eq!(
            primary.nature,
            CoreNature::Commemoratio,
            "Invariant brisé : la nature de la mémoire facultative n'a pas muté en Commemoratio."
        );
    }
}
