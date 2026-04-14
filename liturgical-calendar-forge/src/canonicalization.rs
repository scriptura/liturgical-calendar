use std::collections::BTreeMap;

use crate::error::ForgeError;
use crate::registry::{FeastRegistry, TransferTarget};

// ---------------------------------------------------------------------------
// MONTH_STARTS — table pseudo-DOY (0-indexé, Mar = 60, slot 59 = 29 fév)
// ---------------------------------------------------------------------------

pub const MONTH_STARTS: [u16; 12] =
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

// ---------------------------------------------------------------------------
// is_leap_year
// ---------------------------------------------------------------------------

pub fn is_leap_year(year: u16) -> bool {
    let y = year as u32;
    y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400)
}

// ---------------------------------------------------------------------------
// Conversions pseudo-DOY ↔ date réelle
//
// Pseudo-DOY : slot 59 réservé à Feb 29 même en année non-bissextile.
// Pour les années non-bissextiles, Mar 1 = pseudo-DOY 60 = DOY effectif 59.
// Résultat : MONTH_STARTS est invariant quelle que soit l'année.
// ---------------------------------------------------------------------------

/// Pseudo-DOY → DOY effectif (0-indexé dans l'année civile)
fn pseudo_to_actual(year: u16, pseudo: u16) -> u16 {
    if !is_leap_year(year) && pseudo >= 60 { pseudo - 1 } else { pseudo }
}

