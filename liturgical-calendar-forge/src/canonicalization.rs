use liturgical_calendar_core::LiturgicalPeriod;
use std::collections::BTreeMap;

use crate::error::ForgeError;
use crate::registry::{FeastRegistry, TransferTarget};

// ---------------------------------------------------------------------------
// MONTH_STARTS — table pseudo-DOY (0-indexé, Mar = 60, slot 59 = 29 fév)
// ---------------------------------------------------------------------------

pub const MONTH_STARTS: [u16; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

// ---------------------------------------------------------------------------
// is_leap_year
// ---------------------------------------------------------------------------

pub fn is_leap_year(year: u16) -> bool {
    let y = year as u32;
    y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400)
}

// ---------------------------------------------------------------------------
// Conversions pseudo-DOY ↔ date réelle
// ---------------------------------------------------------------------------

/// Pseudo-DOY → DOY effectif (0-indexé dans l'année civile)
fn pseudo_to_actual(year: u16, pseudo: u16) -> u16 {
    if !is_leap_year(year) && pseudo >= 60 {
        pseudo - 1
    } else {
        pseudo
    }
}

/// DOY effectif (0-indexé) → (mois 1-12, jour 1-31)
fn actual_to_date(year: u16, actual: u16) -> (u8, u8) {
    let leap = is_leap_year(year);
    let month_days: [u16; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut rem = actual;
    for (m, &days) in month_days.iter().enumerate() {
        if rem < days {
            return ((m + 1) as u8, (rem + 1) as u8);
        }
        rem -= days;
    }
    panic!("actual_to_date: DOY {} hors plage pour {}", actual, year);
}

/// (mois, jour) → pseudo-DOY
pub fn date_to_pseudo_doy(_year: u16, month: u8, day: u8) -> u16 {
    MONTH_STARTS[(month - 1) as usize] + (day - 1) as u16
}

// ---------------------------------------------------------------------------
// weekday_of_doy — algorithme Tomohiko Sakamoto
// Retourne 0=Lun … 6=Dim (ISO 8601)
// ---------------------------------------------------------------------------

fn weekday_sakamoto(year: u16, month: u8, day: u8) -> u8 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year as i32;
    let m = month as i32;
    let d = day as i32;
    if m < 3 {
        y -= 1;
    }
    let raw = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7;
    ((raw + 6) % 7) as u8
}

pub fn weekday_of_doy(year: u16, pseudo_doy: u16) -> u8 {
    let actual = pseudo_to_actual(year, pseudo_doy);
    let (month, day) = actual_to_date(year, actual);
    weekday_sakamoto(year, month, day)
}

// ---------------------------------------------------------------------------
// Pâques — Meeus/Jones/Butcher
// ---------------------------------------------------------------------------

pub fn meeus_jones_butcher(year: u16) -> (u8, u8) {
    let y = year as i32;
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = ((h + l - 7 * m + 114) / 31) as u8;
    let day = (((h + l - 7 * m + 114) % 31) + 1) as u8;
    (month, day)
}

pub fn compute_easter(year: u16) -> u16 {
    let (month, day) = meeus_jones_butcher(year);
    date_to_pseudo_doy(year, month, day)
}

// ---------------------------------------------------------------------------
// Résolution des ancres temporelles
// ---------------------------------------------------------------------------

/// Premier dimanche de l'Avent = dimanche le plus proche du 30 novembre (DOY 334)
pub fn resolve_adventus(year: u16) -> u16 {
    let nov30 = 334u16;
    let wd = weekday_of_doy(year, nov30);
    let fwd: i16 = if wd == 6 { 0 } else { 6 - wd as i16 };
    let offset = if fwd <= 3 { fwd } else { fwd - 7 };
    (nov30 as i16 + offset) as u16
}

/// Dimanche dans [Dec 26, Dec 31].
pub fn resolve_nativitas(year: u16) -> u16 {
    let wd = weekday_of_doy(year, 359);
    if wd == 6 {
        return 364;
    }
    359 + (6 - wd) as u16
}

