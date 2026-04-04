# Roadmap de Développement : Liturgical Calendar v2.0

**Version** : 2.0  
**Date de Révision** : 2026-03-08  
**Méthodologie** : 3 jalons, chacun produisant un livrable binaire validable indépendamment  
**Critères de Succès** : Conformité binaire Forge↔Engine · SHA-256 cross-platform · Fuzzing · CI 4 cibles

---

## Philosophie de la Roadmap

**Architecture :** deux crates dans un workspace Cargo. `liturgical-calendar-core` (`no_std`, `no_alloc`) est un projecteur de mémoire O(1) — 4 fonctions FFI, zéro logique de domaine. `liturgical-calendar-forge` (`std`) est le compilateur AOT qui produit les artefacts `.kald` consommés par l'Engine.

**Principe d'organisation :** chaque jalon produit un binaire ou ensemble de crates testables et validables en isolation. Aucune étape intermédiaire sans critère de sortie concret.

---

## Jalon 1 — Binary Foundation

**Périmètre :** Structure binaire `.kald` v2.0 côté Engine — Header, CalendarEntry, formule d'index, fonctions de lecture.

**Critère de sortie :** `kal_validate_header` + `kal_read_entry` compilent en `#![no_std]` sans `alloc` et passent les tests unitaires de layout. `cargo tree -p liturgical-calendar-core` ne retourne aucune dépendance autre que `sha2` (dérogation INV-W1 — RustCrypto, `default-features = false`, pas de `std`/`alloc` transitif).

---

### 1.1 Types de Domaine Engine

**Fichier :** `liturgical-calendar-core/src/types.rs`

Implémenter `Precedence`, `Nature`, `Color`, `Season` conformément aux §4.1–4.4 de la spec.

