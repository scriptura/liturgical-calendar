# Handoff fix Dominica III Quadragesimae sur DOY 59

### 1. Résumé exécutif

En année non bissextile, l'application d'un offset rétrograde à une ancre postérieure au 28 février faisait atterrir le calcul sur le pseudo-DOY 59 (29 février fictif). Ce slot n'existant pas en année commune, la PASSE 1 de `resolve_year` éliminait silencieusement la fête via `if !is_leap && doy == 59 { continue; }`.

Cas déclencheur : `Dominica III Quadragesimae` (`anchor: pascha, offset: -28`) lors d'une année non bissextile avec Pâques au 28 mars (pseudo-DOY 87). Calcul brut : `87 − 28 = 59`.

### 2. Correctif (Transformation de repère continu)

Fichier cible : `crates/forge/src/resolution.rs` (fonction `feast_doy`, branche `RegistryTemporality::Mobile`).

Le pipeline projette l'ancre vers l'espace réel continu (0–364), applique la translation, puis re-projette vers l'espace pseudo-DOY indexé (0–365).

```rust
let anchor_doy = *anchors.get(anchor.as_str())? as i32;
let mut doy = anchor_doy + offset;
if !is_leap_year(year) {
    // 1. Linéarisation : projection vers l'espace réel continu (0..=364)
    let real_anchor = if anchor_doy >= 60 { anchor_doy - 1 } else { anchor_doy };
    let real_doy = real_anchor + offset;
    // 2. Re-projection : réinsertion du slot fantôme (0..=365)
    doy = if real_doy >= 59 { real_doy + 1 } else { real_doy };
    debug_assert!(
        doy != 59,
        "Invariant violé : attributions sur le slot 59 interdites en année commune (year={year})"
    );
}
(0..=365).contains(&doy).then_some(doy as u16)
```

### 3. Suite de tests

À intégrer directement dans `crates/forge/src/resolution.rs` sous `#[cfg(test)] mod tests { ... }` (accès aux fonctions privées du module).

#### 3.0 Helper de fixture

À placer à l'intérieur de `mod tests`. Pas de `#[cfg(test)]` sur la fonction : tout le module est déjà conditionné.

```rust
fn make_dominica_iii_def() -> crate::registry::FeastDef {
    use crate::registry::{
        Color, FeastDef, FeastHistoryEntry, LiturgicalClass, LiturgicalPeriod, Nature,
        Scope, Temporality,
    };
    FeastDef {
        slug: "dominica_iii_quadragesimae".to_string(),
        id: None,
        category: 0,
        class: Some(LiturgicalClass::Lord),
        scope: Scope::Universal,
        temporality: Some(Temporality::Mobile {
            anchor: "pascha".to_string(),
            offset: -28,
        }),
        history: vec![FeastHistoryEntry {
            from: 0,
            to: u16::MAX,
            precedence: Some(2),
            nature: Some(Nature::Dominica),
            color: Some(Color::Violaceus),
            period: Some(LiturgicalPeriod::TempusQuadragesimae),
            has_vigil_mass: false,
            transfers: Vec::new(),
        }],
    }
}
```

#### 3.1 Test de régression multi-années (incluant l'année séculaire 2100)

```rust
#[test]
fn regression_dominica_iii_quadragesimae_easter_march_28() {
    use crate::canonicalization::{
        build_anchor_table, compute_easter, is_leap_year, CanonicalizedYear, SeasonBoundaries,
    };
    use std::collections::BTreeMap;

    const NON_LEAP_EASTER_MARCH_28: &[u16] = &[2027, 2100, 2247, 2309, 2315, 2399];

    let mut registry = crate::registry::FeastRegistry::new();
    registry.insert(make_dominica_iii_def());

    let mut feast_ids = BTreeMap::new();
    feast_ids.insert("dominica_iii_quadragesimae".to_string(), 0x3006);

    for &year in NON_LEAP_EASTER_MARCH_28 {
        assert!(!is_leap_year(year), "L'année {year} doit être non bissextile");
        assert_eq!(
            compute_easter(year),
            87,
            "Pâques {year} doit être au pseudo-DOY 87"
        );

        let canonicalized = CanonicalizedYear {
            year,
            anchors: build_anchor_table(year),
            pre_resolved_transfers: BTreeMap::new(),
            season_boundaries: SeasonBoundaries::compute(year),
        };

        let result = super::resolve_year(canonicalized, &registry, &feast_ids).unwrap();

        let day = result.days.get(&58).expect("DOY 58 manquant");
        assert_eq!(day.primary.slug, "dominica_iii_quadragesimae");
        assert!(
            !result.days.contains_key(&59),
            "Slot 59 illégalement peuplé"
        );
    }
}
```

#### 3.2 Test symétrique : Légitimité du slot 59 en année bissextile (2184)

```rust
#[test]
fn leap_year_mobile_feast_legitimately_lands_on_feb_29() {
    use crate::canonicalization::{
        build_anchor_table, compute_easter, is_leap_year, CanonicalizedYear, SeasonBoundaries,
    };
    use std::collections::BTreeMap;

    let year = 2184u16;
    assert!(is_leap_year(year));
    assert_eq!(compute_easter(year), 87);

    let mut registry = crate::registry::FeastRegistry::new();
    registry.insert(make_dominica_iii_def());

    let mut feast_ids = BTreeMap::new();
    feast_ids.insert("dominica_iii_quadragesimae".to_string(), 0x3006);

    let canonicalized = CanonicalizedYear {
        year,
        anchors: build_anchor_table(year),
        pre_resolved_transfers: BTreeMap::new(),
        season_boundaries: SeasonBoundaries::compute(year),
    };

    let result = super::resolve_year(canonicalized, &registry, &feast_ids).unwrap();

    let day = result
        .days
        .get(&59)
        .expect("Le slot 59 doit être peuplé en année bissextile");
    assert_eq!(day.primary.slug, "dominica_iii_quadragesimae");
}
```

