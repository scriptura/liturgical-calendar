# liturgical-calendar-wasm

Bridge WASM pour [`liturgical-calendar-core`](../liturgical-calendar-core). Expose l'API C native du Core au JavaScript via `WebAssembly`, sans `wasm-bindgen`, sans allocateur heap, sans framework.

---

## Présentation

Ce crate est la couche de liaison entre le runtime liturgique et le navigateur. Il compile en un binaire `.wasm` autonome que le JS charge et pilote directement via l'API `WebAssembly` standard.

**Contraintes architecturales :**

- Cible `wasm32-unknown-unknown`, type `cdylib`
- `no_std` — aucune dépendance à la bibliothèque standard
- Zéro allocation heap — buffers statiques BSS, zéro-init
- Zéro copie pour la résolution de labels (pattern out-static)
- Pas de glue générée — les exports WASM sont des fonctions C directes

**Ce crate ne fait pas partie du build workspace par défaut.** Il est exclu de `default-members` dans le `Cargo.toml` racine et doit être compilé explicitement avec une cible wasm32.

---

## Prérequis

### Toolchain Rust

```sh
rustup target add wasm32-unknown-unknown
```

Vérifier que la cible est installée :

```sh
rustup target list --installed | grep wasm32
# wasm32-unknown-unknown
```

### Artefacts binaires

Le bridge nécessite deux fichiers produits par `liturgical-calendar-forge` :

| Fichier         | Rôle                                                  |
| --------------- | ----------------------------------------------------- |
| `calendar.kald` | Table calendaire compilée (entrées + pool secondaire) |
| `calendar.lits` | Labels i18n UTF-8 (liés au `.kald` par `build_id`)    |

Ces fichiers doivent être placés dans `www/` avant de lancer le serveur.

---

## Configuration workspace

Le fichier `.cargo/config.toml` à la racine du workspace définit les alias de build et de publication pour ce crate :

```toml
# .cargo/config.toml  (racine du workspace)
[alias]
build-wasm   = "build -p liturgical-calendar-wasm --target wasm32-unknown-unknown --profile wasm-release"
publish-wasm = "publish -p liturgical-calendar-wasm --target wasm32-unknown-unknown"
```

Le profil `wasm-release` est déclaré dans le `Cargo.toml` racine :

```toml
[profile.wasm-release]
inherits      = "release"
opt-level     = "z"
lto           = true
codegen-units = 1
strip         = true
```

---

## Build

### Profil de développement

```sh
cargo build -p liturgical-calendar-wasm --target wasm32-unknown-unknown
```

Sortie : `target/wasm32-unknown-unknown/debug/liturgical_calendar_wasm.wasm`

### Profil de production (optimisé taille)

```sh
cargo build-wasm
```

Sortie : `target/wasm32-unknown-unknown/wasm-release/liturgical_calendar_wasm.wasm`

### Déploiement dans `www/`

```sh
cp target/wasm32-unknown-unknown/wasm-release/liturgical_calendar_wasm.wasm www/
```

### Publication sur crates.io

La vérification de `cargo publish` compile le crate — la cible doit être spécifiée explicitement, sans quoi le build host échoue (`no_std` sans panic handler natif).

```sh
cargo publish-wasm
```

---

## Lancement

Le serveur de développement `www/server.py` gère la réécriture SPA (toute requête vers un chemin non-fichier sert `index.html`) et silencieux sur les ressources statiques.

```sh
python3 www/server.py          # port 8080 par défaut
python3 www/server.py 9000     # port personnalisé
```

### Routes supportées

| URL                               | Vue                                             |
| --------------------------------- | ----------------------------------------------- |
| `http://0.0.0.0:8080/`            | Jour courant                                    |
| `http://0.0.0.0:8080/2026`        | Tableau annuel — 366 jours                      |
| `http://0.0.0.0:8080/2026/12/25`  | Détail journalier — tous les champs             |
| `http://0.0.0.0:8080/#2026`       | Idem année — compatible tout hébergeur statique |
| `http://0.0.0.0:8080/#2026/12/25` | Idem jour — compatible tout hébergeur statique  |

Le routage par hash (`#`) ne requiert aucune configuration serveur et fonctionne sur tout hébergeur de fichiers statiques (GitHub Pages, Netlify, S3…). Le routage par chemin requiert la réécriture SPA de `server.py` en développement, ou un fichier `_redirects` / `vercel.json` en production.

