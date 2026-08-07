# liturgical-calendar-forge

Compilateur du calendrier liturgique catholique (Novus Ordo, 1969–2399).

Ingère un corpus de fichiers YAML décrivant le droit liturgique (fêtes, préséances, transferts), calcule l'intégralité du calendrier sur 431 années, et produit deux artefacts binaires statiques consommés par [`liturgical-calendar-core`](https://crates.io/crates/liturgical-calendar-core) :

- **`.kald`** — topologie calendaire : Feast Registry + Timeline + Secondary Pool
- **`.lits`** — labels i18n indexés par FeastID et plage d'années, un fichier par langue

## Philosophie

Toute la complexité liturgique — calcul de Pâques, résolution des préséances, transferts de fêtes, fallback latin — est traitée **une fois pour toutes** à la compilation. L'Engine runtime ne connaît que des lectures d'offset. Une modification du corpus YAML (canonisation, nouvelle règle) se traduit par une recompilation Forge — jamais par une modification du code applicatif.

## CLI

### Installation

```bash
cargo install --path ./liturgical-calendar-forge
# ou depuis crates.io :
cargo install liturgical-calendar-forge
```

### `kal-forge` — Compilation

```bash
# Scope universel romain — .kald seul
kal-forge -s universale

# Scope universel — .kald + .lits (toutes les langues découvertes)
kal-forge -s universale -i

# Propre national
kal-forge -s nationalia/FR -i

# Tout le rite romain (tous les scopes non-DRAFT)
kal-forge -i

# Rite alternatif
kal-forge -r ambrosianus -s universale -i
```

| Court | Long               | Défaut        | Description                                   |
| ----- | ------------------ | ------------- | --------------------------------------------- |
| `-r`  | `--rite`           | `romanus`     | Rite à compiler                               |
| `-s`  | `--scope`          | _(tous)_      | Scope cible. Si absent : compile tout le rite |
| `-c`  | `--corpus`         | `./corpus`    | Racine du corpus YAML                         |
| `-o`  | `--out`            | `./artifacts` | Répertoire de sortie                          |
| `-i`  | `--i18n`           | _(désactivé)_ | Active la production des `.lits`              |
| `-d`  | `--include-drafts` | _(désactivé)_ | Compile les scopes marqués `DRAFT`            |

### `kal-read` — Inspection

```bash
cargo run -q -p liturgical-calendar-forge --bin kal-read -- \
    --kald ./artifacts/romanus_universale.kald \
    --lits ./artifacts/romanus_universale_la.lits \
    --year 2026 --doy 354
```

```
year=2026  doy=354
  registry_index : 245
  feast_id       : 0x3006
  flags          : 0x1131
  precedence     : SollemnitatesMaiores (1)
  color          : Violaceus (3)
  period         : TempusAdventus (1)
  nature         : Dominica (2)
  has_vigil_mass : false
  secondary      : 0 entrée(s) (offset=0)
  vesperae_i     : false
  vigilia        : false
  label          : Dominica IV Adventus
  annotation     : *Rorate cæli*
```

Avec fêtes secondaires :

```
  Célébrations secondaires :

    [0] registry_index=113
    feast_id       : 0x003d
    precedence     : MemoriaeAdLibitum (11)
    color          : Albus (0)
    period         : TempusOrdinarium (0)
    nature         : Memoria (3)
    has_vigil_mass : false
    label          : Immaculati Cordis Beatæ Mariæ Virginis
```

## Pipeline de compilation

```
YAML corpus + dictionnaires i18n
  ↓ Étape 1     Rule Parsing — validations V1–V6, V-T1–T5, V-I1–I3
  ↓ Étape 2     ID Allocation — feast_registry.lock (IDs stables)
  ↓ Étapes 3–5a Canonicalization → Resolution (toutes les années, 431×)
  ↓ Étape 5b    Pass 1 AOT — Feast Registry (invariants, déduplication globale)
  ↓ Étape 5c    Pass 2 AOT — Timeline + Secondary Pool (occurrences)
  ↓ Étape 5d    Vespers pass — occurrence_flags (vesperae_i, vigilia)
  ↓ Étape 6     Binary Packing — .kald + .lits par langue
```

La séparation en deux passes AOT est structurelle : le Feast Registry doit être complet avant que la Timeline puisse être générée (chaque `TimelineEntry` référence un `registry_index`). Les invariants d'une fête (couleur, nature, préséance) ne sont sérialisés qu'une fois quelle que soit la durée de sa présence dans le corpus.

Toute violation de validation est **fatale** — la Forge n'émet aucun binaire partiel.

## Format du Corpus YAML

```yaml
# corpus/romanus/universale/sanctorale/06/petri_et_pauli.yaml
version: 1
category: 1
date:
  month: 6
  day: 29
history:
  - from: 1969
    precedence: 5
    nature: sollemnitas
    color: rubeus
    has_vigil_mass: true
```

```yaml
# corpus/romanus/universale/i18n/la/petri_et_pauli.yaml
version: 1
history:
  - from: 1969
    label: "Ss. Petri et Pauli, apostolorum"
```

Le corpus complet et la documentation du schéma YAML sont disponibles dans [`docs/liturgical-scheme.md`](../docs/specifications/liturgical-scheme.md).

## Stabilité des Identifiants

Deux fichiers lock versionnés garantissent la stabilité des FeastIDs et variant_ids entre compilations :

```
corpus/{rite}/feast_registry.lock     — FeastID par slug
corpus/{rite}/variant_registry.lock   — variant_id par scope
```

Un slug retiré du corpus est **tombstoné** — son ID ne sera jamais réalloué.

## Artefacts Produits

Nommage : chemin de scope aplati avec `_` comme séparateur.

| Scope                   | `.kald`                      | `.lits`                         |
| ----------------------- | ---------------------------- | ------------------------------- |
| `romanus/universale`    | `romanus_universale.kald`    | `romanus_universale_la.lits`    |
| `romanus/nationalia/FR` | `romanus_nationalia_FR.kald` | `romanus_nationalia_FR_fr.lits` |

## Utilisation Programmatique

```rust
use liturgical_calendar_forge::{compile, parsing::ingest_corpus, I18nConfig};

let registry = ingest_corpus(&corpus_root)?;
let checksum = compile(registry, &output_path, variant_id, None, &lock_path)?;

// Les 8 premiers octets du SHA-256 sont le build_id — inscrit dans chaque .lits.
let build_id = u64::from_le_bytes(checksum[..8].try_into().unwrap());
println!("build_id = {build_id:#018x}");
```

## Dépendances

- [`liturgical-calendar-core`](https://crates.io/crates/liturgical-calendar-core) — types `TimelineEntry`, `FeastEntry`, validation header
- `serde` + `serde_yml` — désérialisation YAML
- `sha2` — SHA-256 du payload `.kald`
- `toml` — lecture/écriture des fichiers lock
- `clap` — interface CLI
