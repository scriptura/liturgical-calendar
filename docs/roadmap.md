# Roadmap de Développement : Liturgical Calendar v2.2

**Version** : 2.2.1  
**Date de Révision** : 2026-04-10  
**Méthodologie** : 3 jalons, chacun produisant un livrable binaire validable indépendamment  
**Critères de Succès** : Conformité binaire Forge↔Engine · SHA-256 cross-platform · Fuzzing · CI 4 cibles

---

## État du Corpus (2026-04-10)

### État du corpus YAML

#### Sanctoral universel — ✅ COMPLET

218 fichiers générés (janvier → décembre). Disponibles dans `outputs/sanctorale/`.

#### Temporal — EN COURS

| Lot                                       | État         | Fichiers                                                                       |
| ----------------------------------------- | ------------ | ------------------------------------------------------------------------------ |
| Dimanches de l'Avent (I–IV)               | ✅ Livré     | `dominica_i_adventus` … `dominica_iv_adventus`                                 |
| Noël fixe                                 | ✅ Livré     | `in_nativitate_domini`, `sanctae_dei_genetricis_mariae`, `in_epiphania_domini` |
| Cycle de Noël mobile                      | ✅ Livré     | `sancta_familiae_iesu_mariae_et_ioseph`, `in_baptismate_domini`                |
| Carême + Semaine Sainte                   | ⏳ À générer |                                                                                |
| Octave pascale + Dimanches de Pâques      | ⏳ À générer |                                                                                |
| Fêtes mobiles majeures (Trinité, Corpus…) | ⏳ À générer |                                                                                |
| Dimanches du Temps Ordinaire (X–XXXIV)    | ⏳ À générer | Ancre `tempus_ordinarium` + `ordinal` — déblocage v1.3.3                       |
| Christ-Roi + fin de cycle                 | ⏳ À générer | `domini_nostro_iesu_christi_regis_universi`                                    |

### Décisions architecturales gelées

- `transfers` interdit pour du calcul structurel — invariant permanent.
- `ordinal` exclusif à `anchor: tempus_ordinarium` — validé V4a, rejet fatal si combinaison incorrecte.
- Résolution `tempus_ordinarium` : O(1) via `DOY(adventus) − 7 × (35 − ordinal)`.
- Slots ordinaires absorbés par Pâques/Noël : `Ok(None)` à l'Étape 3, suppression silencieuse à l'Étape 4 — jamais une erreur.

---

## Philosophie de la Roadmap

**Architecture :** deux crates dans un workspace Cargo. `liturgical-calendar-core` (`no_std`, `no_alloc`) est un projecteur de mémoire O(1) — 4 fonctions FFI, zéro logique de domaine. `liturgical-calendar-forge` (`std`) est le compilateur AOT qui produit les artefacts `.kald` et `.lits` consommés par l'Engine.

**Principe d'organisation :** chaque jalon produit un binaire ou ensemble de crates testables et validables en isolation. Aucune étape intermédiaire sans critère de sortie concret.

---

## Jalon 1 — Binary Foundation

**Périmètre :** Structure binaire `.kald` v2.0 côté Engine — Header, CalendarEntry, formule d'index, fonctions de lecture.

**Critère de sortie :** `kal_validate_header` + `kal_read_entry` compilent en `#![no_std]` sans `alloc` et passent les tests unitaires de layout. `cargo tree -p liturgical-calendar-core` ne retourne aucune dépendance autre que `sha2` (dérogation INV-W1 — RustCrypto, `default-features = false`, pas de `std`/`alloc` transitif).

---

### 1.1 Types de Domaine Engine

**Fichier :** `liturgical-calendar-core/src/types.rs`

Implémenter `Precedence`, `Nature`, `Color`, `LiturgicalPeriod` conformément aux §4.1–4.4 de la spec.

- `#[repr(u8)]` sur chaque enum
- `try_from_u8(val: u8) -> Result<Self, DomainError>` pour chaque type
- `DomainError` : `Copy`, pas de `String`, types primitifs uniquement
- Traits dérivés obligatoires par enum :

| Enum               | Traits dérivés obligatoires                                | Justification                        |
| ------------------ | ---------------------------------------------------------- | ------------------------------------ |
| `Precedence`       | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Axe de tri principal, déjà dans spec |
| `Nature`           | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |
| `Color`            | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |
| `LiturgicalPeriod` | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |

Note : `PartialOrd`/`Ord` sur `Nature`, `Color`, `LiturgicalPeriod` sont dérivés automatiquement
par discriminant (`repr(u8)`) et n'ont aucune signification liturgique — ils satisfont
uniquement la contrainte de typage des collections ordonnées.

**Tests :**