> `python3 -m http.server` ne suffit pas : il ne gère pas la réécriture SPA et bloque `fetch()` depuis `file://`.

---

## Layout binaire `.kald` — convention doy

La Forge réserve **366 créneaux fixes par année**, quelle que soit la bissextilité. Le slot 59 est toujours celui du 29 février :

- **Année non bissextile** : Padding Entry pur (`primary_index = 0`, tous les champs nuls).
- **Année bissextile** : entrée réelle si une fête tombe ce jour, sinon Padding Entry portant les primitives temporelles (`liturgical_period`, `liturgical_week`).

Tous les jours à partir du 1er mars ont donc un `doy` constant d'une année à l'autre.

| Date        | doy |
| ----------- | --: |
| 1er janvier |   0 |
| 28 février  |  58 |
| 29 février  |  59 |
| 1er mars    |  60 |
| 25 décembre | 359 |

---

## API WASM exportée

Toutes les fonctions suivent la convention d'appel C et sont appelées via `instance.exports.*`. Le bridge impose un protocole séquentiel strict — tout appel de lecture depuis un état incorrect retourne `KAL_ERR_NOT_READY` (-20).

### État du bridge

```
Uninitialized
    │  kal_wasm_alloc_kald(len) → ptr
    │  [JS écrit dans WebAssembly.Memory]
    │  kal_wasm_commit_kald()   → 0 si OK
    ▼
KaldLoaded
    │  kal_wasm_alloc_lits(len) → ptr
    │  [JS écrit dans WebAssembly.Memory]
    │  kal_wasm_commit_lits()   → 0 si OK
    ▼
Ready  ←  toutes les fonctions de lecture disponibles
```

### Allocation (phase 1)

```c
// Retourne le pointeur WASM destination pour le buffer .kald.
// Retourne 0 si len > KALD_CAP (2 MB).
uint32_t kal_wasm_alloc_kald(uint32_t len);

// Retourne le pointeur WASM destination pour le buffer .lits.
// Retourne 0 si len > LITS_CAP (512 KB).
uint32_t kal_wasm_alloc_lits(uint32_t len);
```

### Commit (phase 2)

```c
// Valide le header .kald v6 (magic, version, discriminant de layout, SHA-256).
// Retourne 0 (OK) ou un code d'erreur Core négatif.
int32_t kal_wasm_commit_kald(void);

// Vérifie build_id (kald_checksum[..8] == lits_header[12..20])
// et valide le header .lits.
// Retourne 0 (OK) ou un code d'erreur négatif.
int32_t kal_wasm_commit_lits(void);
```

### Lecture d'entrée — pattern out-static (ENTRY_BUF)

```c
// Lit la TimelineEntry (8 octets) pour (year, doy) dans ENTRY_BUF interne.
// doy : 0-based, layout fixe 366 slots/an (voir convention doy ci-dessus).
// Retourne 0 (OK) ou un code d'erreur négatif.
int32_t  kal_wasm_read_day(uint16_t year, uint16_t doy);

// Pointeur WASM vers ENTRY_BUF (8 octets, little-endian).
// Valide après kal_wasm_read_day.
uint32_t kal_wasm_entry_ptr(void);
```

Layout de `ENTRY_BUF` (miroir de `TimelineEntry` v6) :

| Offset | Champ              | Type   | Décodage                                       |
| -----: | ------------------ | ------ | ---------------------------------------------- |
|    0–1 | `primary_index`    | u16 LE | 0 = Padding Entry (aucune fête propre)         |
|    2–3 | `secondary_offset` | u16 LE | Offset dans le Secondary Pool                  |
|      4 | `occurrence_flags` | u8     | Voir décodage ci-dessous                       |
|      5 | `secondary_count`  | u8     | Nombre de fêtes secondaires                    |
|      6 | `liturgical_week`  | u8     | 0 = N/A ; 1–34 = ordinal de semaine liturgique |
|      7 | `_reserved`        | u8     | Ignoré                                         |

Décodage de `occurrence_flags` :

| Bits  | Champ              | Valeurs                                         |
| ----- | ------------------ | ----------------------------------------------- |
| [0]   | `has_vesperae_i`   | 1 = Premières Vêpres ce soir (pour DOY+1)       |
| [1]   | `has_vigilia`      | 1 = Messe de Vigile propre ce soir (pour DOY+1) |
| [4:2] | `LiturgicalPeriod` | 0–6 (voir LUT ci-dessous)                       |
| [7:5] | réservés           | nuls                                            |

