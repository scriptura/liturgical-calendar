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
//!   FeastVersionDef::transfers  : Vec<TransferRule>
//!   MobileDef::anchor           : String
//!   MobileDef::offset           : i32
//!   MobileDef::ordinal          : Option<u8>         — tempus_ordinarium uniquement
//!   TransferRule::collides      : String
//!   TransferRule::target        : TransferTarget
//!   TransferTarget              : Offset(u32) | Date{m,d} | Mobile{anchor,offset}
//!   CanonicalizedYear::pre_resolved_transfers : BTreeMap<(String,String), u16>
//!   SeasonBoundaries::period_of(&self, doy: u16) -> LiturgicalPeriod

//! Étape 4 — Conflict Resolution : pipeline 5 passes.

#![allow(missing_docs)]

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use liturgical_calendar_core::{
    Color as CoreColor, LiturgicalPeriod, Nature as CoreNature,
};

use crate::{
    canonicalization::{
        is_leap_year, resolve_tempus_ordinarium, CanonicalizedYear, MONTH_STARTS,
    },
    error::ForgeError,
    registry::{
        FeastDef, FeastRegistry, Scope,
        Temporality as RegistryTemporality, // qualification obligatoire — conflit de nom
        TransferTarget,
    },
};

// ─── FeastIdMap ───────────────────────────────────────────────────────────────

/// `slug → FeastID` alloué. INV-FORGE-2 : BTreeMap.
/// Calculé une fois avant la boucle annuelle dans `compile()`.
pub(crate) type FeastIdMap = BTreeMap<String, u16>;

/// Alloue les FeastIDs selon le layout §5.1 (Scope[2] | Category[2] | Sequence[12]).
/// Ordre : lexicographique des slugs par `(scope_bits, category)` — INV-FORGE-3.
/// BTreeMap garantit l'ordre d'itération déterministe.
pub(crate) fn assign_feast_ids(registry: &FeastRegistry) -> FeastIdMap {
    let mut counters: BTreeMap<(u8, u8), u16> = BTreeMap::new();
    let mut result = FeastIdMap::new();

    for feast in registry.iter() {
        let scope_bits: u8 = match &feast.scope {
            Scope::Universal   => 0,
            Scope::National(_) => 1,
            Scope::Diocesan(_) => 2,
        };
        let key = (scope_bits, feast.category);
        let seq = counters.entry(key).or_insert(1);
        if *seq > 0x0FFF {
            // V3 (FeastIDExhausted) — détecté ici silencieusement,
            // erreur fatale complète réservée à l'Étape 1.
            continue;
        }
        let feast_id: u16 = ((scope_bits as u16) << 14)
            | ((feast.category as u16 & 0x3) << 12)
            | (*seq & 0x0FFF);
        result.insert(feast.slug.clone(), feast_id);
        *seq += 1;
    }
    result
}

// ─── Conversions registry → Core ─────────────────────────────────────────────
// Nécessaires car registry::Color / registry::Nature ≠ liturgical_calendar_core::Color/Nature.

fn color_to_core(c: &crate::registry::Color) -> CoreColor {
    use crate::registry::Color as R;
    match c {
        R::Albus     => CoreColor::Albus,
        R::Rubeus    => CoreColor::Rubeus,
        R::Viridis   => CoreColor::Viridis,
        R::Violaceus => CoreColor::Violaceus,
        R::Rosaceus  => CoreColor::Roseus,  // nom différent
        R::Niger     => CoreColor::Niger,
        // Aureus : réservé dans Core v2.0 (valeur 6 non définie).
        // Fallback Albus — à revoir si Core expose Color::Aureus.
        R::Aureus    => CoreColor::Albus,
    }
}

fn nature_to_core(n: &crate::registry::Nature) -> CoreNature {
    use crate::registry::Nature as R;
    match n {
        R::Sollemnitas  => CoreNature::Sollemnitas,
        R::Festum       => CoreNature::Festum,
        R::Memoria      => CoreNature::Memoria,
        R::Feria        => CoreNature::Feria,
        R::Commemoratio => CoreNature::Commemoratio,
    }
}