/// DOY effectif (0-indexé) → (mois 1-12, jour 1-31)
fn actual_to_date(year: u16, actual: u16) -> (u8, u8) {
    let leap = is_leap_year(year);
    let month_days: [u16; 12] =
        [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
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
    if m < 3 { y -= 1; }
    // raw : 0=Dim, 1=Lun, …, 6=Sam
    let raw = (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7;
    // Conversion → 0=Lun … 6=Dim
    ((raw + 6) % 7) as u8
}

pub fn weekday_of_doy(year: u16, pseudo_doy: u16) -> u8 {
    let actual = pseudo_to_actual(year, pseudo_doy);
    let (month, day) = actual_to_date(year, actual);
    weekday_sakamoto(year, month, day)
}

// ---------------------------------------------------------------------------
// Pâques — Meeus/Jones/Butcher
// Bornes garanties : DOY ∈ [81, 115], DOY ≠ 59
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
    let day   = (((h + l - 7 * m + 114) % 31) + 1) as u8;
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
    let wd = weekday_of_doy(year, nov30); // 0=Lun … 6=Dim
    // Jours jusqu'au prochain dimanche (0 si déjà dimanche)
    let fwd: i16 = if wd == 6 { 0 } else { 6 - wd as i16 };
    // Si fwd <= 3 : avancer, sinon : reculer (fwd - 7 < 0)
    let offset = if fwd <= 3 { fwd } else { fwd - 7 };
    (nov30 as i16 + offset) as u16
}

/// Dimanche dans [Dec 26, Dec 31].
/// Si Noël (DOY 359 = Dec 25) est dimanche → fallback DOY 364 (Dec 30).
pub fn resolve_nativitas(year: u16) -> u16 {
    let wd = weekday_of_doy(year, 359); // 0=Lun … 6=Dim
    if wd == 6 {
        return 364; // Dec 25 = Dim → Ste-Famille = Dec 30
    }
    359 + (6 - wd) as u16
}

/// Premier dimanche strictement après le 6 janvier (DOY 5).
pub fn resolve_epiphania(year: u16) -> u16 {
    let wd = weekday_of_doy(year, 5); // 0=Lun … 6=Dim
    let days: u16 = if wd == 6 { 7 } else { (6 - wd) as u16 };
    5 + days
}

/// Nième dimanche du Temps Ordinaire en fonction du premier dimanche de l'Avent.
/// ordinal ∈ [1, 34] (validé en Étape 1, V4a)
pub fn resolve_tempus_ordinarium(adventus_doy: u16, ordinal: u8) -> u16 {
    adventus_doy.saturating_sub(7 * (35 - ordinal as u16))
}

// ---------------------------------------------------------------------------
// AnchorTable — INV-FORGE-2 : BTreeMap
// ---------------------------------------------------------------------------

pub type AnchorTable = BTreeMap<String, u16>;

/// Construit la table des ancres pour une année donnée.
/// Ordre de résolution conforme à la spec (1→6, pentecostes = pascha+49).
pub fn build_anchor_table(year: u16) -> AnchorTable {
    let mut t = BTreeMap::new();
    let nativitas  = resolve_nativitas(year);
    let epiphania  = resolve_epiphania(year);
    let adventus   = resolve_adventus(year);
    let easter     = compute_easter(year);
    let pentecost  = easter + 49;
    t.insert("nativitas".to_string(),  nativitas);
    t.insert("epiphania".to_string(),  epiphania);
    t.insert("adventus".to_string(),   adventus);
    t.insert("pascha".to_string(),     easter);
    t.insert("pentecostes".to_string(), pentecost);
    // tempus_ordinarium résolu à la demande (dépend de ordinal par fête)
    t
}

// ---------------------------------------------------------------------------
// SeasonBoundaries — jalons de l'année liturgique
// ---------------------------------------------------------------------------

pub struct SeasonBoundaries {
    pub adventus:      u16,
    pub nativitas:     u16,
    pub epiphania:     u16,
    pub ash_wednesday: u16, // easter - 46
    pub palm_sunday:   u16, // easter - 7
    pub easter:        u16,
    pub pentecost:     u16, // easter + 49
}

impl SeasonBoundaries {
    pub fn for_year(year: u16) -> Self {
        let easter = compute_easter(year);
        Self {
            adventus:      resolve_adventus(year),
            nativitas:     resolve_nativitas(year),
            epiphania:     resolve_epiphania(year),
            ash_wednesday: easter - 46,
            palm_sunday:   easter - 7,
            easter,
            pentecost:     easter + 49,
        }
    }
}

// ---------------------------------------------------------------------------
// PreResolvedTransfers — INV-FORGE-2 : BTreeMap
// Clé : (slug_fête, slug_collides)  — Valeur : DOY absolu de la cible
// ---------------------------------------------------------------------------

pub type PreResolvedTransfers = BTreeMap<(String, String), u16>;

fn resolve_mobile_transfer_targets(
    registry: &FeastRegistry,
    anchors:  &AnchorTable,
) -> Result<PreResolvedTransfers, ForgeError> {
    let mut result = BTreeMap::new();

    for feast in registry.iter() {
        for transfer in &feast.transfers {
            if let TransferTarget::Mobile { anchor, offset } = &transfer.target {
                let anchor_doy = anchors.get(anchor.as_str())
                    .ok_or_else(|| ForgeError::UnresolvedAnchor { anchor: anchor.clone() })?;
                let doy_dst = (*anchor_doy as i32 + offset) as u16;
                result.insert(
                    (feast.slug.clone(), transfer.collides.clone()),
                    doy_dst,
                );
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// CanonicalizedYear — livrable de canonicalization pour une année
// ---------------------------------------------------------------------------

pub struct CanonicalizedYear {
    pub year:                    u16,
    pub anchors:                 AnchorTable,
    pub season_boundaries:       SeasonBoundaries,
    pub pre_resolved_transfers:  PreResolvedTransfers,
}

pub fn canonicalize_year(year: u16, registry: &FeastRegistry)
    -> Result<CanonicalizedYear, ForgeError>
{
    let anchors              = build_anchor_table(year);
    let season_boundaries    = SeasonBoundaries::for_year(year);
    let pre_resolved_transfers = resolve_mobile_transfer_targets(registry, &anchors)?;

    Ok(CanonicalizedYear { year, anchors, season_boundaries, pre_resolved_transfers })
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
        assert_eq!(MONTH_STARTS[0], 0);   // Jan 1 = DOY 0
        assert_eq!(MONTH_STARTS[2], 60);  // Mar 1 = DOY 60 (slot 59 = Feb 29)
    }

    // --- is_leap_year ---

    #[test]
    fn leap_year() {
        assert!( is_leap_year(2024));
        assert!(!is_leap_year(2025));
        assert!(!is_leap_year(2100)); // divisible par 100, pas par 400
        assert!( is_leap_year(2000)); // divisible par 400
    }

    // --- Pâques ---

    #[test]
    fn easter_2025() {
        // 20 avr = MONTH_STARTS[3] + 19 = 91 + 19 = 110
        assert_eq!(compute_easter(2025), 110);
    }

    #[test]
    fn easter_2000() {
        // 23 avr = 91 + 22 = 113
        assert_eq!(compute_easter(2000), 113);
    }

    #[test]
    fn easter_bounds_all_years() {
        for year in 1969u16..=2399 {
            let doy = compute_easter(year);
            assert!(
                doy >= 81 && doy <= 115 && doy != 59,
                "year {}: DOY {} hors [81,115] ou == 59", year, doy
            );
        }
    }

    // --- Tempus Ordinarium ---

    #[test]
    fn tempus_ordinarium_34th() {
        // Christ-Roi : 34e dimanche OT, adventus=333 (valeur synthétique de test)
        assert_eq!(resolve_tempus_ordinarium(333, 34), 326);
    }

    #[test]
    fn tempus_ordinarium_1st() {
        assert_eq!(resolve_tempus_ordinarium(333, 1), 95);
    }

    // --- Adventus 2025 ---

    #[test]
    fn adventus_2025_is_nov30() {
        // Nov 30 2025 est un dimanche → adventus = DOY 334
        assert_eq!(resolve_adventus(2025), 334);
    }

    // --- date_to_pseudo_doy ---

    #[test]
    fn date_to_doy_april_20() {
        // Pâques 2025 : (4, 20) → DOY 110
        assert_eq!(date_to_pseudo_doy(2025, 4, 20), 110);
    }

    #[test]
    fn date_to_doy_march_1_invariant() {
        // Mar 1 = DOY 60 quelle que soit l'année (slot 59 réservé Feb 29)
        assert_eq!(date_to_pseudo_doy(2025, 3, 1), 60);
        assert_eq!(date_to_pseudo_doy(2024, 3, 1), 60);
    }

    // --- weekday_of_doy sanity ---

    #[test]
    fn weekday_nov30_2025_is_sunday() {
        // Dimanche = 6 dans convention 0=Lun…6=Dim
        assert_eq!(weekday_of_doy(2025, 334), 6);
    }

    #[test]
    fn weekday_easter_2025_is_sunday() {
        // Pâques est toujours un dimanche
        assert_eq!(weekday_of_doy(2025, compute_easter(2025)), 6);
    }

    // --- PreResolvedTransfers (intégration minimale) ---

    #[test]
    fn pre_resolved_transfer_pascha_offset() {
        use crate::registry::{FeastRegistry, Scope};
        use crate::parsing::parse_feast_from_yaml;

        // Fête iosephi (19 mars) avec un transfer mobile vers pascha-8
        // Pâques 2016 = 27 mars = MONTH_STARTS[2] + 26 = 60 + 26 = 86
        // pascha 2016 = DOY 86 → cible = 86 - 8 = 78 (19 mars = DOY 78)

        let yaml_iosephi = "\
version: 1
category: 1
date:
  month: 3
  day: 19
transfers:
  - collides: dominica_in_palmis
    mobile:
      anchor: pascha
      offset: -8
history:
  - precedence: 1
    nature: sollemnitas
    color: white
";
        let yaml_palmis = "\
version: 1
category: 0
mobile:
  anchor: pascha
  offset: -7
history:
  - precedence: 1
    nature: sollemnitas
    color: red
";

        let mut registry = FeastRegistry::new();
        let def_iosephi = parse_feast_from_yaml("iosephi", Scope::Universal, yaml_iosephi).unwrap();
        let def_palmis  = parse_feast_from_yaml("dominica_in_palmis", Scope::Universal, yaml_palmis).unwrap();
        registry.insert(def_iosephi);
        registry.insert(def_palmis);

        // 2016 : Pâques = 27 mars
        let easter_2016 = compute_easter(2016);
        assert_eq!(easter_2016, 86, "Pâques 2016 doit être DOY 86 (27 mars)");

        let cy = canonicalize_year(2016, &registry).unwrap();
        let key = ("iosephi".to_string(), "dominica_in_palmis".to_string());
        let doy_dst = cy.pre_resolved_transfers[&key];
        // pascha(86) + (-8) = 78 = Mar 19
        assert_eq!(doy_dst, 78);
    }
}