`LiturgicalPeriod` est valide pour **tous** les slots, y compris les Padding Entries — sauf DOY 59 non-bissextile (zéro absolu).

LUT `LiturgicalPeriod` :

| Valeur | Période              |
| -----: | -------------------- |
|      0 | Tempus Ordinarium    |
|      1 | Tempus Adventus      |
|      2 | Tempus Nativitatis   |
|      3 | Tempus Quadragesimae |
|      4 | Triduum Paschale     |
|      5 | Tempus Paschale      |
|      6 | Dies Sancti          |

### Accesseurs directs sur ENTRY_BUF

Ces fonctions évitent de lire `ENTRY_BUF` byte par byte depuis JS :

```c
// primary_index de la dernière TimelineEntry lue.
uint16_t kal_wasm_entry_primary_index(void);

// 1 si primary_index == 0 (Padding Entry), 0 sinon.
uint32_t kal_wasm_entry_is_padding(void);

// LiturgicalPeriod — bits [4:2] de occurrence_flags. Valide même pour les Padding Entries.
uint8_t  kal_wasm_entry_liturgical_period(void);

// Ordinal de semaine liturgique (byte 6). Valide même pour les Padding Entries.
uint8_t  kal_wasm_entry_liturgical_week(void);

// occurrence_flags bruts (byte 4).
uint8_t  kal_wasm_entry_occurrence_flags(void);
```

### Lecture du Feast Registry

```c
// Lit le FeastEntry pour registry_index (1-based) dans FEAST_BUF interne.
// registry_index == 0 retourne KAL_ERR_INDEX_OOB.
// Retourne 0 (OK) ou un code d'erreur négatif.
int32_t  kal_wasm_read_feast(uint16_t registry_index);

// Pointeur WASM vers FEAST_BUF (4 octets, little-endian).
// Valide après kal_wasm_read_feast.
uint32_t kal_wasm_feast_ptr(void);
```

Layout de `FEAST_BUF` (miroir de `FeastEntry` v6) :

| Offset | Champ      | Type   | Décodage                 |
| -----: | ---------- | ------ | ------------------------ |
|    0–1 | `feast_id` | u16 LE | Clé de lookup `.lits`    |
|    2–3 | `flags`    | u16 LE | Voir décodage ci-dessous |

Décodage de `FeastEntry.flags` (v6) :

| Bits    | Champ            | Valeurs                             |
| ------- | ---------------- | ----------------------------------- |
| [3:0]   | `Precedence`     | 0–12                                |
| [7:4]   | `Color`          | 0–5 (Albus, Rubeus, Viridis…)       |
| [10:8]  | réservés         | nuls (v6 — était LiturgicalPeriod)  |
| [13:11] | `Nature`         | 0–4 (Sollemnitas, Festum…)          |
| [14]    | `has_vigil_mass` | 1 = Messe de Vigile propre (corpus) |
| [15]    | réservé          | nul                                 |

> `LiturgicalPeriod` n'est plus dans `FeastEntry` depuis la v6 — lire via `kal_wasm_entry_liturgical_period()` ou `occurrence_flags[4:2]`.

### Fêtes secondaires — pattern out-static (SECONDARY_BUF)

```c
// Remplit SECONDARY_BUF avec count registry_indices depuis le Secondary Pool.
// secondary_offset : ENTRY_BUF[2..4] (secondary_offset de la TimelineEntry).
// count            : ENTRY_BUF[5]    (secondary_count de la TimelineEntry).
// Retourne 0 (OK) ou un code d'erreur négatif.
int32_t  kal_wasm_read_secondary(uint16_t secondary_offset, uint8_t count);

// Pointeur WASM vers SECONDARY_BUF (tableau de uint16_t LE, registry_indices 1-based).
// Appeler kal_wasm_read_feast sur chacun pour obtenir le FeastEntry correspondant.
// Valide après kal_wasm_read_secondary.
uint32_t kal_wasm_secondary_ptr(void);
```

### Résolution de label et annotation — pattern out-static

WASM MVP ne supporte pas le retour multi-valeur. Les résultats sont écrits dans des statics internes lus via accesseurs.