// ─── Enums de classification ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cycle {
    Temporal  = 0,
    Sanctoral = 1,
}

/// Temporalité de résolution locale — à distinguer de `registry::Temporality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Temporality {
    Fixed  = 0,
    Mobile = 1,
}

// ─── ResolutionKey ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResolutionKey<'a> {
    pub precedence:  u8,
    pub cycle:       Cycle,
    pub temporality: Temporality,
    pub slug:        &'a str,
}

// ─── PlacedFeast ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlacedFeast {
    pub slug:           String,
    pub feast_id:       u16,
    pub scope_bits:     u8,   // 0=Universal 1=National 2=Diocesan
    pub precedence:     u8,
    pub nature:         CoreNature,
    pub color:          CoreColor,
    pub has_vigil_mass: bool,
    pub cycle:          Cycle,
    pub temporality:    Temporality,
}

impl PlacedFeast {
    #[inline]
    fn key(&self) -> ResolutionKey<'_> {
        ResolutionKey {
            precedence:  self.precedence,
            cycle:       self.cycle,
            temporality: self.temporality,
            slug:        &self.slug,
        }
    }
}

impl PartialOrd for PlacedFeast {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for PlacedFeast {
    fn cmp(&self, other: &Self) -> Ordering { self.key().cmp(&other.key()) }
}

// ─── ResolvedDay / ResolvedCalendar ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ResolvedDay {
    pub primary:          PlacedFeast,
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
    feast_id:    u16,
    depth:       u8,
    feast:       PlacedFeast,
}

impl Ord for TransferEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.doy_current, self.feast_id).cmp(&(other.doy_current, other.feast_id))
    }
}
impl PartialOrd for TransferEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

const MAX_TRANSFER_DEPTH: u8 = 7;

struct TransferQueue {
    pending: BTreeSet<TransferEntry>,
}

impl TransferQueue {
    fn new() -> Self { Self { pending: BTreeSet::new() } }