#### 3.3 Property test ciblé (Validation de la bijection inverse)

```rust
#[test]
fn reprojection_bijection_exhaustive() {
    for anchor_real in 0..=364i32 {
        let anchor_pseudo = if anchor_real >= 59 {
            anchor_real + 1
        } else {
            anchor_real
        };
        for offset in -365i32..=365 {
            let real = anchor_real + offset;
            if !(0..=364).contains(&real) {
                continue;
            }
            let real_anchor_calc = if anchor_pseudo >= 60 {
                anchor_pseudo - 1
            } else {
                anchor_pseudo
            };
            let real_calc = real_anchor_calc + offset;
            let pseudo_calc = if real_calc >= 59 {
                real_calc + 1
            } else {
                real_calc
            };
            assert_ne!(
                pseudo_calc, 59,
                "Collision fantôme : anchor_real={anchor_real}, offset={offset}"
            );
            let pseudo_to_real = |p: i32| if p >= 60 { p - 1 } else { p };
            assert_eq!(
                pseudo_to_real(pseudo_calc),
                real,
                "Bijection rompue : pseudo={pseudo_calc} ne se reprojette pas sur real={real}"
            );
        }
    }
}
```

#### 3.4 Invariant sur l'inventaire du corpus réel

```rust
#[test]
fn corpus_mobile_feasts_never_land_on_phantom_slot_non_leap() {
    use crate::canonicalization::{build_anchor_table, is_leap_year};
    use crate::ingestion::ingest_corpus;
    use crate::registry::Temporality;
    use std::path::Path;

    let corpus_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/romanus")
        .canonicalize()
        .expect("Corpus introuvable");
    let registry = ingest_corpus(&corpus_path).expect("Échec ingestion corpus");

    for year in 1969u16..=2399 {
        if is_leap_year(year) {
            continue;
        }
        let anchors = build_anchor_table(year);
        for feast in registry.iter() {
            if !matches!(feast.temporality, Some(Temporality::Mobile { .. })) {
                continue;
            }
            if let Some(doy) = super::feast_doy(feast, &anchors, year) {
                assert_ne!(
                    doy, 59,
                    "Fête '{}' produit DOY 59 en {} (non bissextile)",
                    feast.slug, year
                );
            }
        }
    }
}
```

### 4. Invariants, Observabilité & Chantier Parallèle

**Garde-fou `resolve_year`** : conserver la ligne `if !is_leap && doy == 59 { continue; }` dans la PASSE 1. Son rôle exclusif est de filtrer les fêtes **fixes** assignées au 29 février lors des années communes.

**Protection dev-time** : le `debug_assert!(doy != 59, ...)` placé dans la branche `Mobile` de `feast_doy` (section 2) est la protection active. En build debug, il panique **avant** que la valeur ne puisse atteindre `resolve_year`, ce qui garantit l'intégrité de l'allocateur et fait échouer toute dérive future de la formule au plus tôt. Aucun second `debug_assert!` n'est à ajouter dans `resolve_year` : le garde-fou y serait structurellement inatteignable en debug.

**Protection prod-time** : en build release, `debug_assert!` est désactivé. La ligne `if !is_leap && doy == 59 { continue; }` reste le filet de sécurité : elle empêche la propagation silencieuse d'une entrée corrompue en production. Ce filet est légitime et doit être conservé tel quel.

**Découplage YAML** : la correction de `nature: feria` → `nature: dominica` dans `dominica_iii_quadragesimae.yaml` constitue un chantier distinct. Le présent correctif Forge résout l'invariant de layout mémoire de façon complètement indépendante de la déclaration YAML, qui n'est jamais lue par les tests ci-dessus (fixtures synthétiques).

### 5. Checklist de validation End-to-End

- [ ] Application du correctif dans `crates/forge/src/resolution.rs`.
- [ ] Exécution : `cargo test -p liturgical-calendar-forge`.
- [ ] Compilation binaire : `kal-forge -s universale -i`.
- [ ] Validation CLI binaire (2100 — séculaire non bissextile) :

```bash
cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
    --kald ./artifacts/romanus_universale.kald \
    --lits ./artifacts/romanus_universale_la.lits \
    --year 2100 --doy 58
# Attendu : dominica_iii_quadragesimae

cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
    --kald ./artifacts/romanus_universale.kald \
    --lits ./artifacts/romanus_universale_la.lits \
    --year 2100 --doy 59
# Attendu : slot vide / padding
```

- [ ] Validation CLI binaire (2184 — bissextile) :

```bash
cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
    --kald ./artifacts/romanus_universale.kald \
    --lits ./artifacts/romanus_universale_la.lits \
    --year 2184 --doy 59
# Attendu : dominica_iii_quadragesimae
```

---

## Note de méthode

Ce livrable est la v4 avec quatre retouches ponctuelles. Toute la partie algorithmique, les assertions, les valeurs attendues, les bornes et la checklist sont **inchangées** par rapport à la version validée. C'est délibéré : sur les deux dernières itérations, ce sont les réécritures globales qui ont introduit des régressions. La règle appliquée ici — *diff minimal, périmètre strict, aucune réorganisation* — est celle qui aurait dû prévaloir dès la v3.