/// Premier dimanche strictement après le 6 janvier (DOY 5).
pub fn resolve_epiphania(year: u16) -> u16 {
    let wd = weekday_of_doy(year, 5);
    let days: u16 = if wd == 6 { 7 } else { (6 - wd) as u16 };
    5 + days
}

/// Premier dimanche tel que la date soit ≥ 2 janvier — plage [Jan 2, Jan 8] (DOY 1–7).
///
/// Utilisé par les scopes nationaux qui déplacent l'Épiphanie au dimanche
/// le plus proche du 6 janvier dans la tradition romaine revisitée
/// (France, Italie, Allemagne, Autriche, Suisse…).
///
/// `in_baptismate_domini` pour ces scopes utilise `anchor: epiphania_dominica, offset: 7`.
pub fn resolve_epiphania_dominica(year: u16) -> u16 {
    // Jan 2 = DOY 1. On cherche le premier dimanche dans DOY [1, 7].
    let wd = weekday_of_doy(year, 1); // jour de la semaine du 2 janvier
    if wd == 6 { 1 } else { 1 + (6 - wd) as u16 }
}

/// Nième dimanche du Temps Ordinaire en fonction du premier dimanche de l'Avent.
/// Utilisé pour le Segment II (Pentecôte → Avent).
pub fn resolve_tempus_ordinarium(adventus_doy: u16, ordinal: u8) -> u16 {
    adventus_doy.saturating_sub(7 * (35 - ordinal as u16))
}

/// Premier lundi après le Baptême du Seigneur (= `epiphania + 1`).
///
/// Ancre du Segment I du Temps Ordinaire.  Sémantiquement, c'est le
/// début de la première semaine ordinaire ; le dimanche de cette semaine
/// (Dominica II) tombe six jours plus tard, soit `epiphania + 7`.
pub fn resolve_tempus_ordinarium_post_epiphaniam(year: u16) -> u16 {
    resolve_epiphania(year) + 1
}

/// Dispatche la résolution d'un dimanche du Temps Ordinaire (ordinal N ≥ 2)
/// entre Segment I (post-Épiphanie) et Segment II (ante-Adventum).
///
/// **Segment I** — `DOY = epiphania + 7·(N−1)`
///   équivalent à `(post_epiphaniam − 1) + 7·(N−1)`.
///
/// **Segment II** — formule à rebours depuis l'Avent.
///
/// Le seuil de basculement (Mercredi des Cendres = `pascha − 46`) est calculé
/// en interne depuis `year` — la fonction ne consulte aucun slot externe et
/// reste conforme à INV-FORGE-4 (ADR-001).
///
/// **Correction pseudo-DOY** : le slot 59 (29 fév) n'existe pas en année
/// non-bissextile.  Une séquence de dimanches issue de janvier traverse ce
/// slot sans y atterrir : tout `seg1_raw ≥ 59` est incrémenté d'un jour pour
/// rester sur un dimanche réel dans le calendrier effectif.
pub fn resolve_tempus_ordinarium_dispatch(
    year: u16,
    post_epiphaniam: u16,
    adventus_doy: u16,
    ordinal: u8,
) -> u16 {
    debug_assert!(
        ordinal >= 2,
        "L'ordinal I n'est pas associé à un dimanche propre"
    );
    let ash_wednesday = compute_easter(year).saturating_sub(46);
    let seg1_raw = (post_epiphaniam - 1) + 7 * (ordinal as u16 - 1);
    let seg1 = if !is_leap_year(year) && seg1_raw >= 59 {
        seg1_raw + 1
    } else {
        seg1_raw
    };
    if seg1 < ash_wednesday {
        seg1
    } else {
        resolve_tempus_ordinarium(adventus_doy, ordinal)
    }
}

// ---------------------------------------------------------------------------
// AnchorTable — INV-FORGE-2 : BTreeMap
// ---------------------------------------------------------------------------

pub type AnchorTable = BTreeMap<String, u16>;