```c
// Résout label + annotation pour (year, doy) — lit d'abord l'entrée.
// Retourne : 1 = trouvé, 0 = absent (Padding Entry ou absent du .lits), < 0 = erreur.
int32_t  kal_wasm_get_label(uint16_t year, uint16_t doy);

// Résout label + annotation directement par feast_id (fêtes secondaires).
// feast_id : FEAST_BUF[0..2] après kal_wasm_read_feast.
// Retourne : 1 = trouvé, 0 = absent, < 0 = erreur.
int32_t  kal_wasm_get_label_by_id(uint16_t feast_id, uint16_t year);

// Pointeur WASM vers le label UTF-8 (sans null terminal).
uint32_t kal_wasm_label_ptr(void);
uint32_t kal_wasm_label_len(void);

// Pointeur WASM vers l'annotation UTF-8. ANNOTATION_LEN == 0 si absente.
// L'annotation peut contenir du Markdown inline (*italic*).
uint32_t kal_wasm_annotation_ptr(void);
uint32_t kal_wasm_annotation_len(void);
```

### Codes d'erreur

| Code | Constante                   | Origine | Signification                                          |
| ---: | --------------------------- | ------- | ------------------------------------------------------ |
|    0 | `KAL_ENGINE_OK`             | Core    | Succès                                                 |
|   -1 | `KAL_ERR_NULL_PTR`          | Core    | Pointeur null passé à une fonction FFI                 |
|   -2 | `KAL_ERR_BUF_TOO_SMALL`     | Core    | Buffer trop court pour contenir un header valide       |
|   -3 | `KAL_ERR_MAGIC`             | Core    | Magic bytes invalides                                  |
|   -4 | `KAL_ERR_VERSION`           | Core    | Version du format non supportée                        |
|   -5 | `KAL_ERR_CHECKSUM`          | Core    | SHA-256 incorrect — corruption du payload              |
|   -6 | `KAL_ERR_FILE_SIZE`         | Core    | Taille du buffer incohérente avec les métadonnées      |
|   -7 | `KAL_ERR_INDEX_OOB`         | Core    | `(year, doy)` ou `registry_index` hors plage           |
|   -8 | `KAL_ERR_POOL_OOB`          | Core    | Offset ou count hors des bornes du Secondary Pool      |
|  -10 | `KAL_ERR_SCHEMA`            | Core    | Discriminant de layout incompatible (dérive de schéma) |
|  -20 | `KAL_ERR_NOT_READY`         | Bridge  | Bridge non dans l'état requis                          |
|  -21 | `KAL_ERR_BUF_OVERFLOW`      | Bridge  | Taille demandée > capacité statique                    |
|  -22 | `KAL_ERR_BUILD_ID_MISMATCH` | Bridge  | `.kald` et `.lits` issus de builds différents          |
|  -23 | `KAL_ERR_LITS_INVALID`      | Bridge  | Header `.lits` invalide                                |

---

## Intégration JS

Séquence complète d'initialisation et de lecture :