    fn enqueue(
        &mut self, doy_src: u16, feast: PlacedFeast, depth: u8, year: u16,
    ) -> Result<(), ForgeError> {
        if depth > MAX_TRANSFER_DEPTH {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
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

    fn is_empty(&self) -> bool { self.pending.is_empty() }
}

// ─── Déclassement saisonnier — §3.4 ─────────────────────────────────────────

pub(crate) fn should_demote_to_commemoratio(
    feast: &PlacedFeast, period: LiturgicalPeriod,
) -> bool {
    feast.precedence >= 11
        && matches!(
            period,
            LiturgicalPeriod::TempusQuadragesimae
                | LiturgicalPeriod::TempusAdventus
                | LiturgicalPeriod::TriduumPaschale
                | LiturgicalPeriod::DiesSancti
        )
}

// ─── DOY depuis FeastDef.temporality ─────────────────────────────────────────
// Temporality est sur FeastDef, pas sur FeastHistoryEntry.

fn feast_doy(feast_def: &FeastDef, anchors: &BTreeMap<String, u16>) -> Option<u16> {
    match &feast_def.temporality {
        RegistryTemporality::Fixed { month, day } => {
            Some(MONTH_STARTS[*month as usize - 1] + *day as u16 - 1)
        }
        RegistryTemporality::Mobile { anchor, offset } => {
            let anchor_doy = anchors.get(anchor.as_str())?;
            let doy = *anchor_doy as i32 + offset;
            (0..=365).contains(&doy).then_some(doy as u16)
        }
        RegistryTemporality::Ordinal { ordinal } => {
            let adventus = *anchors.get("adventus")?;
            Some(resolve_tempus_ordinarium(adventus, *ordinal))
        }
    }
}

fn feast_cycle_temporality(feast_def: &FeastDef) -> (Cycle, Temporality) {
    match &feast_def.temporality {
        RegistryTemporality::Fixed { .. }             => (Cycle::Sanctoral, Temporality::Fixed),
        RegistryTemporality::Mobile { .. }
        | RegistryTemporality::Ordinal { .. }         => (Cycle::Temporal,  Temporality::Mobile),
    }
}

// ─── Élection canonique ───────────────────────────────────────────────────────

fn elect(
    mut candidates: Vec<PlacedFeast>,
    period:         LiturgicalPeriod,
) -> (PlacedFeast, Vec<PlacedFeast>, Vec<PlacedFeast>) {
    candidates.sort_unstable_by(|a, b| a.key().cmp(&b.key()));

    let primary = candidates.remove(0);
    let temporal_primary = primary.cycle == Cycle::Temporal;

    let mut secondary_feasts = Vec::new();
    let mut to_transfer      = Vec::new();

    for feast in candidates {
if (temporal_primary && should_demote_to_commemoratio(&feast, period)) || feast.precedence >= 8 {
    secondary_feasts.push(feast);
} else if feast.precedence <= 9 && feast.nature != CoreNature::Feria {
    to_transfer.push(feast);
}
        // else : supprimé silencieusement.
    }

    secondary_feasts.sort_unstable_by_key(|f| f.feast_id); // INV-FORGE-4
    (primary, secondary_feasts, to_transfer)
}

// ─── resolve_year ─────────────────────────────────────────────────────────────

pub(crate) fn resolve_year(
    canonicalized: CanonicalizedYear,
    registry:      &FeastRegistry,
    feast_ids:     &FeastIdMap,
) -> Result<ResolvedCalendar, ForgeError> {
    let year    = canonicalized.year;
    let is_leap = is_leap_year(year);

    // ── PASSE 1 ───────────────────────────────────────────────────────────────

    let mut slots: BTreeMap<u16, Vec<PlacedFeast>> = BTreeMap::new();

    for feast_def in registry.iter() {
        let version = match feast_def.active_version_for(year) {
            Some(v) => v,
            None    => continue,
        };

        let doy = match feast_doy(feast_def, &canonicalized.anchors) {
            Some(d) => d,
            None    => continue,
        };

        if !is_leap && doy == 59 { continue; }

        let feast_id = match feast_ids.get(&feast_def.slug) {
            Some(&id) => id,
            None      => continue,
        };

        let scope_bits: u8 = match &feast_def.scope {
            Scope::Universal   => 0,
            Scope::National(_) => 1,
            Scope::Diocesan(_) => 2,
        };

        let (cycle, temporality) = feast_cycle_temporality(feast_def);

        slots.entry(doy).or_default().push(PlacedFeast {
            slug:           feast_def.slug.clone(),
            feast_id,
            scope_bits,
            precedence:     version.precedence,
            nature:         nature_to_core(&version.nature),
            color:          color_to_core(&version.color),
            has_vigil_mass: version.has_vigil_mass,
            cycle,
            temporality,
        });
    }

    // ── PASSE 2 ───────────────────────────────────────────────────────────────

    for (&doy, candidates) in slots.iter_mut() {
        // V7a : Precedence ∈ [0, 3].
        {
            let very_high: Vec<_> = candidates.iter().filter(|f| f.precedence <= 3).collect();
            if very_high.len() >= 2 {
                return Err(ForgeError::SolemnityCollision {
                    slug_a:     very_high[0].slug.clone(),
                    slug_b:     very_high[1].slug.clone(),
                    precedence: very_high[0].precedence,
                    doy, year,
                });
            }
        }

        // V7b : Precedence ∈ [4, 5], même scope.
        {
            let solemn: Vec<_> = candidates.iter()
                .filter(|f| f.precedence >= 4 && f.precedence <= 5)
                .collect();
            for i in 0..solemn.len() {
                for j in (i + 1)..solemn.len() {
                    if solemn[i].scope_bits == solemn[j].scope_bits {
                        return Err(ForgeError::SolemnityCollision {
                            slug_a:     solemn[i].slug.clone(),
                            slug_b:     solemn[j].slug.clone(),
                            precedence: solemn[i].precedence,
                            doy, year,
                        });
                    }
                }
            }
        }

        // §3.1 — scope le plus local prime pour les Solennités.
        if candidates.iter().filter(|f| f.precedence <= 5).count() >= 2 {
            let max_scope = candidates.iter()
                .filter(|f| f.precedence <= 5)
                .map(|f| f.scope_bits)
                .max()
                .unwrap_or(0);
            candidates.retain(|f| !(f.precedence <= 5 && f.scope_bits < max_scope));
        }
    }

    // ── PASSE 3 ───────────────────────────────────────────────────────────────

    let mut resolved_days:      BTreeMap<u16, ResolvedDay>      = BTreeMap::new();
    let mut transfer_queue                                        = TransferQueue::new();
    let mut pending_inserts:    BTreeMap<u16, Vec<PlacedFeast>> = BTreeMap::new();
    let mut retrograde_inserts: Vec<(u16, PlacedFeast)>          = Vec::new();

    for doy in 0u16..=365u16 {
        let mut candidates: Vec<PlacedFeast> = slots.remove(&doy).unwrap_or_default();
        if let Some(fwd) = pending_inserts.remove(&doy) {
            candidates.extend(fwd);
        }
        if candidates.is_empty() { continue; }

        let period = canonicalized.season_boundaries.period_of(doy);
        let (primary, secondary_feasts, to_transfer) = elect(candidates, period);

        for feast in to_transfer {
            let active_rule = registry.get(&feast.slug)
                .and_then(|def| def.active_version_for(year))
                .and_then(|ver| ver.transfers.iter().find(|t| t.collides == primary.slug));

            if let Some(rule) = active_rule {
                let pre_key = (feast.slug.clone(), rule.collides.clone());
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
                        // Mobile sans PreResolved — bug Étape 3 ; fallback générique.
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

        resolved_days.insert(doy, ResolvedDay { primary, secondary_feasts });
    }

    // Inserts rétrogrades — tri par doy_dst pour déterminisme.
    retrograde_inserts.sort_unstable_by_key(|(d, _)| *d);
    for (doy_dst, feast) in retrograde_inserts {
        let period = canonicalized.season_boundaries.period_of(doy_dst);
        if let Some(day) = resolved_days.get_mut(&doy_dst) {
            let mut all = vec![day.primary.clone(), feast];
            all.extend(day.secondary_feasts.clone());
            let (new_primary, new_secondary, _) = elect(all, period);
            day.primary          = new_primary;
            day.secondary_feasts = new_secondary;
        } else {
            resolved_days.insert(doy_dst, ResolvedDay {
                primary: feast, secondary_feasts: Vec::new(),
            });
        }
    }

    // ── PASSE 4 ───────────────────────────────────────────────────────────────

    while let Some(entry) = transfer_queue.pop_first() {
        let TransferEntry { doy_current, feast, depth, .. } = entry;
        let mut placed = false;

        let window_end = (doy_current + 7).min(365);
        for doy_dst in (doy_current + 1)..=window_end {
            let slot_free = match resolved_days.get(&doy_dst) {
                Some(day) => day.primary.precedence > feast.precedence,
                None      => true,
            };
            if !slot_free { continue; }

            let period = canonicalized.season_boundaries.period_of(doy_dst);
            let mut all = vec![feast.clone()];
            if let Some(existing) = resolved_days.remove(&doy_dst) {
                all.push(existing.primary);
                all.extend(existing.secondary_feasts);
            }
            let (new_primary, new_secondary, displaced) = elect(all, period);
            for d in displaced {
                transfer_queue.enqueue(doy_dst, d, depth + 1, year)?;
            }
            resolved_days.insert(doy_dst, ResolvedDay {
                primary: new_primary, secondary_feasts: new_secondary,
            });
            placed = true;
            break;
        }

        if !placed {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_current.saturating_sub(depth as u16),
                blocked_at: doy_current,
                year,
            });
        }
    }

    debug_assert!(transfer_queue.is_empty(), "TransferQueue non vide après Passe 4");

    // ── PASSE 5 ───────────────────────────────────────────────────────────────

    for (&doy, day) in &resolved_days {
        if let Some(&expected_id) = feast_ids.get(&day.primary.slug) {
            if expected_id != day.primary.feast_id {
                return Err(ForgeError::FeastIDMutated {
                    slug:        day.primary.slug.clone(),
                    expected_id,
                    found_id:    day.primary.feast_id,
                    doy, year,
                });
            }
        }
    }

    Ok(ResolvedCalendar { year, days: resolved_days })
}