pub fn build_anchor_table(year: u16) -> AnchorTable {
    let mut t = BTreeMap::new();
    let nativitas = resolve_nativitas(year);
    let epiphania = resolve_epiphania(year);
    let epiphania_dominica = resolve_epiphania_dominica(year);
    let adventus = resolve_adventus(year);
    let easter = compute_easter(year);
    let pentecost = easter + 49;
    let post_epiphaniam = resolve_tempus_ordinarium_post_epiphaniam(year);
    t.insert("nativitas".to_string(), nativitas);
    t.insert("epiphania".to_string(), epiphania);
    t.insert("epiphania_dominica".to_string(), epiphania_dominica);
    t.insert("adventus".to_string(), adventus);
    t.insert("pascha".to_string(), easter);
    t.insert("pentecostes".to_string(), pentecost);
    t.insert(
        "tempus_ordinarium_post_epiphaniam".to_string(),
        post_epiphaniam,
    );
    t
}

// ---------------------------------------------------------------------------
// SeasonBoundaries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SeasonBoundaries {
    pub adventus: u16,
    pub nativitas: u16,
    pub epiphania: u16,
    pub epiphania_dominica: u16,
    pub ash_wednesday: u16,
    pub palm_sunday: u16,
    pub easter: u16,
    pub pentecost: u16,
}

impl SeasonBoundaries {
    pub fn compute(year: u16) -> Self {
        let easter = compute_easter(year);
        let leap = is_leap_year(year);

        // Mercredi des Cendres : 46 jours calendaires avant Pâques.
        // En pseudo-DOY, le slot 59 (29 fév fictif) s'intercale entre
        // Jan–Fév (DOY ≤ 58) et Mars+ (DOY ≥ 60) en année non-bissextile.
        // Pâques est toujours après le slot 59 (DOY ≥ 81).
        // Si le résultat brut atterrit sur le slot fantôme ou avant,
        // la soustraction a traversé le slot fictif → correction de −1.
        let raw_ash = easter.saturating_sub(46);
        let ash_wednesday = if !leap && raw_ash <= 59 {
            raw_ash - 1
        } else {
            raw_ash
        };

        Self {
            adventus: resolve_adventus(year),
            nativitas: resolve_nativitas(year),
            epiphania: resolve_epiphania(year),
            epiphania_dominica: resolve_epiphania_dominica(year),
            ash_wednesday,
            palm_sunday: easter.saturating_sub(7),
            easter,
            pentecost: easter + 49,
        }
    }

    pub fn period_of(&self, doy: u16) -> LiturgicalPeriod {
        // Nativitas Domini : pseudo-DOY fixe = MONTH_STARTS[11] + 24 = 335 + 24 = 359.
        // Indépendant de la bissextilité — Dec 25 est toujours DOY 359.
        const NAT_DOMINI: u16 = 359;

        // Triduum Paschale : Feria V in Cena Domini → Sabbatum Sanctum.
        let triduum_start = self.easter.saturating_sub(3);
        let triduum_end = self.easter.saturating_sub(1);
        if doy >= triduum_start && doy <= triduum_end {
            return LiturgicalPeriod::TriduumPaschale;
        }

        // Dies Sancti (Semaine Sainte) : Dominica in Palmis → Feria IV.
        let dies_sancti_end = self.easter.saturating_sub(4);
        if doy >= self.palm_sunday && doy <= dies_sancti_end {
            return LiturgicalPeriod::DiesSancti;
        }

        // Tempus Paschale : Dominica Resurrectionis → Pentecostes.
        if doy >= self.easter && doy <= self.pentecost {
            return LiturgicalPeriod::TempusPaschale;
        }

        // Tempus Quadragesimae : Feria IV Cinerum → Feria IV ante Dominicam in Palmis.
        if doy >= self.ash_wednesday && doy < self.palm_sunday {
            return LiturgicalPeriod::TempusQuadragesimae;
        }

        // Tempus Adventus : Dominica I Adventus → Vigilia Nativitatis (24 déc, DOY 358).
        // Borne haute exclusive : NAT_DOMINI (359) — Dec 25 appartient au Temps de Noël.
        if doy >= self.adventus && doy < NAT_DOMINI {
            return LiturgicalPeriod::TempusAdventus;
        }

        // Tempus Nativitatis (deux segments, pas de condition intermédiaire) :
        //   Segment I  — Nativitas Domini (25 déc, DOY 359) → fin d'année civile.
        //   Segment II — 1er janvier (DOY 0) → Baptisma Domini (self.epiphania, inclus).
        // Dominica II per annum = self.epiphania + 7 → TempusOrdinarium (exclus).
        if doy >= NAT_DOMINI || doy <= self.epiphania {
            return LiturgicalPeriod::TempusNativitatis;
        }

        LiturgicalPeriod::TempusOrdinarium
    }
}