- `#[repr(u8)]` sur chaque enum
- `try_from_u8(val: u8) -> Result<Self, DomainError>` pour chaque type
- `DomainError` : `Copy`, pas de `String`, types primitifs uniquement

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
    pub fn precedence(&self) -> Result<Precedence, DomainError>;
    pub fn color(&self)      -> Result<Color, DomainError>;
    pub fn season(&self)     -> Result<Season, DomainError>;
    pub fn nature(&self)     -> Result<Nature, DomainError>;
    pub fn is_padding(&self) -> bool { self.primary_id == 0 }
}
```

Extraction des champs depuis `flags` :

- `Precedence = flags & 0x000F`
- `Color = (flags >> 4) & 0x000F`
- `Season = (flags >> 8) & 0x0007`
- `Nature = (flags >> 11) & 0x0007`

**Tests :**

- `assert_eq!(size_of::<CalendarEntry>(), 8)` — stride constant, critique pour l'indexation
- `assert_eq!(offset_of!(CalendarEntry, flags), 4)` — alignement naturel u16 sur offset pair
- `assert_eq!(offset_of!(CalendarEntry, secondary_count), 6)`
- Roundtrip flags encode/decode pour toutes les combinaisons (Precedence × Nature × Color × Season)
- Padding Entry : `primary_id = 0`, `secondary_count = 0`, `is_padding() == true`
- Reserved non-nul : toléré en lecture (l'Engine ne valide pas `_reserved` au niveau entrée)

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

---

## Jalon 2 — The Compiler

**Périmètre :** Pipeline Forge complet (5 étapes), production d'un `.kald` valide et vérifiable.

**Critère de sortie :** La Forge produit un `.kald` valide pour l'année 2025. `kal_validate_header` retourne `KAL_ENGINE_OK`. `kal_read_entry` retourne des `CalendarEntry` cohérentes pour les 366 slots de 2025 (doy 0–365), dont la Padding Entry à `doy=59` (`primary_id=0`, 2025 non-bissextile) et la fête du 28 février (`doy=58`) correctement résolue.

> 2025 n'est **pas** une année bissextile. La Padding Entry est **présente** à `doy = 59` (`primary_id = 0`). La fête du 28 février occupe `doy = 58`. Pâques 2025 : 20 avril → `doy = 110` (`MONTH_STARTS[3] + 19 = 91 + 19 = 110`). À vérifier dans le `.kald` produit.

---

### 2.1 Rule Parsing (Étape 1)

**Fichier :** `liturgical-calendar-forge/src/parsing.rs`

- Ingestion YAML → structures Rust intermédiaires (`FeastVersionDef`)
- Construction du `FeastRegistry` (BTreeMap)
- Application des validations V1–V6 (§10 spec) — erreurs fatales
- Normalisation : `normalize_color`, `normalize_nature` (allocation `String` autorisée en Forge)

**Test :** YAML minimal (1 fête universelle, 1 fête nationale) → `FeastRegistry` construit sans erreur.

---

### 2.2 Canonicalization (Étape 2)

**Fichier :** `liturgical-calendar-forge/src/canonicalization.rs`

- `compute_easter(year: i32) -> u16` (DOY 0-based) — algorithme Meeus/Jones/Butcher
- `is_leap_year(year: i32) -> bool`
- `SeasonBoundaries::compute(year: i32) -> SeasonBoundaries` (DOY 0-based)
- `doy_from_month_day(month: u8, day: u8) -> u16` via table `MONTH_STARTS`
- Résolution des dates flottantes (Ascension = Pâques + 39, Pentecôte = Pâques + 49, etc.)

**Tests :**

- Pâques 2025 : attendu `doy = 110` (20 avril → `MONTH_STARTS[3] + 19 = 91 + 19 = 110`)
- Pâques 2000 : 23 avril → `doy = 113`
- Année bissextile 2024 : `is_leap_year(2024) == true`, `is_leap_year(2025) == false`
- `MONTH_STARTS[0] == 0`, `MONTH_STARTS[2] == 60` (mars après padding)

---

### 2.3 Conflict Resolution (Étape 3)

**Fichier :** `liturgical-calendar-forge/src/resolution.rs`

- Résolution des collisions par comparaison de `Precedence` (valeur entière, numérique inverse)
- Calcul des transferts (fête tombant un dimanche → règles de déplacement du NALC 1969)
- Sortie : `ResolvedCalendar` — table indexée `(year, doy) → ResolvedDay { primary, secondaries: Vec<FeastId> }`

**Règle de collision :** si deux fêtes occupent le même slot, celle de `Precedence` valeur numérique inférieure reste en `primary`. L'autre est soit transférée (si `Precedence ≤ 9` et `Nature ≠ Feria` — voir spec §3.4), soit commémorée (`secondary_feasts`, si `Precedence ∈ [8, 12]`), soit supprimée, selon les règles de résolution spécifiées en §3.2–§3.5 de la spec.

**Test :** collision Solennité (Precedence=4) vs Mémoire (Precedence=11) → la Solennité reste en `primary`, la Mémoire en `secondary` si applicable.

---

### 2.4 Day Materialization (Étape 4)

**Fichier :** `liturgical-calendar-forge/src/materialization.rs`

- Itération sur la plage (pour Jalon 2 : uniquement 2025)
- Génération des 366 slots, dont la Padding Entry à `doy=59` (2025 non-bissextile)
- Construction du Secondary Pool : collection ordonnée de FeastIDs, attribution des `secondary_index`

**Test :** 2025 → 366 entrées dont exactement 1 Padding Entry (`doy=59`, `primary_id=0`). Vérifier que `PoolBuilder::insert` déclenche `ForgeError::SecondaryPoolOverflow` (V11) si le pool dépasse 65 535 entrées — injecter un corpus synthétique saturé pour couvrir ce chemin.

---

### 2.5 Binary Packing (Étape 5)

**Fichier :** `liturgical-calendar-forge/src/packing.rs`

```rust
pub fn encode_flags(p: Precedence, c: Color, s: Season, n: Nature) -> u16 {
    (p as u16) | ((c as u16) << 4) | ((s as u16) << 8) | ((n as u16) << 11)
}
```

- Sérialisation LE de chaque `CalendarEntry` (8 octets)
- Calcul SHA-256 sur `[Data Body ∥ Secondary Pool]` via `sha2` crate (Forge uniquement — `std`)
- Construction du `Header` v2.0 (64 octets)
- Écriture séquentielle : Header + Data Body + Secondary Pool
- Validation post-écriture : relecture du fichier produit via `kal_validate_header`

**Test de conformité Jalon 2 :**

```rust
#[test]
fn conformity_2025() {
    let kald = forge_year(2025); // produit le .kald pour 2025 uniquement
    assert_eq!(kal_validate_header(&kald, kald.len(), null_mut()), KAL_ENGINE_OK);
    // Vérifier Pâques 2025
    let mut entry = CalendarEntry::zeroed();
    let rc = kal_read_entry(&kald, kald.len(), 2025, 110, &mut entry); // doy Pâques
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_ne!(entry.primary_id, 0); // non-padding
    // Vérifier Padding Entry
    let rc = kal_read_entry(&kald, kald.len(), 2025, 59, &mut entry);
    assert_eq!(rc, KAL_ENGINE_OK);
    assert_eq!(entry.primary_id, 0); // Padding
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

**Fin de la Roadmap v2.0 — Ready for Implementation**

_Révisée le 2026-03-08. Trois jalons : Binary Foundation, The Compiler, Sanctification. Engine (`liturgical-calendar-core`) : 4 fonctions FFI, `no_std`/`no_alloc`, projecteur de mémoire O(1). Forge (`liturgical-calendar-forge`) : compilateur AOT, pipeline en 5 étapes, logique liturgique complète. Format binaire `.kald` v2.0 : Header 64 octets, `CalendarEntry` 8 octets, Secondary Pool. Convention DOY 0-based. Plage 1969–2399 (431 ans). Référence : `specification.md` v2.0._