```js
const { instance } = await WebAssembly.instantiateStreaming(
  fetch("liturgical_calendar_wasm.wasm"),
  {},
);
const { exports } = instance;
const memory = exports.memory;

// — Commit .kald —
const kaldBuf = await fetch("calendar.kald").then((r) => r.arrayBuffer());
const kaldPtr = exports.kal_wasm_alloc_kald(kaldBuf.byteLength);
new Uint8Array(memory.buffer, kaldPtr, kaldBuf.byteLength).set(
  new Uint8Array(kaldBuf),
);
exports.kal_wasm_commit_kald(); // 0 = OK

// — Commit .lits —
const litsBuf = await fetch("calendar.lits").then((r) => r.arrayBuffer());
const litsPtr = exports.kal_wasm_alloc_lits(litsBuf.byteLength);
new Uint8Array(memory.buffer, litsPtr, litsBuf.byteLength).set(
  new Uint8Array(litsBuf),
);
exports.kal_wasm_commit_lits(); // 0 = OK

// — Lecture d'une entrée —
exports.kal_wasm_read_day(2026, 359); // 25 décembre 2026

// Accesseurs directs (recommandé) :
const primaryIndex = exports.kal_wasm_entry_primary_index();
const isPadding = exports.kal_wasm_entry_is_padding(); // 0 ou 1
const litPeriod = exports.kal_wasm_entry_liturgical_period(); // 0–6
const litWeek = exports.kal_wasm_entry_liturgical_week(); // 0–34
const occFlags = exports.kal_wasm_entry_occurrence_flags();
const hasVesperaeI = (occFlags & 0x01) !== 0;
const hasVigilia = (occFlags & 0x02) !== 0;

// Lecture directe depuis ENTRY_BUF (alternative bas niveau) :
const view = new DataView(memory.buffer, exports.kal_wasm_entry_ptr(), 8);
const secondaryOffset = view.getUint16(2, true); // secondary_offset
const secondaryCount = view.getUint8(5); // secondary_count

// — Label + annotation fête principale —
const decoder = new TextDecoder("utf-8");
const rc = exports.kal_wasm_get_label(2026, 359);
if (rc === 1) {
  const label = decoder.decode(
    new Uint8Array(
      memory.buffer,
      exports.kal_wasm_label_ptr(),
      exports.kal_wasm_label_len(),
    ),
  );
  const hasAnnotation = exports.kal_wasm_annotation_len() > 0;
  const annotation = hasAnnotation
    ? decoder.decode(
        new Uint8Array(
          memory.buffer,
          exports.kal_wasm_annotation_ptr(),
          exports.kal_wasm_annotation_len(),
        ),
      )
    : null;
}

// — Fêtes secondaires —
if (secondaryCount > 0) {
  exports.kal_wasm_read_secondary(secondaryOffset, secondaryCount);
  const secView = new DataView(
    memory.buffer,
    exports.kal_wasm_secondary_ptr(),
    secondaryCount * 2,
  );
  for (let i = 0; i < secondaryCount; i++) {
    const registryIndex = secView.getUint16(i * 2, true);
    exports.kal_wasm_read_feast(registryIndex);
    const feastView = new DataView(
      memory.buffer,
      exports.kal_wasm_feast_ptr(),
      4,
    );
    const feastId = feastView.getUint16(0, true);

    exports.kal_wasm_get_label_by_id(feastId, 2026);
    const secLabel = decoder.decode(
      new Uint8Array(
        memory.buffer,
        exports.kal_wasm_label_ptr(),
        exports.kal_wasm_label_len(),
      ),
    );
  }
}

// — Jour sans fête (Padding Entry) — primitives temporelles disponibles —
exports.kal_wasm_read_day(2026, 183); // lundi ordinaire
if (exports.kal_wasm_entry_is_padding()) {
  const period = exports.kal_wasm_entry_liturgical_period(); // ex: 0 = TempusOrdinarium
  const week = exports.kal_wasm_entry_liturgical_week(); // ex: 13
  // Construire le label côté JS : "Feria II, Hebdomada XIII per annum"
}
```

Tous les labels sont décodés zero-copy : `TextDecoder` opère directement sur `WebAssembly.Memory` sans copie intermédiaire.

---

## Limites des buffers statiques

| Buffer          | Capacité | Dimensionnement                                 |
| --------------- | -------- | ----------------------------------------------- |
| `KALD_BUF`      | 2 MB     | 431 ans × 366 jours × 8 octets ≈ 1.26 MB + pool |
| `LITS_BUF`      | 512 KB   | Labels UTF-8 multi-langues                      |
| `ENTRY_BUF`     | 8 octets | Une `TimelineEntry` v6                          |
| `FEAST_BUF`     | 4 octets | Un `FeastEntry`                                 |
| `SECONDARY_BUF` | 32 × u16 | registry_indices secondaires par jour (max 32)  |

Si un artefact dépasse `KALD_CAP` ou `LITS_CAP`, `kal_wasm_alloc_*` retourne `0`. Les constantes sont ajustables dans `src/lib.rs` avant recompilation.

---

## Structure

```
crates/liturgical-calendar-wasm/
├── Cargo.toml     — cdylib, no_std, dépendance Core
└── src/
    └── lib.rs     — bridge complet

www/
├── index.html                       — page hôte (vues annuelle + journalière)
├── app.js                           — routeur et orchestration JS
├── server.py                        — serveur de développement SPA
├── liturgical_calendar_wasm.wasm    — binaire compilé (copié depuis target/)
├── calendar.kald                    — artefact Forge v6
└── calendar.lits                    — artefact Forge
```
