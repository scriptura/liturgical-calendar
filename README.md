# Liturgical Calendar

![Architecture: ECS / DOD](https://img.shields.io/badge/Architecture-ECS_%2F_DOD-blue?style=for-the-badge)
![Pipeline: AOT](https://img.shields.io/badge/Pipeline-AOT-success?style=for-the-badge)
![Memory: no__std](https://img.shields.io/badge/Memory-no__std-orange?style=for-the-badge)
![Allocation: zero--alloc](https://img.shields.io/badge/Allocation-zero--alloc-red?style=for-the-badge)

Un moteur de référence haute performance pour le calcul et la consultation du calendrier liturgique catholique (Novus Ordo, 1969–2399).

Ce projet repose sur un changement de paradigme : la donnée liturgique n'est pas traitée comme un algorithme perpétuel, mais comme une **vue matérialisée** d'un état du droit à un instant T.

## Philosophie : Data over Logic

Calculer dynamiquement le calendrier liturgique à l'exécution — dates de Pâques, préséances, transferts de fêtes — entraîne une complexité inutile et des risques d'incohérence entre plateformes. L'intelligence métier est entièrement déportée en amont.

Le système se divise en deux composants asymétriques :

**La Forge** (`liturgical-calendar-forge`) ingère les règles liturgiques déclarées en YAML, calcule les dates mobiles sur plusieurs siècles, résout les conflits calendaires, et génère deux artefacts binaires statiques et cryptographiquement vérifiés : un `.kald` (topologie) et un `.lits` par langue (labels).

**L'Engine** (`liturgical-calendar-core`) est le runtime ultra-léger intégré dans vos applications. Il ne contient aucune règle liturgique — il lit la table pré-calculée. Un accès à n'importe quel jour de n'importe quelle année se fait en O(1), par simple calcul d'offset.

## Garanties Techniques

- **Zéro calcul à l'exécution** — L'Engine accède à `(année, doy)` par offset arithmétique dans un buffer contigu. Aucun branchement conditionnel lié aux années bissextiles ou aux fêtes mobiles.
- **Artefacts comme source de vérité** — Une canonisation, une modification de préséance ou une nouvelle règle de transfert se traduit par une mise à jour YAML + une recompilation Forge. L'Engine consomme le nouveau `.kald` sans modification ni recompilation.
- **`no_std`, `no_alloc`** — L'Engine fonctionne sans bibliothèque standard ni allocation mémoire dynamique.
- **FFI C native** — Interface C-ABI : embarquable partout (serveurs, iOS/Android, systèmes embarqués, WebAssembly).
- **Validation AOT exhaustive** — Tout le corpus YAML est validé formellement à la compilation. Cycles de dépendances, collisions de préséance, incohérences de dates : toute erreur de configuration est fatale à la Forge. Aucune ne peut atteindre le runtime.
- **Déterminisme garanti** — Build identique sur toute machine. Les `feast_registry.lock` et `variant_registry.lock` versionnés garantissent la stabilité des identifiants numériques entre compilations.

## Architecture des Artefacts

```
.kald  —  Header 80 octets (magic, version, SHA-256, layout_discriminant, registry_count…)
          Feast Registry : FeastEntry × registry_count  (4 octets chacune — invariants AOT)
          Timeline       : TimelineEntry × 157 746      (8 octets chacune — occurrences)
          Secondary Pool : registry_index values (u16)

.lits  —  Entry Table (feast_id, from, to, label_offset, annotation_offset)
          String Pool UTF-8 (labels + annotations, null-terminés)
          Couplé au .kald via build_id (kald_checksum[..8])
```

`FeastEntry` — 4 octets, stride constant (invariants de la fête, indépendants de l'année) :

```
feast_id  u16   —  identifiant corpus (pour vérification croisée .lits)
flags     u16   —  precedence[3:0] | color[7:4] | period[10:8] | nature[13:11] | vigil[14]
```

`TimelineEntry` — 8 octets, stride constant (occurrence journalière) :

```
primary_index    u16   —  registry_index de la fête principale (0 = Padding Entry)
secondary_offset u16   —  offset dans le Secondary Pool
occurrence_flags u8    —  bit 0 = vesperae_i, bit 1 = vigilia
secondary_count  u8    —  nombre de fêtes secondaires
_reserved        u16   —  0x0000
```

Les invariants d'une fête (couleur, nature, préséance) ne sont stockés qu'une seule fois dans le Feast Registry, indépendamment du nombre d'années où elle est célébrée.

## Organisation du Corpus

La source de vérité est un corpus de fichiers YAML organisé par rite et juridiction :

```
corpus/
├── romanus/                      ← Rite romain
│   ├── feast_registry.lock       ← IDs stables — versionné
│   ├── variant_registry.lock     ← variant_id par scope — versionné
│   ├── universale/
│   │   ├── i18n/la/              ← labels latins (obligatoires)
│   │   ├── temporale/            ← fêtes mobiles
│   │   └── sanctorale/           ← fêtes fixes
│   ├── nationalia/FR/
│   │   ├── i18n/
│   │   └── sanctorale/
│   └── dioecesana/PARIS/
└── ambrosianus/                  ← Rite ambrosien
```

Chaque fête = un fichier YAML. Le nom du fichier (sans extension) **est** le slug — jamais déclaré dans le corps du YAML. Aucun champ textuel dans le corpus : les labels sont externalisés dans les dictionnaires `i18n/`.

## Compilation et Lecture

### Prérequis

- Rust toolchain ≥ 1.77
- `cargo install --path ./liturgical-calendar-forge`

### Produire les artefacts

```bash
# Scope universel romain — .kald + .lits latin
kal-forge -s universale -i

# Propre national France
kal-forge -s nationalia/FR -i

# Tout le rite romain (tous les scopes non-DRAFT)
kal-forge -i
```

Produit dans `./artifacts/` :

```
romanus_universale.kald
romanus_universale_la.lits
romanus_nationalia_FR.kald
romanus_nationalia_FR_la.lits
romanus_nationalia_FR_fr.lits
```

### Lire une entrée

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

### Intégration Engine (API C)

```c
// Validation au chargement (avec SHA-256) — appeler une seule fois
int rc = kal_validate_header(kald_data, kald_len, NULL);

// Lecture d'une occurrence journalière — O(1)
TimelineEntry entry;
rc = kal_read_entry(kald_data, kald_len, 2025, 109, &entry);

// Résolution des invariants de la fête — O(1)
FeastEntry feast;
rc = kal_read_feast(kald_data, kald_len, entry.primary_index, &feast);
uint8_t precedence = feast.flags & 0x000F;
```

## Structure du Projet

```
liturgical-calendar/
├── crates/
│   ├── core/                         ← Engine (no_std, no_alloc, FFI C)
│   │   └── src/
│   │       ├── entry.rs              ← TimelineEntry, FeastEntry, LAYOUT_DISCRIMINANT
│   │       ├── ffi.rs                ← kal_read_entry, kal_read_feast, kal_validate_header, …
│   │       ├── header.rs             ← validation .kald (fast + checksum)
│   │       ├── lits_provider.rs      ← LitsProvider (zero-copy)
│   │       └── types.rs              ← Precedence, Nature, Color, LiturgicalPeriod
│   └── forge/                        ← Compilateur YAML → binaire
│       └── src/
│           ├── main.rs               ← CLI kal-forge
│           ├── bin/kal_read.rs       ← CLI kal-read
│           ├── parsing.rs            ← ingestion YAML, FeastRegistry
│           ├── canonicalization.rs   ← Pâques, ancres, DOY
│           ├── resolution.rs         ← conflit de préséance, transfers
│           ├── materialization.rs    ← FeastRegistryBuilder, TimelineEntry, Secondary Pool
│           ├── packing.rs            ← sérialisation .kald (2 passes AOT)
│           ├── i18n.rs               ← DictStore, LabelTable, fallback latin
│           ├── lits_writer.rs        ← sérialisation .lits
│           ├── lock.rs               ← feast_registry.lock
│           └── variant_lock.rs       ← variant_registry.lock
├── corpus/                           ← Source de vérité liturgique
└── artifacts/                        ← Artefacts compilés (ignorés par Git)
```

## Documentation

- [`docs/liturgical-scheme.md`](docs/liturgical-scheme.md) — Contrat de données YAML (format corpus + i18n)
- [`docs/kal-forge-guide.md`](docs/kal-forge-guide.md) — Mode d'emploi `kal-forge` et `kal-read`
- [`docs/adr/`](docs/adr/) — Architecture Decision Records

## Couverture Temporelle

1969 (réforme du calendrier romain) → 2399. 431 années, 157 746 slots, layout AOT invariant.