// ---------------------------------------------------------------------------
// PreResolvedTransfers
// ---------------------------------------------------------------------------

pub type PreResolvedTransfers = BTreeMap<(String, String), u16>;

fn resolve_mobile_transfer_targets(
    registry: &FeastRegistry,
    anchors: &AnchorTable,
) -> Result<PreResolvedTransfers, ForgeError> {
    let mut result = BTreeMap::new();

    for feast in registry.iter() {
        for entry in &feast.history {
            for transfer in &entry.transfers {
                if let TransferTarget::Mobile { anchor, offset } = &transfer.target {
                    let anchor_doy = anchors.get(anchor.as_str()).ok_or_else(|| {
                        ForgeError::UnresolvedAnchor {
                            anchor: anchor.clone(),
                        }
                    })?;
                    let doy_dst = (*anchor_doy as i32 + offset) as u16;
                    for c in &transfer.collides {
                        result.insert((feast.slug.clone(), c.clone()), doy_dst);
                    }
                }
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// CanonicalizedYear
// ---------------------------------------------------------------------------

pub struct CanonicalizedYear {
    pub year: u16,
    pub anchors: AnchorTable,
    pub season_boundaries: SeasonBoundaries,
    pub pre_resolved_transfers: PreResolvedTransfers,
}

pub fn canonicalize_year(
    year: u16,
    registry: &FeastRegistry,
) -> Result<CanonicalizedYear, ForgeError> {
    let anchors = build_anchor_table(year);
    let season_boundaries = SeasonBoundaries::compute(year);
    let pre_resolved_transfers = resolve_mobile_transfer_targets(registry, &anchors)?;

    Ok(CanonicalizedYear {
        year,
        anchors,
        season_boundaries,
        pre_resolved_transfers,
    })
}

// ---------------------------------------------------------------------------
// Tests unitaires
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- MONTH_STARTS ---

    #[test]
    fn month_starts_jan_mar() {
        assert_eq!(MONTH_STARTS[0], 0);
        assert_eq!(MONTH_STARTS[2], 60);
    }

    // --- is_leap_year ---

    #[test]
    fn leap_year() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(2100));
        assert!(is_leap_year(2000));
    }

    // --- Pâques ---

    #[test]
    fn easter_2025() {
        assert_eq!(compute_easter(2025), 110);
    }

    #[test]
    fn easter_2000() {
        assert_eq!(compute_easter(2000), 113);
    }

    #[test]
    fn easter_bounds_all_years() {
        for year in 1969u16..=2399 {
            let doy = compute_easter(year);
            assert!(
                doy >= 81 && doy <= 115 && doy != 59,
                "year {}: DOY {} hors [81,115] ou == 59",
                year,
                doy
            );
        }
    }

    // --- Tempus Ordinarium ---

    #[test]
    fn tempus_ordinarium_34th() {
        assert_eq!(resolve_tempus_ordinarium(333, 34), 326);
    }

    #[test]
    fn tempus_ordinarium_1st() {
        assert_eq!(resolve_tempus_ordinarium(333, 1), 95);
    }

    // --- Tempus Ordinarium — Segment I (post-Épiphanie) ---

    #[test]
    fn post_epiphaniam_2026() {
        // epiphania(2026) = DOY 10 (Jan 11) → post_epiphaniam = DOY 11 (lun. Jan 12)
        assert_eq!(resolve_tempus_ordinarium_post_epiphaniam(2026), 11);
    }

    #[test]
    fn post_epiphaniam_is_monday() {
        // Le lundi suit immédiatement le dimanche du Baptême.
        // Donc post_epiphaniam − 1 = epiphania = dimanche (wd 6).
        // post_epiphaniam doit être un lundi (wd 0).
        for year in 1969u16..=2399 {
            let doy = resolve_tempus_ordinarium_post_epiphaniam(year);
            assert_eq!(
                weekday_of_doy(year, doy),
                0,
                "year {}: post_epiphaniam DOY {} n'est pas un lundi",
                year,
                doy
            );
        }
    }

    #[test]
    fn dispatch_2026_segment_i_ordinaux_ii_vi() {
        // 2026 : epiphania = 10, ash_wednesday = 48 (corrigé phantom slot), adventus = 333
        // Dominica II–VI restent dans le Segment I.
        let post_ep = resolve_tempus_ordinarium_post_epiphaniam(2026);
        let adventus = resolve_adventus(2026);
        let expected = [17u16, 24, 31, 38, 45]; // ordinaux II–VI
        for (i, &exp) in expected.iter().enumerate() {
            let ord = (i + 2) as u8;
            assert_eq!(
                resolve_tempus_ordinarium_dispatch(2026, post_ep, adventus, ord),
                exp,
                "Dominica {} per annum 2026",
                ord
            );
        }
    }

    #[test]
    fn dispatch_2026_segment_i_boundary() {
        // Dominica VII 2026 : DOY seg1 = 10 + 42 = 52 ≥ ash(48) → Segment II.
        let post_ep = resolve_tempus_ordinarium_post_epiphaniam(2026);
        let adventus = resolve_adventus(2026);
        let seg2 = resolve_tempus_ordinarium_dispatch(2026, post_ep, adventus, 7);
        assert_eq!(seg2, resolve_tempus_ordinarium(adventus, 7));
        assert_ne!(seg2, 52);
    }

    #[test]
    fn dispatch_2003_pseudo_doy_gap_ordinal_8() {
        // 2003 (non-bissextile) : epiphania = DOY 11, ordinal 8 → seg1_raw = 60.
        // Le slot DOY 59 (29 fév fictif) est traversé sans y atterrir : +1 de correction.
        // seg1 = 61 (2 mars) < ash_wednesday(63) → Segment I.
        // Sans correction : DOY 60 = 1er mars = samedi (décalage d'un jour après le gap).
        let post_ep = resolve_tempus_ordinarium_post_epiphaniam(2003);
        let adventus = resolve_adventus(2003);
        let doy = resolve_tempus_ordinarium_dispatch(2003, post_ep, adventus, 8);
        assert_eq!(doy, 61, "Dominica VIII 2003 : DOY 61 attendu (2 mars)");
        assert_eq!(weekday_of_doy(2003, doy), 6, "DOY 61 doit être un dimanche");
    }

    #[test]
    fn dispatch_segment_i_invariant_all_years() {
        // Pour tout ordinal en Segment I, le DOY résolu doit être un dimanche.
        for year in 1969u16..=2399 {
            let post_ep = resolve_tempus_ordinarium_post_epiphaniam(year);
            let adventus = resolve_adventus(year);
            for ord in 2u8..=8 {
                let doy = resolve_tempus_ordinarium_dispatch(year, post_ep, adventus, ord);
                assert_eq!(
                    weekday_of_doy(year, doy),
                    6,
                    "year {} ord {}: DOY {} n'est pas un dimanche",
                    year,
                    ord,
                    doy
                );
            }
        }
    }

    // --- Epiphania (Baptême du Seigneur) ---

    #[test]
    fn epiphania_2025_after_jan6() {
        // Jan 6 2025 = lundi (wd=0) → premier dimanche après = Jan 12 = DOY 11
        assert_eq!(resolve_epiphania(2025), 11);
    }

    // --- Epiphania Dominica (Épiphanie au dimanche, scopes nationaux) ---

    #[test]
    fn epiphania_dominica_range_is_jan2_jan8() {
        // Doit toujours tomber dans [DOY 1, DOY 7] = [Jan 2, Jan 8]
        for year in 1969u16..=2399 {
            let doy = resolve_epiphania_dominica(year);
            assert!(
                doy >= 1 && doy <= 7,
                "year {}: epiphania_dominica DOY {} hors [1, 7]",
                year,
                doy
            );
        }
    }

    #[test]
    fn epiphania_dominica_is_always_sunday() {
        for year in 1969u16..=2399 {
            let doy = resolve_epiphania_dominica(year);
            assert_eq!(
                weekday_of_doy(year, doy),
                6,
                "year {}: epiphania_dominica DOY {} n'est pas un dimanche",
                year,
                doy
            );
        }
    }

    #[test]
    fn epiphania_dominica_2025_is_jan5() {
        // Jan 2 2025 = jeudi (wd=3) → premier dimanche ≥ Jan 2 = Jan 5 = DOY 4
        assert_eq!(resolve_epiphania_dominica(2025), 4);
    }

    #[test]
    fn epiphania_dominica_2026_is_jan4() {
        // Jan 2 2026 = vendredi (wd=4) → premier dimanche = Jan 4 = DOY 3
        assert_eq!(resolve_epiphania_dominica(2026), 3);
    }

    #[test]
    fn epiphania_dominica_when_jan2_is_sunday() {
        // Trouver une année où Jan 2 est un dimanche → DOY doit être 1
        // Jan 2 = dimanche si Jan 1 = samedi
        // 2021 : Jan 1 = vendredi → Jan 2 = samedi → non
        // 2000 : Jan 1 = samedi → Jan 2 = dimanche → DOY 1
        assert_eq!(resolve_epiphania_dominica(2000), 1);
        assert_eq!(weekday_of_doy(2000, 1), 6);
    }

    #[test]
    fn adventus_2025_is_nov30() {
        assert_eq!(resolve_adventus(2025), 334);
    }

    // --- ash_wednesday --- correction pseudo-DOY phantom slot

    #[test]
    fn ash_wednesday_2026_is_feb18() {
        // Easter 2026 = DOY 95 (April 5). raw = 95−46 = 49 = Feb 19 (jeudi).
        // Correction phantom : !leap && 49 ≤ 59 → ash_wednesday = 48 = Feb 18 (mercredi).
        let sb = SeasonBoundaries::compute(2026);
        assert_eq!(
            sb.ash_wednesday, 48,
            "Mercredi des Cendres 2026 doit être DOY 48 (18 fév)"
        );
        assert_eq!(
            weekday_of_doy(2026, sb.ash_wednesday),
            2,
            "DOY 48 doit être un mercredi"
        );
    }

    #[test]
    fn ash_wednesday_is_always_wednesday() {
        for year in 1969u16..=2399 {
            let sb = SeasonBoundaries::compute(year);
            assert_eq!(
                weekday_of_doy(year, sb.ash_wednesday),
                2,
                "year {}: ash_wednesday DOY {} n'est pas un mercredi",
                year,
                sb.ash_wednesday
            );
        }
    }

    // --- period_of --- bornes TempusNativitatis

    #[test]
    fn period_nativitas_domini_is_tempus_nativitatis() {
        // Dec 25 = DOY 359 doit toujours être TempusNativitatis, jamais TempusAdventus.
        for year in 1969u16..=2399 {
            let sb = SeasonBoundaries::compute(year);
            assert_eq!(
                sb.period_of(359),
                LiturgicalPeriod::TempusNativitatis,
                "year {}: DOY 359 (25 déc) doit être TempusNativitatis",
                year
            );
        }
    }

    #[test]
    fn period_dominica_ii_per_annum_is_tempus_ordinarium() {
        // Dominica II per annum = epiphania + 7.
        // Doit être TempusOrdinarium, pas TempusNativitatis.
        for year in 1969u16..=2399 {
            let sb = SeasonBoundaries::compute(year);
            let dominica_ii = sb.epiphania + 7;
            assert_eq!(
                sb.period_of(dominica_ii),
                LiturgicalPeriod::TempusOrdinarium,
                "year {}: Dominica II per annum (DOY {}) doit être TempusOrdinarium",
                year,
                dominica_ii
            );
        }
    }

    #[test]
    fn period_ash_wednesday_is_tempus_quadragesimae() {
        for year in 1969u16..=2399 {
            let sb = SeasonBoundaries::compute(year);
            assert_eq!(
                sb.period_of(sb.ash_wednesday),
                LiturgicalPeriod::TempusQuadragesimae,
                "year {}: ash_wednesday DOY {} doit être TempusQuadragesimae",
                year,
                sb.ash_wednesday
            );
        }
    }

    #[test]
    fn period_epiphania_is_tempus_nativitatis() {
        // Le Baptême du Seigneur (epiphania) est le dernier jour du Temps de Noël.
        for year in 1969u16..=2399 {
            let sb = SeasonBoundaries::compute(year);
            assert_eq!(
                sb.period_of(sb.epiphania),
                LiturgicalPeriod::TempusNativitatis,
                "year {}: epiphania DOY {} doit être TempusNativitatis",
                year,
                sb.epiphania
            );
        }
    }

    // --- date_to_pseudo_doy ---

    #[test]
    fn date_to_doy_april_20() {
        assert_eq!(date_to_pseudo_doy(2025, 4, 20), 110);
    }

    #[test]
    fn date_to_doy_march_1_invariant() {
        assert_eq!(date_to_pseudo_doy(2025, 3, 1), 60);
        assert_eq!(date_to_pseudo_doy(2024, 3, 1), 60);
    }

    // --- weekday_of_doy ---

    #[test]
    fn weekday_nov30_2025_is_sunday() {
        assert_eq!(weekday_of_doy(2025, 334), 6);
    }

    #[test]
    fn weekday_easter_2025_is_sunday() {
        assert_eq!(weekday_of_doy(2025, compute_easter(2025)), 6);
    }

    // --- PreResolvedTransfers ---
    //
    // YAML precedence: 2 → interne 1 (SollemnitatesFixaeMaior).

    #[test]
    fn pre_resolved_transfer_pascha_offset() {
        use crate::parsing::parse_feast_from_yaml;
        use crate::registry::{FeastRegistry, Scope};

        let yaml_iosephi = r#"
version: 1
category: 1
class: lord
date:
  month: 3
  day: 19
history:
  - precedence: 2
    nature: sollemnitas
    color: albus
    transfers:
      - collides: dominica_in_palmis_de_passione_domini
        mobile:
          anchor: pascha
          offset: -8
"#;
        let yaml_palmis = r#"
version: 1
category: 0
class: lord
mobile:
  anchor: pascha
  offset: -7
history:
  - precedence: 2
    nature: sollemnitas
    color: rubeus
"#;

        let mut registry = FeastRegistry::new();
        let def_iosephi = parse_feast_from_yaml("iosephi", Scope::Universal, yaml_iosephi).unwrap();
        let def_palmis = parse_feast_from_yaml(
            "dominica_in_palmis_de_passione_domini",
            Scope::Universal,
            yaml_palmis,
        )
        .unwrap();
        registry.insert(def_iosephi);
        registry.insert(def_palmis);

        // 2016 : Pâques = 27 mars = DOY 86
        let easter_2016 = compute_easter(2016);
        assert_eq!(easter_2016, 86, "Pâques 2016 doit être DOY 86 (27 mars)");

        let cy = canonicalize_year(2016, &registry).unwrap();
        let key = (
            "iosephi".to_string(),
            "dominica_in_palmis_de_passione_domini".to_string(),
        );
        let doy_dst = cy.pre_resolved_transfers[&key];
        // pascha(86) + (−8) = 78 = Mar 19
        assert_eq!(doy_dst, 78);
    }
}