- Roundtrip `try_from_u8(v as u8) == Ok(v)` pour chaque variant
- `try_from_u8(13)` → `Err` pour Precedence (valeurs 13–15 réservées)

---

### 1.2 Header v2.0

**Fichier :** `liturgical-calendar-core/src/header.rs`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic:       [u8; 4],
    pub version:     u16,
    pub variant_id:  u16,
    pub epoch:       u16,
    pub range:       u16,
    pub entry_count: u32,
    pub pool_offset: u32,
    pub pool_size:   u32,
    pub checksum:    [u8; 32],
    pub _reserved:   [u8; 8],
}

pub fn validate_header(bytes: &[u8]) -> Result<Header, HeaderError>;
```

Validations dans l'ordre :

1. `bytes.len() >= 64` → `HeaderError::FileTooSmall`
2. Magic == `b"KALD"` → `HeaderError::InvalidMagic`
3. `version == 4` → `HeaderError::UnsupportedVersion(version)`
4. `bytes.len() as u64 == 64 + entry_count as u64 * 8 + pool_size as u64` → `HeaderError::FileSizeMismatch`
5. `_reserved == [0u8; 8]` → `HeaderError::ReservedNotZero`
6. SHA-256 sur `bytes[64..]` == `checksum` → `HeaderError::ChecksumMismatch`

Désérialisation : `u16::from_le_bytes`, `u32::from_le_bytes` — LE canonique obligatoire.

**Tests :**

- `assert_eq!(size_of::<Header>(), 64)`
- `assert_eq!(align_of::<Header>(), 8)` (ou au moins 4)
- Offset de chaque champ via `offset_of!`
- Validation nominal : header construit → sérialisé → `validate_header` OK
- Chaque chemin d'erreur (magic invalide, version 3, taille incohérente, reserved non-nul, SHA erroné)

---

### 1.3 CalendarEntry v2.0

**Fichier :** `liturgical-calendar-core/src/entry.rs`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarEntry {
    pub primary_id:      u16,  // offset 0
    pub secondary_index: u16,  // offset 2
    pub flags:           u16,  // offset 4 — u16 aligné sur offset pair
    pub secondary_count: u8,   // offset 6
    pub _reserved:       u8,   // offset 7
}

impl CalendarEntry {
    /// Entrée nulle : tous les champs à zéro. `const fn` — no_alloc safe.
    pub const fn zeroed() -> Self;

    pub fn is_padding(&self) -> bool { self.primary_id == 0 }
    pub fn precedence(&self) -> Result<Precedence, DomainError>;
    pub fn color(&self)      -> Result<Color, DomainError>;
    pub fn liturgical_period(&self) -> Result<LiturgicalPeriod, DomainError>;
    pub fn nature(&self)     -> Result<Nature, DomainError>;
}

impl Default for CalendarEntry {
    fn default() -> Self { Self::zeroed() }
}
```

Extraction des champs depuis `flags` :

- `Precedence = flags & 0x000F`
- `Color = (flags >> 4) & 0x000F`
- `LiturgicalPeriod = (flags >> 8) & 0x0007`
- `Nature = (flags >> 11) & 0x0007`

**Tests :**

- `assert_eq!(size_of::<CalendarEntry>(), 8)` — stride constant, critique pour l'indexation
- `assert_eq!(offset_of!(CalendarEntry, flags), 4)` — alignement naturel u16 sur offset pair
- `assert_eq!(offset_of!(CalendarEntry, secondary_count), 6)`
- Roundtrip flags encode/decode pour toutes les combinaisons (Precedence × Nature × Color × LiturgicalPeriod)
- Padding Entry : `primary_id = 0`, `secondary_count = 0`, `is_padding() == true`
- Reserved non-nul : toléré en lecture (l'Engine ne valide pas `_reserved` au niveau entrée)

```rust
#[test]
fn zeroed_is_padding() {
    let e = CalendarEntry::zeroed();
    assert!(e.is_padding());
    assert_eq!(e.flags, 0);
    assert_eq!(e.secondary_count, 0);
    assert_eq!(e._reserved, 0);
}

#[test]
fn default_equals_zeroed() {
    assert_eq!(CalendarEntry::default(), CalendarEntry::zeroed());
}
```

---

### 1.4 Formule d'Index et `kal_read_entry`

**Fichier :** `liturgical-calendar-core/src/ffi.rs`

```rust
#[no_mangle]
pub unsafe extern "C" fn kal_read_entry(
    data: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out_entry: *mut CalendarEntry,
) -> i32 {
    // 1. Null check
    // 2. Parse header (validate_header ou lecture directe entry_count)
    // 3. Bounds : year ∈ [1969, 2399], doy ∈ [0, 365]
    // 4. idx = (year as u32 - 1969) * 366 + doy as u32
    // 5. idx < header.entry_count → KAL_ERR_INDEX_OOB
    // 6. offset = 64 + idx * 8
    // 7. Lire CalendarEntry en LE depuis data[offset..]
    // 8. *out_entry = entry ; retourner KAL_ENGINE_OK
}
```

**Tests :**

- Formule : `year=1969, doy=0` → `idx=0`, `year=2399, doy=365` → `idx = 430*366+365`
- Limites : `year=1968` → `KAL_ERR_INDEX_OOB`, `year=2400` → `KAL_ERR_INDEX_OOB`
- `doy=366` → `KAL_ERR_INDEX_OOB`
- Null `out_entry` → `KAL_ERR_NULL_PTR`
- Lecture Padding Entry `doy=59` sur une année non-bissextile → `KAL_ENGINE_OK`, `primary_id=0`

---

### 1.5 `cbindgen` — Génération `kal_engine.h`

**Fichier :** `liturgical-calendar-core/cbindgen.toml`

Générer `kal_engine.h` exposant :

- `KalHeader`, `KalCalendarEntry` (structs C)
- Constantes `KAL_ENGINE_OK`, `KAL_ERR_*`
- Prototypes `kal_validate_header`, `kal_read_entry`

Vérification : compilation d'un programme C minimal utilisant `kal_read_entry`.

**Feature `gen-headers` — déclaration obligatoire :**

`build.rs` conditionne l'invocation de cbindgen via `#[cfg(feature = "gen-headers")]`. Cette feature doit être déclarée explicitement dans `Cargo.toml`, même sans dépendance associée — Cargo 1.75+ traite `unexpected_cfg` comme erreur sous `-D warnings` :

```toml
# liturgical-calendar-core/Cargo.toml
[features]
gen-headers = []
```

cbindgen reste une `build-dependency` optionnelle, non installée par défaut. La génération de `kal_engine.h` est une étape **manuelle, hors CI standard** :

```bash
cargo build -p liturgical-calendar-core --features gen-headers
```

---

## Jalon 2 — The Compiler

**Périmètre :** Pipeline Forge complet (6 étapes), production d'un `.kald` et d'un `.lits` valides et vérifiables.

**Critère de sortie :** La Forge produit un `.kald` valide pour l'année 2025. `kal_validate_header` retourne `KAL_ENGINE_OK`. `kal_read_entry` retourne des `CalendarEntry` cohérentes pour les 366 slots de 2025 (doy 0–365), dont la Padding Entry à `doy=59` (`primary_id=0`, 2025 non-bissextile) et la fête du 28 février (`doy=58`) correctement résolue.

> 2025 n'est **pas** une année bissextile. La Padding Entry est **présente** à `doy = 59` (`primary_id = 0`). La fête du 28 février occupe `doy = 58`. Pâques 2025 : 20 avril → `doy = 110` (`MONTH_STARTS[3] + 19 = 91 + 19 = 110`). À vérifier dans le `.kald` produit.

---

### 2.0 Invariants de développement Jalon 2

Ces contraintes s'appliquent à l'implémentation de la Forge. Elles complètent les invariants
architecturaux INV-W1–W9 (spec §0.2).

**INV-FORGE-LINT — Politique lint Forge en Jalon 2**

```rust
// liturgical-calendar-forge/src/lib.rs
#![allow(missing_docs)] // Activé en Jalon 3 (INV-W7)
```

Activer `warn(missing_docs)` prématurément génère 100+ warnings masquant les erreurs réelles.

**INV-FORGE-MOVE — Ownership dans le pipeline**

Les méthodes de pipeline consomment leurs entrées par move. Dans les tests unitaires,
utiliser `.clone()` avant tout appel si la valeur est lue dans l'assertion suivante (INV-W8).

**INV-FORGE-DERIVE — Traits obligatoires sur les enums de domaine Forge**

Les enums `Nature`, `Color`, `LiturgicalPeriod` dans `registry.rs` (côté Forge) doivent dériver
`PartialOrd, Ord`. Ces enums sont distincts de leurs homologues du Core — ils peuvent
évoluer indépendamment mais doivent satisfaire les mêmes contraintes de trait
(voir §1.1 de cette roadmap, Patch 2).

**INV-FORGE-SORT — `ResolutionKey` est la seule autorité de résolution intra-slot**

Toute désignation `primary` / `secondary` dans un slot DOY passe **exclusivement** par un `sort_unstable_by_key(|f| f.resolution_key())`. Aucun `if/else` conditionnel sur `Precedence`, `Cycle` ou `Temporality` n'est autorisé dans `resolution.rs` en dehors de la garde V7 (Passe 2) et du déclassement saisonnier (§3.4 spec). Toute déviation constitue une violation architecturale — le déterminisme bit-for-bit du `.kald` en dépend.

`ResolutionKey` est définie dans `liturgical-calendar-forge/src/resolution.rs`. Elle n'appartient pas au Core — l'Engine ne trie jamais, il lit.

**INV-FORGE-ORDINAL — `ordinal` exclusif à `anchor: tempus_ordinarium`**

Le champ `ordinal` est invalide sur toute ancre autre que `tempus_ordinarium`. Le champ `offset` est invalide sur `anchor: tempus_ordinarium`. Ces deux contraintes sont vérifiées par les validations V4a (spec §10) avant toute désérialisation du bloc `history`.

---

### 2.1 Rule Parsing (Étape 1)

**Fichier :** `liturgical-calendar-forge/src/parsing.rs`

- Découverte récursive des fichiers YAML depuis `corpus_root` via `CompilationTarget` (liturgical-scheme.md §5.3)
- Tri lexicographique des fichiers par répertoire avant ingestion (INV-FORGE-1)
- **Dérivation du slug depuis le stem du nom de fichier** (`path.file_stem()`) — validation `[a-z][a-z0-9_]*` (V6) effectuée **avant** désérialisation YAML. Tout stem invalide → `ParseError::InvalidSlugSyntax(stem)`, le fichier n'est pas parsé.
- Dérivation du scope et de la region depuis le chemin — validation de cohérence path ↔ contenu (`ParseError::ScopePathMismatch`)
- Ingestion YAML → structures Rust intermédiaires (`FeastVersionDef`)
  - Champ attendu : `version: 1` (pas `format_version` — sa présence produit `MalformedYaml` par champ inconnu)
  - Champ `slug` : **absent du YAML** — fourni par `path.file_stem()` avant désérialisation
- Construction du `FeastRegistry` (BTreeMap)
- Désérialisation et validation du bloc `transfers` si présent : V-T1, V-T2, V-T3 (§8 scheme, Groupe E)
- Application des validations V1–V6 (§10 spec) — erreurs fatales
- Normalisation : `normalize_color`, `normalize_nature` (allocation `String` autorisée en Forge)

**Convention champs serde réservés :** voir §0.3 de la spec — champs YAML futurs préfixés `_` avec `#[serde(rename = "clé_yaml")]`. Le struct de désérialisation utilise `#[serde(deny_unknown_fields)]` — la présence de tout champ inconnu (ex: `title`) produit `ParseError::MalformedYaml`.

**Test :** corpus atomique minimal (1 fichier `universale/sanctorale/`, 1 fichier `nationalia/{ISO}/sanctorale/`) → slug déduit correctement du stem, `FeastRegistry` construit sans erreur, scope et region correctement déduits du chemin. Vérifier que `format_version: 1` dans un fichier YAML produit `MalformedYaml`. Vérifier que la présence d'un champ `title:` dans un bloc `history[]` produit `MalformedYaml`.

---

### 2.1bis i18n Resolution (Étape 1bis)

**Fichier :** `liturgical-calendar-forge/src/i18n.rs`

Corrélation entre le `FeastRegistry` et les dictionnaires `i18n/`. Produit le `LabelTable` consommé par l'Étape 6 pour générer les `.lits`.

**Structure `DictStore` :**

```rust
/// Table des dictionnaires chargés. Clé : (lang, slug, from) → label.
/// BTreeMap pour garantir l'ordre déterministe cross-build (INV-FORGE-2).
pub(crate) struct DictStore {
    entries: BTreeMap<(String, String, u16), BTreeMap<String, String>>,
    // (lang,    slug,   from)             → { field → value }
}
```

**Algorithme :**

```
1. Découvrir i18n/{lang}/{slug}.yaml pour toutes les langues présentes.
   Tri lexicographique sur (lang, slug) — ordre déterministe.

2. Pour chaque fichier i18n/{lang}/{slug}.yaml :
   Désérialiser : BTreeMap<u16 (from), BTreeMap<String (field), String (value)>>
   Pour chaque (from, fields) :
     SI from ∉ history_froms(slug) → ParseError::I18nOrphanKey  [V-I2]
     Insérer dans DictStore[(lang, slug, from)]

3. Pour chaque (slug, from) dans le FeastRegistry :
   SI DictStore[("la", slug, from)]["title"] absent → ParseError::I18nMissingLatinKey  [V-I1]

4. Construire LabelTable par fusion AOT :
   Pour chaque (slug, from), pour chaque lang compilée :
     value = DictStore[(lang, slug, from)]["title"]
             ?? DictStore[("la", slug, from)]["title"]   // fallback latin garanti par V-I1
     Insérer dans LabelTable[(feast_id, from)] → value
```

**Sortie :** `LabelTable` — `BTreeMap<(FeastID, u16 from, u16 to, String lang), String>`. `to` est copié depuis le bloc `history[]` correspondant pour permettre la recherche `(FeastID, year)` dans `LitsProvider::get`.

**Tests :**

- V-I1 : corpus avec `i18n/la/` absent ou clé `from` manquante → `ParseError::I18nMissingLatinKey`, compilation interrompue.
- V-I2 : dictionnaire `i18n/fr/foo.yaml` avec `from: 2030` alors que `history[]` de `foo` ne contient pas `from: 2030` → `ParseError::I18nOrphanKey`.
- Fallback AOT : dictionnaire `fr` sans clé `from: 2011` → le `LabelTable` contient la valeur latine pour `(feast_id, 2011, lang="fr")`.
- Déterminisme : deux exécutions avec le même corpus → `LabelTable` identique octet par octet.

---

### 2.2 Canonicalization (Étape 3)

**Fichier :** `liturgical-calendar-forge/src/canonicalization.rs`

- `compute_easter(year: i32) -> u16` (DOY 0-based) — algorithme Meeus/Jones/Butcher
- `is_leap_year(year: i32) -> bool`
- `SeasonBoundaries::compute(year: i32) -> SeasonBoundaries` (DOY 0-based)
- `doy_from_month_day(month: u8, day: u8) -> u16` via table `MONTH_STARTS`
- Résolution des ancres dans l'ordre v2.2 : `nativitas` → `epiphania` → `adventus` → `tempus_ordinarium` → `pascha` → `pentecostes`
- `resolve_tempus_ordinarium(adventus_doy: u16, ordinal: u8) -> u16` — O(1), dépend uniquement de `adventus_doy`

**Tests :**

- Pâques 2025 : attendu `doy = 110` (20 avril → `MONTH_STARTS[3] + 19 = 91 + 19 = 110`)
- Pâques 2000 : 23 avril → `doy = 113`
- Année bissextile 2024 : `is_leap_year(2024) == true`, `is_leap_year(2025) == false`
- `MONTH_STARTS[0] == 0`, `MONTH_STARTS[2] == 60` (mars après padding)
- `tempus_ordinarium` ordinal 34, Avent 2025 (DOY 333) : `333 - 7*(35-34) = 326` ✅
- `tempus_ordinarium` ordinal 1, Avent 2025 (DOY 333) : `333 - 7*34 = 95` → absorbé (Ok(None)) ✅
- V4a : `anchor: tempus_ordinarium` + `offset` présent → `ParseError::OffsetOnOrdinalAnchor`
- V4a : `anchor: tempus_ordinarium` + `ordinal` absent → `ParseError::MissingOrdinal`
- V4a : `anchor: pascha` + `ordinal` présent → `ParseError::OrdinalOnNonOrdinalAnchor`

---

### 2.3 Conflict Resolution (Étape 4)

**Fichier :** `liturgical-calendar-forge/src/resolution.rs`

**Types à définir dans ce fichier :**

```rust
/// Clé de tri canonique — voir spec §3.0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResolutionKey<'a> {
    pub precedence:  u8,
    pub cycle:       Cycle,
    pub temporality: Temporality,
    pub slug:        &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cycle       { Temporal = 0, Sanctoral = 1 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Temporality { Fixed = 0, Mobile = 1 }
```

**Pipeline (5 passes — spec §3.3) :**

- **Passe 1** : Placement de toutes les fêtes (fixes et mobiles) dans `BTreeMap<u16 (doy), Vec<PlacedFeast>>`. Aucun conflit résolu. Les slots `tempus_ordinarium` absorbés produisent `Ok(None)` — pas d'entrée dans la table.
- **Passe 2** : Garde V7 + résolution de scope. Pour chaque slot multi-fêtes : appliquer la hiérarchie `diocesan > national > universal` (§3.1) puis détecter `SolemnityCollision` (Precedence ≤ 3 tout scope, Precedence ∈ [4,5] même scope) — fatal avant tout tri.
- **Passe 3** : Tri Canonique + Élection + Déclassement (§3.4 spec) + Dispatch Transferts. Appliquer `sort_unstable_by_key(|f| f.resolution_key())` sur chaque slot. `slot[0]` = primary. Partition de `slot[1..]` en secondary_feasts, to_transfer, suppressed. Consulter bloc `transfers` pour chaque fête to_transfer.
- **Passe 4** : `TransferQueue` — BTreeSet déterministe, transferts strictement vers l'avant, profondeur bornée à 7.
- **Passe 5** : Vérification de clôture — `ResolutionIncomplete` si invariant violé.

Sortie : `ResolvedCalendar` — table indexée `(year, doy) → ResolvedDay { primary, secondaries: Vec<FeastId> }`.

**Tests :**

- `ResolutionKey` : `sort_unstable_by_key` sur un slot de 3 fêtes → vérifier que `slot[0]` est la Solennité (Precedence=1), `slot[1]` la Fête (Precedence=8), `slot[2]` la Mémoire (Precedence=11).
- Tiebreaker `slug` : deux fêtes de Precedence, Cycle et Temporality identiques → la fête dont le slug est lexicographiquement inférieur devient primary.
- Passe 2 — V7 : deux Solennités de même scope (Precedence=1) sur le même DOY → `ForgeError::SolemnityCollision` avant tout tri.
- Passe 3 — `transfers` avec `offset` : fête déplacée atterrit au bon DOY calculé.
- Passe 3 — `transfers` avec `date` : fête déplacée atterrit à la date fixe.
- Passe 4 — profondeur : chaîne de 8 jours consécutifs bloquants → `ForgeError::TransferFailed`.
- Absorption `tempus_ordinarium` : ordinal 1, année 2025 → slot non inséré dans la table (Ok(None)), aucun transfert tenté.

---

### 2.4 Day Materialization (Étape 5)

**Fichier :** `liturgical-calendar-forge/src/materialization.rs`

- Itération sur la plage (pour Jalon 2 : uniquement 2025)
- Génération des 366 slots, dont la Padding Entry à `doy=59` (2025 non-bissextile)
- Construction du Secondary Pool : collection ordonnée de FeastIDs, attribution des `secondary_index`

**Test :** 2025 → 366 entrées dont exactement 1 Padding Entry (`doy=59`, `primary_id=0`). Vérifier que `PoolBuilder::insert` déclenche `ForgeError::SecondaryPoolOverflow` (V11) si le pool dépasse 65 535 entrées — injecter un corpus synthétique saturé pour couvrir ce chemin.

---

### 2.5 Binary Packing (Étape 6)

**Fichier :** `liturgical-calendar-forge/src/packing.rs`

**Production `.kald` :**

```rust
pub fn encode_flags(p: Precedence, c: Color, lp: LiturgicalPeriod, n: Nature) -> u16 {
    (p as u16) | ((c as u16) << 4) | ((lp as u16) << 8) | ((n as u16) << 11)
}
```

- Sérialisation LE de chaque `CalendarEntry` (8 octets)
- Calcul SHA-256 sur `[Data Body ∥ Secondary Pool]` via `sha2` crate (Forge uniquement — `std`)
- Construction du `Header` v2.0 (64 octets)
- Écriture séquentielle : Header + Data Body + Secondary Pool
- Validation post-écriture : relecture du fichier produit via `kal_validate_header`

**Production `.lits` (une par langue compilée) :**

**Fichier :** `liturgical-calendar-forge/src/lits_writer.rs`

```rust
pub fn write_lits(
    label_table: &LabelTable,
    lang: &str,
    kald_checksum: &[u8; 32],
    output: &Path,
) -> Result<(), ForgeError>;
```

- Trier les entrées par `(feast_id ASC, from ASC)` — ordre déterministe (INV-FORGE-2)
- Construire le String Pool UTF-8 : chaînes zéro-terminées concaténées
- Écrire Header 32 octets : magic `b"LITS"`, `version=1`, `lang` (6 octets zéro-padded), `kald_build_id = kald_checksum[..8]`, `entry_count`, `pool_offset`, `pool_size`
- Écrire Entry Table : `(feast_id u16, from u16, to u16, str_offset u32)` × `entry_count`
- Écrire String Pool
- Le `.kald` doit être finalisé **avant** l'appel — `kald_checksum` est son SHA-256 calculé en §2.5 production `.kald`

**Test de conformité Jalon 2 :**

```rust
use liturgical_calendar_core::entry::CalendarEntry;
use liturgical_calendar_core::ffi::{kal_read_entry, kal_validate_header, KAL_ENGINE_OK};
use liturgical_calendar_forge::forge_year;
use std::ptr::null_mut;

#[test]
fn conformity_2025() {
    let (kald, lits_la) = forge_year(2025, &["la"]).expect("forge_year(2025) doit réussir");

    // Validation structurelle .kald : header + SHA-256
    let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), null_mut()) };
    assert_eq!(rc, KAL_ENGINE_OK);

    // Pâques 2025 : 20 avril = MONTH_STARTS[3] + 19 = 91 + 19 = doy 110
    let mut e = CalendarEntry::zeroed();
    let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 110, &mut e) };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(e.primary_id, 0, "doy=110 doit contenir Pâques");

    // Padding Entry : 2025 non-bissextile → doy=59 vide
    let mut e2 = CalendarEntry::zeroed();
    let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 59, &mut e2) };
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_eq!(e2.primary_id, 0, "doy=59 doit être Padding Entry pour 2025");

    // Cohérence .lits : kald_build_id = kald_checksum[..8]
    let kald_checksum = &kald[12..20]; // octets 12–19 du header .kald = SHA-256[..8] (ajustement offset réel)
    let lits_build_id = &lits_la[12..20]; // octets 12–19 du header .lits
    assert_eq!(kald_checksum, lits_build_id, "kald_build_id doit correspondre");

    // Label Pâques via LitsProvider
    let provider = LitsProvider::new(&lits_la).expect("header .lits valide");
    let label = provider.get(e.primary_id, 2025);
    assert!(label.is_some(), "Pâques doit avoir un label latin pour 2025");
}
```

---

## Jalon 3 — Sanctification

**Périmètre :** Couverture complète 1969–2399 (431 ans), CI cross-platform 4 cibles, fuzzing du reader, API complète.

**Critère de sortie :** `cargo test --release` vert sur les 4 cibles CI. Zéro panic sur 10 000 inputs fuzzés sur `kal_validate_header` et `kal_read_entry`. SHA-256 identique sur toutes les cibles.

---

### 3.1 Couverture Complète 1969–2399

**Fichier :** `tests/full_range.rs`

- Forge génère un `.kald` couvrant les 431 ans en une seule passe
- `kal_validate_header` valide le fichier produit
- Vérification Padding Entries : années non-bissextiles → `kal_read_entry(year, 59).primary_id == 0`
- Vérification années bissextiles : `kal_read_entry(year, 59).primary_id != 0` (slot réel, pas padding)
- Déterminisme : deux exécutions Forge → SHA-256 identique bit-for-bit
- Vérification corpus Temporal complet : tous les dimanches du Temps Ordinaire (ordinals 10–34) présents pour chaque année où non absorbés

**Années bissextiles dans la plage 1969–2399 :**
Toutes les années divisibles par 4 sauf séculaires non-centenaires (2100, 2200, 2300 sont non-bissextiles). 2000 et 2400 sont bissextiles (mais 2400 hors plage). À valider dans les tests.

**Test de détection rapide :**

```rust
for year in 1969u16..=2399 {
    let is_leap = is_leap_year(year as i32);
    let mut e = CalendarEntry::zeroed();
    kal_read_entry(data, len, year, 59, &mut e);
    if is_leap {
        assert_ne!(e.primary_id, 0, "year {year}: doy=59 devrait être réel");
    } else {
        assert_eq!(e.primary_id, 0, "year {year}: doy=59 devrait être Padding");
    }
}
```

---

### 3.2 CI Matrix — 4 Cibles

**Fichier :** `.github/workflows/ci.yml`

```yaml
strategy:
  matrix:
    target:
      - x86_64-unknown-linux-gnu
      - aarch64-unknown-linux-gnu
      - x86_64-apple-darwin
      - wasm32-unknown-unknown
```

Étapes par cible :

1. `cargo build --target $TARGET -p liturgical-calendar-core` — compile sans erreur
2. `cargo test --release --target $TARGET -p liturgical-calendar-core` (sauf wasm : `wasm-pack test`)
3. Vérification SHA-256 cross-platform : le `.kald` produit par la Forge sur `x86_64-linux` est validé par l'Engine compilé pour `aarch64-linux`

**Critère :** aucun `from_ne_bytes` / `to_ne_bytes` résiduel dans `liturgical-calendar-core`. Politique LE canonique exclusive.

---

### 3.3 Fuzzing Reader

**Outil :** `cargo-fuzz` + `libfuzzer`

**Cibles de fuzzing :**

```rust
// fuzz/fuzz_targets/validate_header.rs
fuzz_target!(|data: &[u8]| {
    let _ = kal_validate_header(data.as_ptr(), data.len(), null_mut());
    // Invariant : ne doit jamais paniquer
});

// fuzz/fuzz_targets/read_entry.rs
fuzz_target!(|data: &[u8]| {
    if data.len() < 4 { return; }
    let year = u16::from_le_bytes([data[0], data[1]]);
    let doy  = u16::from_le_bytes([data[2], data[3]]);
    let mut entry = CalendarEntry::zeroed();
    let _ = kal_read_entry(data[4..].as_ptr(), data[4..].len(), year, doy, &mut entry);
});
```

**Critère :** zéro panic sur 10 000 inputs fuzzés. Couverture des chemins d'erreur :

- Magic invalide, version invalide
- `file_size` incohérent
- SHA-256 corrompu
- Index OOB (`year` hors `[1969, 2399]`, `doy >= 366`)
- `_reserved` non-nuls
- Buffer trop court (< 64 octets)

---

### 3.4 `kal_read_secondary`

**Fichier :** `liturgical-calendar-core/src/ffi.rs`

Implémentation complète conforme §7.3 spec.

**Tests :**

- Entrée avec `secondary_count = 2` : vérifier que 2 FeastIDs sont lus correctement depuis le pool
- `secondary_count = 0` → `KAL_ENGINE_OK`, aucun accès pool
- `secondary_index + secondary_count > pool_size / 2` → `KAL_ERR_POOL_OOB`
- `out_capacity < secondary_count` → `KAL_ERR_BUF_TOO_SMALL`
- Null `out_ids` → `KAL_ERR_NULL_PTR`

---

### 3.5 `kal_scan_flags`

**Fichier :** `liturgical-calendar-core/src/ffi.rs`

Implémentation complète conforme §7.4 spec.

**Tests :**

- `flag_mask = 0x000F, flag_value = 4` → toutes les Solennités (Precedence=4) de l'année 2025
- `flag_mask = 0x000F, flag_value = 0` → uniquement le Triduum (Precedence=0) : doit retourner exactement 3 entrées (Jeudi, Vendredi, Samedi Saints)
- Résultat trié croissant par `idx`
- `out_capacity` insuffisant → `KAL_ERR_BUF_TOO_SMALL`

**Vérification SIMD-readiness :** Compiler avec `RUSTFLAGS="-C target-cpu=native"` et inspecter l'asm généré — confirmer vectorisation AVX2 sur le scan u16 à stride 8.

---

### 3.6 Valgrind et Sanitizers

**Sur `x86_64-unknown-linux-gnu` uniquement :**

```bash
cargo test --release 2>&1 | valgrind --error-exitcode=1 -- target/release/deps/liturgical_calendar_core-*
```

Critères :

- Zéro memory leak
- Zéro accès invalide (out-of-bounds read/write)
- Zéro use-after-free

Optionnel : `cargo test` avec `RUSTFLAGS="-Z sanitizer=address"` (nightly).

---

## Metrics de Qualité

| Dimension                                               | Cible                                                    |
| ------------------------------------------------------- | -------------------------------------------------------- |
| Coverage `cargo-tarpaulin` (`liturgical-calendar-core`) | ≥ 90%                                                    |
| Clippy warnings                                         | 0                                                        |
| Lignes `unsafe` dans l'Engine                           | < 50 (toutes justifiées avec bloc `SAFETY`)              |
| API publique documentée                                 | 100%                                                     |
| Dépendances externes `liturgical-calendar-core`         | `sha2` uniquement (dérogation INV-W1) — zéro autre crate |
| Build Time Forge (431 ans)                              | < 30s                                                    |
| Latence `kal_read_entry`                                | < 100ns                                                  |
| Latence `kal_scan_flags` (431 ans, O(N))                | < 10ms                                                   |
| Panics sur fuzzing                                      | 0 / 10 000 inputs                                        |
| SHA-256 déterminisme                                    | Cross-platform (4 cibles CI)                             |
| Valgrind (Linux x86_64)                                 | Clean                                                    |

---

## Extensions Futures (v2.x)

**v2.5 — Compression**  
Flag `compression` dans le Header (bits libres de `variant_id`). Support ZSTD optionnel du Data Body. L'Engine décompresse à la volée si le flag est activé — la contrainte `no_alloc` impose un décompresseur streaming sans buffer intermédiaire alloué.

**v2.6 — Rites Extraordinaires**  
`variant_id = 1` : Forme extraordinaire (Missale Romanum 1962). Forge dédiée avec règles pré-1969. L'Engine est agnostique au rite — il lit le même format `.kald`.

**v2.7 — Calendriers Orientaux**  
`variant_id = 2` : Calendrier Julien / Pâques orthodoxe. Algorithme Pâques julien dans la Forge dédiée.

**v2.8 — API REST**  
Serveur HTTP léger wrappant les 4 fonctions FFI de l'Engine. Endpoints : `GET /day/{year}/{doy}`, `GET /scan?mask=&value=`.

---

**Fin de la Roadmap v2.2**

_Architecture AOT-Only. Engine : 4 fonctions FFI, `no_std`/`no_alloc`, O(1). Forge : compilateur AOT, pipeline 6 étapes, `.kald` + `.lits`. Plage 1969–2399. Référence : `specification.md` v2.2.1, `liturgical-scheme.md`._
