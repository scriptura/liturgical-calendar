# Liturgical Scheme v1.0 — Contrat de Données Amont

**Statut** : Canonique / Source de Vérité YAML  
**Scope** : `liturgical-calendar-forge` — Étapes 1 (Rule Parsing) et 2 (Canonicalization)  
**Référence** : `specification.md` v2.0  
**Date de Révision** : 2026-04-09 — GELÉ  
**Version** : 1.3.1 ❄️

---

## 0. Rôle de ce Document

Ce document est le **contrat de données amont** de la Forge. Il définit exhaustivement le format YAML utilisé pour décrire les calendriers liturgiques (universel, national, diocésain). Toute entrée conforme à ce schéma peut être ingérée sans ambiguïté par les Étapes 1 et 2 du pipeline Forge.

**Flux de transformation :**

```
YAML (version, category, date|mobile, transfers, history…)   [pur graphe métier — zéro String]
+ Dictionnaires i18n (la/, fr/, …)                           [source de vérité textuelle externe]
  → [Étape 1] Rule Parsing + Validation V1–V6, V-T1–V-T3
    → [Étape 1bis] i18n Resolution — corrélation YAML ↔ dict, fallback latin AOT (V-I1)
      → [Étape 2] Canonicalization (DOY, Pâques, dates mobiles)
        → [Étapes 3–5] Resolution (bloc transfers) → Materialization → Binary Packing
          → .kald  (CalendarEntry 8 bytes/slot — topologie pure, zéro chaîne)
          → .lits  (Language Index Table — labels indexés par FeastID + plage d'années)
```

**Invariants absolus :**

- Toute entrée YAML est **validée à la compilation** (AOT). Aucune erreur de configuration ne peut atteindre le runtime.
- Le `slug` est la clé de déduplication humaine. Il est **déduit du stem du nom de fichier** (`path.file_stem()`) — il n'est **pas déclaré dans le corps du YAML**. La Forge le transforme en `FeastID` (u16) via le `FeastRegistry`. Le slug n'existe pas dans le binaire `.kald`.
- **Un rename du fichier est sémantiquement un changement d'identité de la fête**, pas un refactor. Le `feast_registry.lock` (INV-FORGE-3 dans la spec) protège la stabilité des FeastIDs inter-builds à condition d'être versionné avec le corpus YAML.
- **Zéro String dans le YAML.** Le YAML est un pur graphe de données métier : aucun champ textuel (`title`, `name`, `description`, etc.). Les labels sont externalisés dans des dictionnaires i18n séparés (§4.4).
- **Identité i18n dérivée.** La clé composite `{slug}.{from}.{field}` identifie un label de manière unique. Elle est implicite — jamais déclarée dans le YAML — et résolue par la Forge à la compilation.
- **Latin comme langue source obligatoire.** Pour chaque clé `{slug}.{from}.{field}` présente dans le corpus YAML, une entrée doit exister dans le dictionnaire `i18n/la/`. Son absence est une erreur fatale (V-I1). Le fallback vers le latin est réalisé par la Forge (fusion AOT) — jamais au runtime.
- La Forge rejette tout fichier YAML contenant des plages temporelles incompatibles, des valeurs hors domaine, ou un stem de fichier ne respectant pas la syntaxe `[a-z][a-z0-9_]*`. Tout échec de validation est fatal — aucune sortie partielle.
- Les champs `from` / `to` expriment des années grégoriennes entières. Bornes inclusives. Plage admise : **[1969, 2399]**.

---

## 1. Organisation du Corpus sur Disque

### 1.1 Principe : 1 Slug = 1 Fichier

Chaque fête liturgique est décrite dans un **fichier YAML indépendant**, nommé d'après son slug. **Le stem du nom de fichier (sans extension) constitue la définition du slug** — il n'est pas redéclaré dans le corps YAML. Cette atomicité garantit :

- Des diffs Git par fête (lisibilité, code review)
- Un corpus partiel valide (un diocèse peut ne livrer que ses propres fichiers)
- L'absence de collision de slugs au niveau du système de fichiers (deux fichiers de même nom dans le même répertoire est une impossibilité FS)
- La suppression de toute redondance slug YAML ↔ nom de fichier (principe DRY)

### 1.2 Hiérarchie des Répertoires

```
data/
├── universale/
│   ├── temporale/          ← Proprium de Tempore : fêtes à date mobile  (category = 0)
│   │   ├── pascha.yaml
│   │   ├── ascensio_domini.yaml
│   │   ├── pentecostes.yaml
│   │   ├── feria_iv_cinerum.yaml
│   │   ├── dominica_in_palmis.yaml
│   │   └── ...
│   └── sanctorale/         ← Sanctorale universale : fêtes à date fixe  (category ≥ 1)
│       ├── nativitas_domini.yaml
│       ├── assumptio_bmv.yaml
│       ├── omnium_sanctorum.yaml
│       └── ...
├── nationalia/
│   └── {ISO}/              ← Code ISO 3166-1 alpha-2 (ex : FR, PL, DE)
│       ├── temporale/      ← Proprium de Tempore national (peu fréquent)
│       └── sanctorale/     ← Saints propres et surcharges nationales
│           └── {slug}.yaml
└── dioecesana/
    └── {ID}/               ← Identifiant diocésain (ex : PARIS, LYON)
        ├── temporale/
        └── sanctorale/
            └── {slug}.yaml
```

**Règles structurelles :**

- `temporale/` accueille les fêtes déclarées avec un bloc `mobile` (anchor + offset). C'est le Proprium de Tempore — le cycle pascal et ses dépendances.
- `sanctorale/` accueille les fêtes déclarées avec un bloc `date` (fixe). C'est le cycle annuel des saints et des solennités fixes.
- La correspondance répertoire ↔ `category` est **documentaire, pas normative**. Le champ `category` reste déclaré explicitement dans chaque fichier YAML — un diocèse peut utiliser des catégories 2 ou 3 dans son `sanctorale/`.
- Un fichier situé dans `nationalia/FR/sanctorale/nativitas_domini.yaml` est une **surcharge nationale** du slug `nativitas_domini` universel. Aucune redéclaration de `date` n'est requise si la date est héritée du scope universel.

### 1.3 Dérivation du Scope et de la Region depuis le Chemin

Le **scope** et la **region** sont déduits du chemin du fichier. Ils ne sont **pas** répétés dans l'en-tête YAML — supprimer cette redondance est l'un des bénéfices de l'approche atomique.

| Chemin                                 | Scope déduit | Region déduite |
| -------------------------------------- | ------------ | -------------- |
| `data/universale/**/{slug}.yaml`       | `universal`  | `null`         |
| `data/nationalia/{ISO}/**/{slug}.yaml` | `national`   | `{ISO}`        |
| `data/dioecesana/{ID}/**/{slug}.yaml`  | `diocesan`   | `{ID}`         |

La Forge valide la cohérence path ↔ contenu lors de l'ingestion. Un fichier dont l'en-tête déclare explicitement un scope ou une region **contradictoire** avec son chemin est rejeté avec `ParseError::ScopePathMismatch { path, declared_scope }`.

### 1.4 Ordre d'Ingestion (INV-FORGE-1)

La Forge ingère les fichiers dans l'ordre suivant, sans exception :

```
1. data/universale/temporale/    — triés lexicographiquement par nom de fichier
2. data/universale/sanctorale/   — triés lexicographiquement par nom de fichier
3. data/nationalia/{ISO}/temporale/   — si applicable, tri lex.
4. data/nationalia/{ISO}/sanctorale/  — si applicable, tri lex.
5. data/dioecesana/{ID}/temporale/    — si applicable, tri lex.
6. data/dioecesana/{ID}/sanctorale/   — si applicable, tri lex.
```

Le nommage slug (latin snake_case) garantit que l'ordre lexicographique des noms de fichiers est déterministe et reproductible. `fs::read_dir` n'étant pas ordonné, la Forge collecte les chemins, les trie, puis les ingère — identique à INV-FORGE-1 de la spec.

### 1.5 Format d'un Fichier Atomique

L'en-tête de fichier est allégé : `scope`, `region` et `slug` sont déduits du chemin du fichier (§1.3 et §1.1). `version` est obligatoire pour la détection de schéma incompatible.

```yaml
version: 1 # Obligatoire — détection UnsupportedSchemaVersion
category: 0 # Sous-espace FeastID — bits [13:12] — voir §2.2
date: # OU mobile: — exactement l'un des deux
  month: 12
  day: 25

history:
  - from: 1969
    to: ~
    precedence: 1
    nature: sollemnitas
    color: albus
    season: tempus_nativitatis # Optionnel — voir §2.3
    # title : absent du YAML — défini dans i18n/la/nativitas_domini.yaml (§4.4)
```

> **Surcharge partielle (scope national/diocésain) :** un fichier de surcharge peut omettre le bloc `date`/`mobile` si la temporalité est héritée du scope universel. Il ne déclare que les champs `history` qui diffèrent. La Forge fusionne les blocs `history` selon les règles de §5.2.

---

## 2. Définition d'une Fête (`feast`)

Chaque entrée dans la liste `feasts` définit une fête liturgique. Elle comporte deux niveaux :

1. **Identité** : champs stables dans le temps (`slug`, `id`, `date` ou `mobile`, `scope`, `region`, `category`)
2. **Historique** : tableau `history` — un ou plusieurs blocs décrivant les métadonnées pour des plages d'années distinctes

```yaml
feasts:
  - id: <u16> # Optionnel — voir §2.2
    # slug : déduit du stem du nom de fichier — non déclaré ici (§2.1)
    # scope et region : déduits du chemin du fichier (§1.3) — non répétés ici
    category: <0–3> # Sous-espace FeastID — bits [13:12], 4 valeurs — voir §2.2

    # Temporalité — exactement UN des deux blocs suivants doit être présent
    date: # Fête à date FIXE — voir §3.1
      month: <1–12>
      day: <1–31>
    mobile: # Fête à date MOBILE — voir §3.2
      anchor: <anchor_id>
      offset: <integer> # Peut être négatif (ex: -7 pour le dimanche avant Pâques)

    # Transferts déclaratifs — optionnel — voir §2.4
    transfers:
      - collides: <slug_concurrent>  # Slug (= stem du fichier) de la fête en collision
        offset: <integer>            # OU date: — exclusifs — voir §2.4
      - collides: <slug_concurrent>
        date:
          month: <1–12>
          day: <1–31>

    history: # Tableau ordonné des versions — voir §4
      - from: <year>
        to: <year|~>
        precedence: <0–12>
        nature: <string>
        color: <string>
        season: <string> # Optionnel — voir §2.3
        # Labels textuels (title, etc.) : externalisés dans i18n/{lang}/{slug}.yaml (§4.4)
```

---

## 2.1 Identité : `slug`

Le `slug` est la **clé primaire immuable** d'une fête dans le `FeastRegistry`. Il est **déduit du stem du nom de fichier** lors de l'ingestion par la Forge (`path.file_stem()` — le nom sans l'extension `.yaml`). Il n'est pas déclaré dans le corps du YAML.

```
nativitas_domini.yaml  →  slug = "nativitas_domini"
ascensio_domini.yaml   →  slug = "ascensio_domini"
ioannis_pauli_ii.yaml  →  slug = "ioannis_pauli_ii"
```

**Validation préalable à la désérialisation :** la Forge valide la syntaxe du stem **avant** de tenter la lecture YAML. Tout stem ne satisfaisant pas `[a-z][a-z0-9_]*` est rejeté avec `ParseError::InvalidSlugSyntax(stem)`. Le fichier n'est pas parsé.

**Règle de neutralité obligatoire :** le slug identifie la **personne ou l'événement**, pas son statut liturgique courant.

```
✅  ioannis_pauli_ii        ← stable, indépendant du statut (Beatus / Sanctus)
❌  s_ioannis_pauli_ii      ← encode "Sanctus" — cassé à la béatification
❌  b_caroli_de_foucauld    ← encode "Beatus" — cassé si canonisé ultérieurement
```

**Justification structurelle :** si le statut est encodé dans le slug, une canonisation force un rename de fichier → un nouveau `FeastID` → une rupture de continuité historique. Le `FeastID` alloué par la Forge doit être **stable** sur toute la plage 1969–2399 pour un slug donné.

**Invariant opérationnel :** un rename de fichier est sémantiquement un changement d'identité de la fête. Il entraîne l'allocation d'un nouveau FeastID et la tombstonisation de l'ancien dans `feast_registry.lock`. Cette opération est irréversible — le lock garantit qu'un FeastID tombstoné n'est jamais réalloué.

**Syntaxe** : `[a-z][a-z0-9_]*` — Latin snake_case, sans accent, sans tiret, sans espace. Le premier caractère doit être une lettre (pas un chiffre). Validation V6 (appliquée au stem du nom de fichier).

**Exemples valides :** `nativitas_domini`, `ioannis_pauli_ii`, `ascensio_domini`, `dominica_in_palmis`

---

## 2.2 Identité : `id` et `category`

**FeastID — Layout u16 officiel (spec §5.1) :**

```
 15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0
┌───┬───┬───┬───┬───────────────────────────────────────────────┐
│ S   S │ C   C │             Sequence (12 bits)                │
└───┴───┴───┴───┴───────────────────────────────────────────────┘
  [15:14]  [13:12]                [11:0]
   Scope   Category              Sequence
```

| Bits  | Champ      | Valeurs                                                                           |
| ----- | ---------- | --------------------------------------------------------------------------------- |
| 15–14 | `Scope`    | `00` = Universal · `01` = National · `10` = Diocesan · `11` = réservé             |
| 13–12 | `Category` | 0–3 (4 catégories par scope)                                                      |
| 11–0  | `Sequence` | 1–4095 par (Scope, Category) ; `0` non allouable (`0x0000` réservé Padding Entry) |

**`category` (bits [13:12] du FeastID) :**

| Valeur | Usage conventionnel                             |
| ------ | ----------------------------------------------- |
| 0      | Fêtes du Temporal (Dominicales, Pâques, Avent…) |
| 1      | Fêtes du Sanctoral universel                    |
| 2      | Fêtes propres nationales ou diocésaines         |
| 3      | Usage libre / extensions futures                |

**`id` (u16, optionnel) :**

Si absent : la Forge alloue le prochain FeastID libre dans `(scope, category)` selon INV-FORGE-3 — ordre lexicographique des slugs au premier build, puis stabilité garantie par `feast_registry.lock` aux builds suivants. L'ordre d'apparition dans le YAML n'influe pas sur le FeastID alloué.

Si présent : la Forge vérifie l'absence de collision dans le `FeastRegistry`. Une collision → `RegistryError::FeastIDConflict`. Ce mécanisme est réservé aux fêtes dont l'identifiant doit être stable et documenté.

**Validation V3 :** `allocated_count(scope, category) ≤ 4095` — violation → `RegistryError::FeastIDExhausted { scope, category }`.

---

## 2.3 Champ `season` dans `history`

Le champ `season` est **optionnel**. Quand il est absent, la Forge le calcule automatiquement depuis les `SeasonBoundaries` de l'année en cours (Étape 2, Canonicalization).

Il peut être fourni explicitement pour les fêtes dont la saison est fixe indépendamment du calendrier temporel (ex: une Solennité du Sanctoral tombe toujours en `TempusOrdinarium` sauf si elle coïncide avec une période privilégiée — la Forge résout ce conflit en Étape 3).

**Valeurs admises :** voir §6.4.

---

## 2.4 Bloc `transfers` — Résolution Déclarative des Collisions

Le bloc `transfers` est **optionnel**. Il déclare les règles de repli d'une fête lorsqu'elle entre en collision sur le même DOY avec une fête identifiée par son slug. Il est consommé **exclusivement par la Passe 3 de l'Étape 3 (Conflict Resolution)**.

En l'absence de ce bloc, la Forge applique les règles générales de §3.2–§3.3 (Precedence, tiebreakers). En présence de ce bloc, les entrées `transfers` constituent une table de dispatch déclarative — aucun code impératif conditionnel dans la Forge pour les exceptions liturgiques.

### Structure

```yaml
transfers:
  - collides: <slug_concurrent>  # Slug (stem du fichier) de la fête déclenchante
    offset: <uint>               # Décalage relatif en jours (non signé, > 0) si collision
  - collides: <slug_concurrent>
    date:                        # Date fixe de repli si collision
      month: <1–12>
      day: <1–31>
```

### Sémantique

| Clé       | Type              | Signification                                                                        |
| --------- | ----------------- | ------------------------------------------------------------------------------------ |
| `collides`| String (slug)     | La fête dont la présence sur le même DOY déclenche la règle de transfert             |
| `offset`  | **u32 ≥ 1**       | Décalage en jours vers l'avant depuis le DOY de collision — **strictement positif** |
| `date`    | `{month, day}`    | Date fixe de repli, indépendante du DOY de collision                                 |

**`offset` et `date` sont mutuellement exclusifs.** Une entrée qui déclare les deux est rejetée avec `ParseError::TransferAmbiguous { slug, collides }`. Une entrée qui ne déclare ni l'un ni l'autre est également invalide — la règle serait sans effet.

### Invariants

**Résolution au premier build :** le slug déclaré dans `collides` doit être résolvable dans le `FeastRegistry` au moment de la Passe 3. Un slug inconnu (absent du corpus compilé, faute de frappe) est rejeté avec `ParseError::UnknownCollidesTarget { slug, collides }`. Ce n'est pas un avertissement — une règle de transfert non résolvable est une règle silencieusement morte.

**Résolution à un seul niveau :** si la date de repli (via `offset` ou `date`) atterrit elle-même sur un slot occupé, la Forge applique les règles générales de Precedence (§3.2). Elle **ne réapplique pas** le bloc `transfers` de la fête déplacée sur sa nouvelle date. Tout atterrissage sur un slot occupé après déplacement produit un `ConflictWarning`, pas une erreur fatale.

**Unicité de la clé `collides` :** deux entrées `transfers` ne peuvent référencer le même slug concurrent. Violation → `ParseError::TransferDuplicateCollides { slug, collides }`.

### Exemple — Sainte Famille

La Sainte Famille est normalement le dimanche dans l'Octave de Noël. Si Noël tombe un dimanche (et qu'il n'y a pas d'octave), la Sainte Famille est transférée au 30 décembre.

```yaml
# data/universale/temporale/sanctae_familiae.yaml
version: 1
category: 0
mobile:
  anchor: pascha
  offset: -7 # exemple fictif — la vraie règle est relative à Noël

transfers:
  - collides: nativitas_domini
    date:
      month: 12
      day: 30
```

### Exemple — Sacré-Cœur et Corpus Christi

```yaml
transfers:
  - collides: sacratissimi_cordis_iesu
    offset: 6  # déplacement vers l'avant — V-T4 : offset ≥ 1
  - collides: corpus_christi
    date:
      month: 6
      day: 25
```

---

## 3. Temporalité

### 3.1 Dates Fixes

```yaml
date:
  month: <1–12> # Mois grégorien, 1-based
  day: <1–31> # Jour du mois, 1-based
```

**Conversion en DOY (0-based) par la Forge :**

```rust
// Table MONTH_STARTS (constante de compilation, §2.2 de la spec)
const MONTH_STARTS: [u16; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

fn doy_from_date(month: u8, day: u8) -> u16 {
    MONTH_STARTS[(month - 1) as usize] + (day - 1) as u16
}
// Exemples :
// Nativitas Domini (25 déc) : MONTH_STARTS[11] + 24 = 335 + 24 = 359
// Assumptio BMV (15 août)   : MONTH_STARTS[7] + 14 = 213 + 14 = 227
```

**Cas particulier — le 29 février (`month: 2, day: 29`) :**

Le 29 février est déclarable en YAML et produit toujours `doy = 59`. Pour les années non-bissextiles, la Forge écrit une **Padding Entry** à `doy = 59` (`primary_id = 0`, `secondary_count = 0`, `flags = 0`). La fête n'est pas transférée — elle est simplement absente pour cette année. L'Engine reçoit la Padding Entry et retourne `KAL_ENGINE_OK` avec `primary_id = 0` ; l'interprétation est laissée à l'appelant.

```yaml
# Exemple : Fête fixée au 29 février — fichier : sancti_cassiani.yaml
date:
  month: 2
  day: 29 # doy = 59 ; Padding Entry les années non-bissextiles
category: 1
history:
  - from: 1969
    precedence: 12
    nature: memoria
    color: rubeus
    # title → i18n/la/sancti_cassiani.yaml : { 1969: { title: "S. Cassianus" } }
```

**Validation V3 :** la Forge vérifie que `day` est cohérent avec `month` (ex: `month: 2, day: 30` → `ParseError::InvalidDate`). Le 29 février est la seule date admise en dehors des limites standard du mois ; toutes les autres dates invalides sont rejetées.

---

### 3.2 Dates Mobiles

Les fêtes mobiles sont exprimées par un **ancre** et un **offset** en jours.

```yaml
mobile:
  anchor: <anchor_id> # Identifiant d'ancre — voir tableau ci-dessous
  offset:
    <integer> # Offset en jours, peut être négatif
    # Exemples : +39, -7, 0
```

**Ancres disponibles :**

| `anchor_id`   | Définition canonique        | Résolution par la Forge                           |
| ------------- | --------------------------- | ------------------------------------------------- |
| `pascha`      | Dimanche de Pâques          | Meeus/Jones/Butcher (Étape 2)                     |
| `adventus`    | Premier dimanche de l'Avent | Dimanche le plus proche du 30 novembre (Étape 2)  |
| `pentecostes` | Pentecôte                   | `pascha + 49` (dérivée, non déclarée directement) |

> **Note :** `pentecostes` est une ancre de convenance. Elle est strictement équivalente à `anchor: pascha, offset: +49`. Les deux formes sont admises et produisent le même DOY.

**Fêtes mobiles standard dérivées de Pâques :**

| Fête                     | Ancre    | Offset |
| ------------------------ | -------- | ------ |
| Jeudi Saint              | `pascha` | -3     |
| Vendredi Saint           | `pascha` | -2     |
| Samedi Saint             | `pascha` | -1     |
| Ascensio Domini          | `pascha` | +39    |
| Pentecostes              | `pascha` | +49    |
| Corpus Christi           | `pascha` | +60    |
| Sacratissimi Cordis Iesu | `pascha` | +68    |
| Dominica in Palmis       | `pascha` | -7     |
| Feria IV Cinerum         | `pascha` | -46    |

**Exemple complet — Ascension :**

```yaml
# data/universale/temporale/ascensio_domini.yaml
version: 1
category: 0
mobile:
  anchor: pascha
  offset: +39
history:
  - from: 1969
    precedence: 1
    nature: sollemnitas
    color: albus
    season: tempus_paschale
    # title → i18n/la/ascensio_domini.yaml : { 1969: { title: "Ascensio Domini" } }
```

**Validation V4 — Cycles et dépendances :**

Chaque ancre déclarée doit être résolvable sans cycle. La Forge construit un graphe de dépendances de toutes les dates mobiles avant de les calculer. Un cycle (ex: une fête dont l'offset référencerait une autre fête mobile) est rejeté avec `ParseError::CircularDependency { slug, anchor }`.

Dans la version actuelle, seules les ancres de la table ci-dessus sont admises. Les ancres personnalisées (référençant d'autres fêtes par slug) ne sont **pas** supportées en v1.0.

---

## 4. Logique de l'Attribut `history`

### 4.1 Structure

Le bloc `history` est un tableau ordonné. Chaque entrée couvre une plage d'années `[from, to]` et définit les métadonnées de la fête pour cette période.

```yaml
history:
  - from: <year> # Borne inférieure inclusive. Défaut : 1969 si omis.
    to: <year|~> # Borne supérieure inclusive. null (ou omis) = indéfini.
    precedence: <0–12> # Rang liturgique effectif.
    nature: <string>   # Voir §6.2.
    color: <string>    # Voir §6.3.
    season: <string>   # Optionnel — voir §2.3 et §6.4.
    # Aucun champ textuel (title, name…) : labels externalisés dans i18n/{lang}/{slug}.yaml (§4.4)
```

**Sémantique de `to: ~` (null) :**

L'entrée est active de `from` jusqu'à la fin de la plage supportée (2399). Elle reste active pour toutes les années `y ≥ from` tant qu'aucune entrée ultérieure dans le même bloc `history` ne prend le relais.

**Ordre d'évaluation :** la Forge trie les entrées `history` par `from` croissant avant traitement. L'ordre dans le YAML n'est pas significatif — mais la Forge rejette toute ambiguïté (voir Validation V1).

### 4.2 Algorithme de Résolution Temporelle

Pour une année `y` et un slug `s`, la Forge sélectionne l'entrée unique dont `[from, to]` contient `y` :

```rust
fn resolve_feast_for_year<'a>(
    slug: &str,
    history: &'a [FeastVersion],
    year: u16,
) -> Result<Option<&'a FeastVersion>, RegistryError> {
    let candidates: Vec<_> = history
        .iter()
        .filter(|v| v.from <= year)
        .filter(|v| v.to.map_or(true, |to| year <= to))
        .collect();

    match candidates.len() {
        0 => Ok(None),                // Fête inexistante pour cette année
        1 => Ok(Some(candidates[0])), // Résolution unique — cas nominal
        _ => Err(RegistryError::TemporalOverlap {
            slug: slug.to_string(),
            year,
            conflicting_entries: candidates.len(),
        }),
    }
}
```

Si `resolve_feast_for_year` retourne `Ok(None)`, la fête est **absente** du dataset pour cette année — aucune `CalendarEntry` n'est produite pour ce slug. Ce n'est pas une erreur : la fête peut ne pas encore exister, ou avoir été supprimée du calendrier.

### 4.3 Exemple Complet — Jean-Paul II (Béatification → Canonisation)

```yaml
# data/nationalia/PL/sanctorale/ioannis_pauli_ii.yaml
# slug = "ioannis_pauli_ii", scope = national, region = PL — déduits du chemin
version: 1
category: 1
date:
  month: 10
  day: 22

history:
  # Période 1 : Béatification (2011–2013)
  # title → i18n/la/ioannis_pauli_ii.yaml : { 2011: { title: "B. Ioannes Paulus II" } }
  - from: 2011
    to: 2013
    precedence: 11
    nature: memoria
    color: albus

  # Période 2 : Canonisation (2014 → indéfini)
  # title → i18n/la/ioannis_pauli_ii.yaml : { 2014: { title: "S. Ioannes Paulus II" } }
  - from: 2014
    to: ~
    precedence: 12
    nature: memoria
    color: albus
```

**Lecture de la hiérarchie :**

La `Precedence` est numérique inverse : valeur plus faible = priorité plus haute. La béatification (valeur 11, Memoria obligatoria nationale) avait une priorité **plus haute** que la version canonisée (valeur 12, Memoria ad libitum universelle). Les deux peuvent coexister dans le registre car leurs scopes et leurs plages temporelles sont disjoints.

---

## 4.4 Dictionnaires i18n — Séparation Canon / Labels

### Structure sur disque

Les labels textuels sont stockés dans une arborescence `i18n/` parallèle au corpus YAML. Le latin (`la`) est la **langue source obligatoire** — référence de la Forge.

```
corpus/
├── universale/
│   ├── temporale/
│   │   └── ascensio_domini.yaml       ← graphe métier pur
│   └── sanctorale/
│       └── nativitas_domini.yaml
└── i18n/
    ├── la/                             ← source obligatoire
    │   ├── ascensio_domini.yaml
    │   └── nativitas_domini.yaml
    ├── fr/
    │   └── nativitas_domini.yaml
    └── de/
        └── nativitas_domini.yaml
```

### Format d'un fichier dictionnaire

La clé de premier niveau est l'année `from` du bloc `history` concerné. Les sous-clés sont les champs textuels (`title`, `subtitle`, …).

```yaml
# i18n/la/nativitas_domini.yaml
1969:
  title: "In Nativitate Domini"

# i18n/fr/nativitas_domini.yaml
1969:
  title: "Nativité du Seigneur"
```

```yaml
# i18n/la/ioannis_pauli_ii.yaml
2011:
  title: "B. Ioannes Paulus II"
2014:
  title: "S. Ioannes Paulus II"
```

### Clé composite implicite

L'identité d'un label est la clé composite `{slug}.{from}.{field}` :

| Composant | Source | Exemple |
|-----------|--------|---------|
| `slug` | stem du fichier dictionnaire | `nativitas_domini` |
| `from` | clé de premier niveau YAML | `1969` |
| `field` | sous-clé | `title` |

→ Clé : `nativitas_domini.1969.title`

Cette clé est **implicite** — jamais déclarée explicitement. Elle est reconstruite par la Forge lors de la corrélation YAML ↔ dictionnaires (Étape 1bis).

### Invariants

- Pour chaque entrée `history[]` du corpus YAML dont `from = Y`, une clé `{slug}.Y.title` **doit** exister dans `i18n/la/`. Son absence est une erreur fatale V-I1.
- Un dictionnaire peut déclarer un `from` non présent dans le YAML correspondant → `ParseError::I18nOrphanKey { slug, lang, from }` (erreur fatale — indique une désynchronisation du corpus).
- Une langue autre que le latin peut omettre certaines clés : la Forge fusionne le latin en fallback AOT. La traduction partielle est admise, l'absence totale du latin ne l'est pas.
- Les fichiers dictionnaires utilisent le même stem que les fichiers YAML — la résolution est par correspondance de nom, pas par déclaration interne.

---

## 5. Hiérarchie et Scopes

### 5.1 Définition des Scopes

| Scope       | Description                                               | Champ `region`                      | Bits FeastID        |
| ----------- | --------------------------------------------------------- | ----------------------------------- | ------------------- |
| `universal` | Calendarium Generale Romanum                              | `null`                              | bits [15:14] = `00` |
| `national`  | Calendrier national approuvé par la Conférence épiscopale | Code ISO 3166-1 alpha-2 (ex: `FR`)  | bits [15:14] = `01` |
| `diocesan`  | Propre diocésain                                          | Identifiant diocésain (ex: `PARIS`) | bits [15:14] = `10` |

Le bit [15:14] est encodé dans les 2 bits supérieurs du FeastID u16. Le bit [13:12] encode la `category`. Les bits [11:0] encodent la séquence (4096 valeurs par (scope, category)).

### 5.2 Règles de Fusion

La Forge fusionne les fichiers YAML dans l'ordre de priorité suivant, du moins prioritaire au plus prioritaire :

```
universale/  <  nationalia/{ISO}/  <  dioecesana/{ID}/
```

**Règle de résolution des collisions (même slug) :**

Le scope le plus local l'emporte. Si `dioecesana/PARIS/sanctorale/nativitas_domini.yaml` et `nationalia/FR/sanctorale/nativitas_domini.yaml` définissent tous deux le slug `nativitas_domini`, la version diocésaine est retenue et la version nationale ignorée pour la génération du dataset Paris.

**Règle de résolution des collisions (même DOY, slugs différents) :**

Deux fêtes de scopes différents peuvent tomber le même jour. La Forge applique l'Étape 3 (Conflict Resolution) : la `Precedence` la plus haute (valeur numérique inférieure) détermine le `primary_id`. L'autre est placée en `secondary` si elle est une Memoria ou Commemoratio, sinon transférée ou supprimée selon les règles NALC 1969.

**Surcharge partielle :**

Un fichier national ou diocésain peut ne redéfinir que certains champs d'une fête universelle (ex: une `precedence` relevée). La Forge fusionne les blocs `history` — les entrées du scope local écrasent, pour les années couvertes, les entrées du scope universel. Les labels sont surchargés via le dictionnaire i18n du scope local (`i18n/la/` dans `nationalia/{ISO}/` ou `dioecesana/{ID}/`).

```yaml
# data/nationalia/FR/sanctorale/nativitas_domini.yaml
# slug = "nativitas_domini", scope = national, region = FR — tous déduits du chemin

version: 1
# Pas de date/mobile : héritée du fichier universale/sanctorale/nativitas_domini.yaml
category: 0
history:
  - from: 1969
    to: ~
    precedence: 1
    nature: sollemnitas
    color: albus
    # Surcharge du titre → i18n/fr/ dans nationalia/FR/ si elle existe,
    # sinon fallback i18n/la/ universel (fusion AOT)
```

### 5.3 Interface de Configuration de la Forge

Avec l'approche atomique, la Forge reçoit un répertoire racine (`corpus_root`) et une cible de compilation (`CompilationTarget`). Elle découvre elle-même les fichiers à ingérer en parcourant la hiérarchie de §1.2 dans l'ordre de §1.4.

```rust
// Interface Forge — configuration de la session de compilation
struct ForgeSession {
    corpus_root: PathBuf,          // Racine du corpus : contient universale/, nationalia/, dioecesana/
    target:      CompilationTarget, // Portée de la compilation
    output:      PathBuf,          // Chemin de sortie .kald
    range:       RangeInclusive<u16>, // Ex : 1969..=2399
}

enum CompilationTarget {
    Universal,                     // Calendarium Generale uniquement
    National  { region: String },  // Universal + national/{region}
    Diocesan  { region: String, diocese: String }, // Universal + national + dioecesana/{diocese}
}
```

**Algorithme de découverte des fichiers :**

```
fn discover_files(root: &Path, target: &CompilationTarget) -> Vec<PathBuf> {
    let mut files = vec![];

    // 1. Universale — toujours chargé
    collect_yaml_sorted(root / "universale" / "temporale",  &mut files);
    collect_yaml_sorted(root / "universale" / "sanctorale", &mut files);

    // 2. Nationale — si target est National ou Diocesan
    if let Some(region) = target.region() {
        collect_yaml_sorted(root / "nationalia" / region / "temporale",  &mut files);
        collect_yaml_sorted(root / "nationalia" / region / "sanctorale", &mut files);
    }

    // 3. Diocésaine — si target est Diocesan
    if let Some(diocese) = target.diocese() {
        collect_yaml_sorted(root / "dioecesana" / diocese / "temporale",  &mut files);
        collect_yaml_sorted(root / "dioecesana" / diocese / "sanctorale", &mut files);
    }

    files
}

fn collect_yaml_sorted(dir: &Path, out: &mut Vec<PathBuf>) {
    // fs::read_dir n'est pas ordonné — collecte puis tri lex. (INV-FORGE-1)
    if !dir.exists() { return; }
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension() == Some("yaml".as_ref()))
        .map(|e| e.path())
        .collect();
    entries.sort();
    out.extend(entries);
}
```

Un répertoire absent (ex: `nationalia/FR/temporale/` vide ou inexistant) n'est pas une erreur.

---

## 6. Référentiel des Valeurs Admises

### 6.1 `precedence` — Degrés Liturgiques Effectifs

Valeurs admises : **0 à 12 inclus**. Valeurs 13–15 : réservées système — la Forge rejette toute occurrence avec `RegistryError::InvalidPrecedenceValue(u8)`.

**Hiérarchie inverse : valeur plus faible = priorité plus haute.**

| Valeur | Niveau canonique (NALC 1969)                                | Bits flags [3:0] |
| ------ | ----------------------------------------------------------- | ---------------- |
| 0      | Triduum Sacrum                                              | `0000`           |
| 1      | Nativitas, Epiphania, Ascensio, Pentecostes                 | `0001`           |
| 2      | Dominicae Adventus, Quadragesimae, Paschales                | `0010`           |
| 3      | Feria IV Cinerum ; Hebdomada Sancta                         | `0011`           |
| 4      | Sollemnitates Domini, BMV, Sanctorum in Calendario Generali | `0100`           |
| 5      | Sollemnitates propriae                                      | `0101`           |
| 6      | Festa Domini in Calendario Generali                         | `0110`           |
| 7      | Dominicae per annum                                         | `0111`           |
| 8      | Festa BMV et Sanctorum in Calendario Generali               | `1000`           |
| 9      | Festa propria                                               | `1001`           |
| 10     | Feriae Adventus (17–24 Dec) ; Octava Nativitatis            | `1010`           |
| 11     | Memoriae obligatoriae                                       | `1011`           |
| 12     | Feriae per annum ; Memoriae ad libitum                      | `1100`           |
| 13–15  | **Réservés système** — non admissibles en YAML              | —                |

### 6.2 `nature` — Type Liturgique

Valeurs YAML (insensibles à la casse, normalisées par la Forge) et leur correspondance en `CalendarEntry.flags` bits [13:11] :

| Valeur YAML    | `Nature` Rust          | Valeur numérique | Note                                                       |
| -------------- | ---------------------- | ---------------- | ---------------------------------------------------------- |
| `sollemnitas`  | `Nature::Sollemnitas`  | 0                |                                                            |
| `festum`       | `Nature::Festum`       | 1                |                                                            |
| `memoria`      | `Nature::Memoria`      | 2                | Couvre Beatus/Beata — "beatus" n'est pas une Nature        |
| `feria`        | `Nature::Feria`        | 3                | Inclut les Dominicae (classe de Precedence, pas de Nature) |
| `commemoratio` | `Nature::Commemoratio` | 4                |                                                            |

Toute autre valeur → `RegistryError::UnknownNatureString(String)` avec hint :

```
"beatus" n'est pas une Nature. Utiliser nature: memoria.
```

### 6.3 `color` — Couleur Liturgique

Valeurs YAML (insensibles à la casse) et correspondance en `CalendarEntry.flags` bits [7:4] :

| Valeur YAML | `Color` Rust       | Valeur numérique | Usage canonique                                  |
| ----------- | ------------------ | ---------------- | ------------------------------------------------ |
| `albus`     | `Color::Albus`     | 0                | Fêtes du Seigneur, Vierge, Confesseurs, Docteurs |
| `rubeus`    | `Color::Rubeus`    | 1                | Passion, Apôtres, Martyrs, Pentecôte             |
| `viridis`   | `Color::Viridis`   | 2                | Temps ordinaire                                  |
| `violaceus` | `Color::Violaceus` | 3                | Avent, Carême                                    |
| `roseus`    | `Color::Roseus`    | 4                | Gaudete (Avent III), Laetare (Carême IV)         |
| `niger`     | `Color::Niger`     | 5                | Messes des défunts                               |

### 6.4 `season` — Saison Liturgique (optionnel)

Valeurs YAML et correspondance en `CalendarEntry.flags` bits [10:8] :

| Valeur YAML            | `Season` Rust                 | Valeur numérique |
| ---------------------- | ----------------------------- | ---------------- |
| `tempus_ordinarium`    | `Season::TempusOrdinarium`    | 0                |
| `tempus_adventus`      | `Season::TempusAdventus`      | 1                |
| `tempus_nativitatis`   | `Season::TempusNativitatis`   | 2                |
| `tempus_quadragesimae` | `Season::TempusQuadragesimae` | 3                |
| `triduum_paschale`     | `Season::TriduumPaschale`     | 4                |
| `tempus_paschale`      | `Season::TempusPaschale`      | 5                |
| `dies_sancti`          | `Season::DiesSancti`          | 6                |

Si omis dans le YAML, la Forge calcule la saison depuis les `SeasonBoundaries` de l'année courante (Étape 2). Pour les fêtes du Sanctoral dont la saison est sans ambiguïté (ex: une Mémoire en juillet → `tempus_ordinarium`), fournir le champ évite le recalcul et garantit la valeur indépendamment du calendrier temporel.

---

## 7. Mapping YAML ↔ `CalendarEntry`

Tableau de correspondance complet entre les champs YAML et les champs binaires de `CalendarEntry` (spec §3.3–3.4) :

| Champ YAML                        | Type YAML           | Destination binaire                | Offset | Note                                                             |
| --------------------------------- | ------------------- | ---------------------------------- | ------ | ---------------------------------------------------------------- |
| *(stem du nom de fichier)*        | —                   | —                                  | —      | Slug : clé FeastRegistry. Déduit du chemin. Absent du `.kald`.  |
| `id`                              | u16 \| null         | `CalendarEntry.primary_id`         | 0      | Alloué par la Forge si absent.                                   |
| `date.month` + `date.day`         | Integer             | DOY 0-based (formule §3.1)         | —      | Formule : `MONTH_STARTS[month-1] + day - 1`                      |
| `mobile.anchor` + `mobile.offset` | String + Integer    | DOY 0-based (Étape 2)              | —      | Résolution Pâques + offset                                       |
| `transfers[]`                     | Tableau             | —                                  | —      | Consommé exclusivement par la Passe 3 de l'Étape 3. Absent du `.kald`. |
| —                                 | —                   | `CalendarEntry.secondary_index`    | 2      | Alimenté par Étape 4 (Materialization)                           |
| `history[].precedence`            | Integer [0–12]      | `CalendarEntry.flags` bits [3:0]   | 4      |                                                                  |
| `history[].color`                 | String enum         | `CalendarEntry.flags` bits [7:4]   | 4      |                                                                  |
| `history[].season`                | String enum \| null | `CalendarEntry.flags` bits [10:8]  | 4      | Calculé si absent                                                |
| `history[].nature`                | String enum         | `CalendarEntry.flags` bits [13:11] | 4      |                                                                  |
| —                                 | —                   | `CalendarEntry.secondary_count`    | 6      | Alimenté par Étape 3 (Conflict Resolution)                       |
| *(clé `{slug}.{from}.{field}`)*   | —                   | Fichier `.lits`                    | —      | Clé implicite. Résolue via dictionnaires i18n (Étape 1bis). Absent du `.kald`. |
| `scope`                           | String enum         | FeastID bits [15:14]               | —      | `universal=00`, `national=01`, `diocesan=10`                     |
| `category`                        | Integer [0–3]       | FeastID bits [13:12]               | —      |                                                                  |

> **Rappel layout `CalendarEntry` (spec §3.3) :** `primary_id (u16, off 0)` | `secondary_index (u16, off 2)` | `flags (u16, off 4)` | `secondary_count (u8, off 6)` | `_reserved (u8, off 7)`. Les trois `u16` sont aux offsets pairs — alignement naturel garanti.

**Encodage `flags` :**

```rust
fn encode_flags(p: Precedence, c: Color, s: Season, n: Nature) -> u16 {
    (p as u16)             // bits [3:0]
    | ((c as u16) << 4)    // bits [7:4]
    | ((s as u16) << 8)    // bits [10:8]
    | ((n as u16) << 11)   // bits [13:11]
    // bits [15:14] = 0 (réservés, doivent être nuls)
}
```

---

## 8. Validations Forge (V1–V6)

Ces validations sont appliquées durant l'**Étape 1 (Rule Parsing)**. Tout échec est fatal — la Forge n'émet aucun binaire partiel.

### Groupe A — Syntaxe et Structure (V1)

**V1 — Validité syntaxique YAML et conformité au schéma**

```
∀ fichier f :
  f est syntaxiquement valide YAML
  ∧ f.version == 1
  ∧ exactement un de {date, mobile} est présent par feast
```

Violations → `ParseError::MalformedYaml`, `ParseError::UnsupportedSchemaVersion`, `ParseError::MissingTemporalityField { slug }`, `ParseError::AmbiguousTemporalityField { slug }`

### Groupe B — Unicité des Identifiants (V2)

**V2a — Unicité des Slugs par scope**

```
∀ (scope, fichier) : aucun slug n'apparaît plus d'une fois dans ce couple
```

Violation → `RegistryError::DuplicateSlug { slug, scope }`

**V2b — Unicité des FeastIDs**

```
∀ id explicitement fourni : aucun autre feast ne revendique le même FeastID
```

Violation → `RegistryError::FeastIDConflict { id, slug_a, slug_b }`

**V2c — Capacité FeastID (ex-V3 spec §10)**

```
∀ (scope, category) : allocated_count ≤ 4095
```

Violation → `RegistryError::FeastIDExhausted { scope, category }`

**V2d — Unicité temporelle dans un bloc `history` (ex-V1 spec §10)**

```
∀ slug s, ∀ scope sc, ∀ année y ∈ [1969, 2399] :
  |{ entrées e ∈ history(s, sc) | e.from ≤ y ≤ e.to_or_max }| ≤ 1
```

Violation → `RegistryError::TemporalOverlap { slug, year, conflicting_entries }`

### Groupe C — Intégrité des Dates (V3)

**V3a — Validité des dates fixes et des dates de transfert**

```
∀ feast avec date :
  month ∈ [1, 12]
  ∧ day ∈ [1, days_in_month(month)]  (29 fév admis, traitement §3.1)

∀ entrée t ∈ transfers avec t.date présent :
  t.date.month ∈ [1, 12]
  ∧ t.date.day ∈ [1, days_in_month(t.date.month)]
```

La validation de cohérence calendaire s'applique aussi aux dates de repli déclarées dans le bloc `transfers`. Une date de repli impossible (ex: `month: 2, day: 30`) est rejetée avec le même variant, le champ `slug` désignant la fête porteuse du bloc `transfers`.

Violation → `ParseError::InvalidDate { slug, month, day }`

**V3b — Cohérence des plages temporelles (ex-V4 spec §10)**

```
∀ entrée e ∈ history :
  e.from ≤ e.to_or_max
  ∧ e.from ≥ 1969
  ∧ e.to_or_max ≤ 2399
```

Violation → `RegistryError::InvalidTemporalRange { from, to }`

**V3c — Padding Entry (29 février)**

Invariant de la Forge, pas une erreur utilisateur. La Forge génère automatiquement la Padding Entry à `doy = 59` pour les années non-bissextiles. Aucune validation YAML n'est requise ; la Forge ne délègue pas ce slot à une fête déclarée pour `month: 2, day: 29` sur les années non-bissextiles.

### Groupe D — Domaines et Cycles (V4, V5, V6)

**V4 — Résolution des cycles de dépendances (dates mobiles)**

```
∀ feast avec mobile : anchor ∈ { "pascha", "adventus", "pentecostes" }
∧ aucun cycle dans le graphe de dépendances des ancres
```

Violation → `ParseError::UnknownAnchor { slug, anchor }`, `ParseError::CircularDependency { slug, anchor }`

**V5 — Nature conforme aux enums (ex-V5 spec §10)**

```
∀ entrée e ∈ history : e.nature ∈ { "sollemnitas", "festum", "memoria", "feria", "commemoratio" }
```

Violation → `RegistryError::UnknownNatureString(String)` avec hint si valeur canonique informelle détectée.

**V6 — Stem du nom de fichier syntaxiquement valide**

```
slug ∈ [a-z][a-z0-9_]*   (latin snake_case, premier char alphabétique)
```

La validation s'applique au **stem du nom de fichier** (`path.file_stem()`), avant toute désérialisation YAML. Tout fichier dont le stem ne satisfait pas ce pattern est rejeté immédiatement.

Violation → `ParseError::InvalidSlugSyntax(String)` — la `String` contient le stem fautif.

**V2-Bis — Domaine de Precedence (ex-V2 spec §10)**

```
∀ entrée e ∈ history : e.precedence ∈ [0, 12]
```

Violation → `RegistryError::InvalidPrecedenceValue(u8)` (valeurs 13–15 réservées système)

### Groupe E — Validité du Bloc `transfers` (V-T1, V-T2, V-T3, V-T4)

Ces validations sont appliquées durant l'**Étape 1 (Rule Parsing)**, lors de la désérialisation du bloc `transfers`. Tout échec est fatal.

**V-T1 — Exclusivité `offset` / `date` dans chaque entrée**

```
∀ entrée t ∈ transfers :
  (t.offset présent) XOR (t.date présent)
```

Une entrée qui déclare les deux clés (`offset` et `date`) est ambiguë. Une entrée qui n'en déclare aucune est sans effet et donc invalide.

Violations → `ParseError::TransferAmbiguous { slug, collides }` (les deux présents) · `ParseError::TransferEmpty { slug, collides }` (aucun)

**V-T2 — Résolvabilité de `collides` dans le `FeastRegistry`**

```
∀ entrée t ∈ transfers :
  t.collides ∈ slugs_connus_du_FeastRegistry
```

Le slug déclaré dans `collides` doit être présent dans le `FeastRegistry` au terme de l'Étape 1. Un slug inconnu (absent du corpus compilé, faute de frappe) constitue une règle morte — la Forge la traite comme une erreur fatale, pas un avertissement.

Violation → `ParseError::UnknownCollidesTarget { slug, collides }`

**V-T3 — Unicité de `collides` au sein du bloc**

```
∀ (t1, t2) ∈ transfers² : t1 ≠ t2 ⟹ t1.collides ≠ t2.collides
```

Deux entrées référençant le même slug concurrent produiraient un comportement non déterministe (laquelle appliquer ?). La première en position YAML n'a pas de sémantique canonique.

Violation → `ParseError::TransferDuplicateCollides { slug, collides }`

**V-T4 — Offset de transfert strictement positif**

```
∀ entrée t ∈ transfers avec t.offset présent :
  t.offset ≥ 1
```

Le transfert est, par invariant architectural, un déplacement **vers l'avant** uniquement. Un `offset = 0` serait un no-op (la fête resterait sur le slot de collision). Un `offset` négatif serait un déplacement vers le passé, incompatible avec la garantie de terminaison de la `TransferQueue` (transferts strictement croissants en DOY — spec §3.3 Passe 4). Le type YAML est désérialisé en `u32` — toute valeur YAML négative ou nulle est rejetée à la désérialisation.

Violation → `ParseError::TransferOffsetNotPositive { slug, collides, offset }`

### Groupe F — Corrélation YAML ↔ Dictionnaires i18n (V-I1, V-I2)

Ces validations sont appliquées durant l'**Étape 1bis (i18n Resolution)**. Elles opèrent après la construction complète du `FeastRegistry` — tous les slugs et toutes les plages `[from, to]` sont connus. Tout échec est fatal.

La nature de la validation change radicalement par rapport aux groupes A–E : on ne valide plus "la présence d'un champ texte dans le YAML" mais "la présence d'une clé d'indexation dans le dictionnaire externe". Le YAML est purement structurel — la Forge corrèle le graphe métier avec les dictionnaires.

**V-I1 — Présence obligatoire de la clé latine**

```
∀ (slug s, entrée e ∈ history(s)) :
  ∃ clé "{s}.{e.from}.title" ∈ i18n/la/{s}.yaml
```

Pour chaque entrée `history[]` du corpus YAML, la clé composite `{slug}.{from}.title` doit exister dans le dictionnaire latin correspondant. L'absence est une erreur fatale — le latin est la langue source, pas un optionnel.

Violation → `ParseError::I18nMissingLatinKey { slug, from, field }`

**V-I2 — Absence de clés orphelines dans les dictionnaires**

```
∀ clé "{s}.{y}.{field}" ∈ i18n/{lang}/{s}.yaml :
  ∃ entrée e ∈ history(s) avec e.from == y
```

Un dictionnaire ne peut contenir de clés dont l'année `from` est absente du bloc `history[]` correspondant. Une telle clé indique une désynchronisation entre le corpus YAML et les dictionnaires.

Violation → `ParseError::I18nOrphanKey { slug, lang, from, field }`

### Tableau de Correspondance Spec §10 ↔ Ce Document

Ce tableau est la clé de lecture bidirectionnelle entre les codes d'erreur Rust (spec §10) et les groupes de validation de ce document. Les codes V1–V6, V-T1–V-T4 et V-I1–V-I2 sont les seuls identifiants à utiliser dans le code et les messages d'erreur produits par la Forge.

| Code spec §10 | Variant Rust (`RegistryError` / `ParseError`)         | Groupe §8      | Libellé                                                         |
| ------------- | ----------------------------------------------------- | -------------- | --------------------------------------------------------------- |
| **V1**        | `TemporalOverlap { slug, year, conflicting_entries }` | **B — V2d**    | Unicité temporelle dans `history` (même slug/scope, même année) |
| **V2**        | `InvalidPrecedenceValue(u8)`                          | **D — V2-Bis** | Domaine de Precedence — valeurs 13–15 interdites                |
| **V3**        | `FeastIDExhausted { scope, category }`                | **B — V2c**    | Capacité FeastID ≤ 4095 par (scope, category)                   |
| **V4**        | `InvalidTemporalRange { from, to }`                   | **C — V3b**    | Cohérence et bornes des plages `[from, to]` dans `[1969, 2399]` |
| **V5**        | `UnknownNatureString(String)`                         | **D — V5**     | Nature conforme aux 5 enums admis                               |
| **V6**        | `InvalidSlugSyntax(String)`                           | **D — V6**     | Stem fichier : `[a-z][a-z0-9_]*` — validé avant parsing YAML   |

**Validations §8 sans code V-numéroté dans la spec** (erreurs structurelles de parsing) :

| Variant Rust                                              | Groupe §8    | Libellé                                                         |
| --------------------------------------------------------- | ------------ | --------------------------------------------------------------- |
| `ParseError::MalformedYaml` / `UnsupportedSchemaVersion`  | **A — V1**   | Syntaxe YAML invalide ou `version != 1`                         |
| `RegistryError::DuplicateSlug { slug, scope }`            | **B — V2a**  | Collision de slug dans le même scope (même stem, même répertoire scope) |
| `RegistryError::FeastIDConflict { id, slug_a, slug_b }`   | **B — V2b**  | Collision sur `id` explicite                                    |
| `ParseError::InvalidDate { slug, month, day }`            | **C — V3a**  | Date fixe impossible (ex: 30 février)                           |
| `ParseError::CircularDependency { slug, anchor }`         | **D — V4**   | Cycle dans le graphe des ancres mobiles                         |
| `ParseError::TransferAmbiguous { slug, collides }`        | **E — V-T1** | Entrée `transfers` avec `offset` et `date` simultanément        |
| `ParseError::TransferEmpty { slug, collides }`            | **E — V-T1** | Entrée `transfers` sans `offset` ni `date`                      |
| `ParseError::UnknownCollidesTarget { slug, collides }`    | **E — V-T2** | Slug `collides` absent du `FeastRegistry`                       |
| `ParseError::TransferDuplicateCollides { slug, collides }` | **E — V-T3** | Deux entrées `transfers` référencent le même concurrent         |
| `ParseError::TransferOffsetNotPositive { slug, collides, offset }` | **E — V-T4** | Offset de transfert nul ou négatif — déplacement vers l'avant obligatoire |
| `ParseError::I18nMissingLatinKey { slug, from, field }`   | **F — V-I1** | Clé `{slug}.{from}.{field}` absente du dictionnaire `i18n/la/` |
| `ParseError::I18nOrphanKey { slug, lang, from, field }`   | **F — V-I2** | Clé dictionnaire référençant un `from` absent du `history[]`    |

---

## 9. Exemples Complets

### 9.1 Solennité Fixe — Nativitas Domini

```yaml
# data/universale/sanctorale/nativitas_domini.yaml
# slug = "nativitas_domini" déduit du stem du nom de fichier

version: 1
category: 0
date:
  month: 12
  day: 25 # doy = 335 + 24 = 359 (toujours — DOY 0-based)

history:
  - from: 1969
    to: ~
    precedence: 1
    nature: sollemnitas
    color: albus
    season: tempus_nativitatis
    # title → i18n/la/nativitas_domini.yaml : { 1969: { title: "In Nativitate Domini" } }
```

**Résultat dans `CalendarEntry` pour le 25 décembre de n'importe quelle année :**

```
primary_id      : FeastID alloué pour nativitas_domini (ex: 0x0001)
secondary_count : 0  (aucune commémoration — Solennité évince tout)
flags           : 0x0201  (= 0b_0000_0010_0000_0001)
                  décomposé :
                  bits [3:0]   = 0001 → Precedence = 1 (SollemnitatesFixaeMaior)
                  bits [7:4]   = 0000 → Color      = 0 (Albus)
                  bits [10:8]  = 010  → Season     = 2 (TempusNativitatis)
                  bits [13:11] = 000  → Nature     = 0 (Sollemnitas)
                  bits [15:14] = 00   → réservés
```

Valeur `flags` numérique : `encode_flags(1, 0, 2, 0)` = `1 | (0 << 4) | (2 << 8) | (0 << 11)` = `0x0201`.

---

### 9.2 Fête Mobile — Ascensio Domini

```yaml
# data/universale/temporale/ascensio_domini.yaml
# slug = "ascensio_domini" déduit du stem du nom de fichier

version: 1
category: 0
mobile:
  anchor: pascha
  offset: +39

history:
  - from: 1969
    to: ~
    precedence: 1
    nature: sollemnitas
    color: albus
    season: tempus_paschale
    # title → i18n/la/ascensio_domini.yaml : { 1969: { title: "Ascensio Domini" } }
```

**Résolution en 2025 :** Pâques 2025 = `doy 110` (20 avril, DOY 0-based). Ascension = `doy 110 + 39 = 149` (29 mai 2025).

---

### 9.3 Fête avec Historique — Ioannes Paulus II

Voir §4.3 pour l'exemple complet avec béatification (2011–2013) et canonisation (2014–présent).

**Résolution de la Forge pour l'année 2012 :** `from=2011, to=2013` contient 2012 → `precedence = 11`, `nature = memoria`. Label via `ioannis_pauli_ii.2011.title` → `"B. Ioannes Paulus II"` (dict `i18n/la/`).

**Résolution pour l'année 2014 :** `from=2014, to=~` contient 2014 → `precedence = 12`, `nature = memoria`. Label via `ioannis_pauli_ii.2014.title` → `"S. Ioannes Paulus II"`.

**Résolution pour l'année 2009 :** aucune entrée ne contient 2009 → `Ok(None)` → fête absente du dataset 2009.

---

### 9.4 Surcharge Diocésaine

```yaml
# data/dioecesana/PARIS/sanctorale/dionysii_parisiensis.yaml
# slug = "dionysii_parisiensis", scope = diocesan, region = PARIS — tous déduits du chemin

version: 1
category: 2
date:
  month: 10
  day: 9 # doy = 274 + 8 = 282

history:
  - from: 1969
    to: ~
    precedence: 4
    nature: sollemnitas
    color: rubeus
    # title → i18n/la/dionysii_parisiensis.yaml (dans dioecesana/PARIS/i18n/)
```

**Comportement Forge :** lors d'une compilation `CompilationTarget::Diocesan { region: "FR", diocese: "PARIS" }`, la Forge découvre et ingère les fichiers dans l'ordre de §1.4 : universale, puis nationalia/FR, puis dioecesana/PARIS. Si le 9 octobre est également occupé par une fête nationale ou universelle de `Precedence` ≥ 4, la Forge applique Conflict Resolution (Étape 3) : la Solennité diocésaine (`Precedence = 4`) a la même valeur que les Solennités du calendrier général — la règle de scope local l'emporte à égalité de `Precedence`.

---

### 9.5 Dictionnaire i18n — Exemple Complet

Corpus YAML (graphe métier pur) et dictionnaires i18n correspondants pour `ioannis_pauli_ii` :

```yaml
# data/nationalia/PL/sanctorale/ioannis_pauli_ii.yaml  ← graphe métier
version: 1
category: 1
date:
  month: 10
  day: 22
history:
  - from: 2011
    to: 2013
    precedence: 11
    nature: memoria
    color: albus
  - from: 2014
    to: ~
    precedence: 12
    nature: memoria
    color: albus
```

```yaml
# i18n/la/ioannis_pauli_ii.yaml  ← source latine obligatoire
2011:
  title: "B. Ioannes Paulus II, pp."
2014:
  title: "S. Ioannes Paulus II, pp."
```

```yaml
# i18n/fr/ioannis_pauli_ii.yaml  ← traduction française (partielle admise)
2014:
  title: "Saint Jean-Paul II, pape"
  # 2011 absent : la Forge fusionne le latin en fallback AOT pour cette clé
```

**Clés implicites résolues par la Forge :**

| Clé composite              | Source  | Valeur résolue                   |
|----------------------------|---------|----------------------------------|
| `ioannis_pauli_ii.2011.title` | `la` | `"B. Ioannes Paulus II, pp."`    |
| `ioannis_pauli_ii.2014.title` | `la` | `"S. Ioannes Paulus II, pp."`    |
| `ioannis_pauli_ii.2011.title` | `fr` | `"B. Ioannes Paulus II, pp."` ← fallback latin |
| `ioannis_pauli_ii.2014.title` | `fr` | `"Saint Jean-Paul II, pape"`     |

---

## 10. Checklist de Conformité YAML

Avant de soumettre un fichier YAML à la Forge :

**Fichier YAML (graphe métier)**
- [ ] `version: 1` présent dans chaque fichier (`format_version` est **supprimé** — erreur fatale si présent)
- [ ] Le fichier est placé dans le répertoire correct selon son scope : `universale/`, `nationalia/{ISO}/`, `dioecesana/{ID}/`
- [ ] Le nom du fichier (stem) **est** le slug — aucune clé `slug` dans le corps YAML
- [ ] Le stem satisfait `[a-z][a-z0-9_]*` — latin snake_case, premier caractère alphabétique
- [ ] Le stem est **neutre** liturgiquement — aucun statut (saint, bienheureux…) encodé dans le nom
- [ ] Le fichier est dans `temporale/` si la fête est déclarée avec `mobile:`, dans `sanctorale/` si déclarée avec `date:`
- [ ] Chaque fête a exactement un bloc `date` ou `mobile` — jamais les deux
- [ ] **Aucun champ textuel** (`title`, `name`, `description`, …) dans aucun bloc `history[]` — le YAML est un graphe de données pur
- [ ] L'évolution de `precedence` ou de `nature` est portée par des entrées `history` distinctes
- [ ] Les plages `[from, to]` du bloc `history` sont disjointes pour un même slug/scope
- [ ] `precedence` ∈ [0, 12] pour chaque entrée `history` — jamais 13, 14 ou 15
- [ ] `nature` est l'une des 5 valeurs admises (§6.2) — aucun terme canonique informel
- [ ] `color` est l'une des 6 valeurs admises (§6.3)
- [ ] `from` ≥ 1969 et `to` ≤ 2399 pour toutes les entrées `history`
- [ ] `from` est renseigné explicitement si différent de 1969
- [ ] `id` absent sauf besoin documenté d'un identifiant stable
- [ ] Si bloc `transfers` présent : chaque entrée déclare `offset` **ou** `date`, jamais les deux, jamais aucun
- [ ] Si bloc `transfers` présent : chaque valeur de `collides` est un slug existant dans le corpus
- [ ] Si bloc `transfers` présent : chaque valeur de `collides` est unique dans le bloc
- [ ] Les fêtes au 29 février (`date.month: 2, date.day: 29`) sont intentionnelles — Padding Entry les années non-bissextiles

**Dictionnaire i18n (à livrer avec tout nouveau fichier YAML)**
- [ ] Un fichier `i18n/la/{slug}.yaml` existe pour chaque fichier YAML soumis
- [ ] Pour chaque entrée `history[]` avec `from: Y`, une clé `Y: { title: "…" }` est présente dans `i18n/la/{slug}.yaml`
- [ ] Aucune clé orpheline dans les dictionnaires : toute année `Y` déclarée dans un dictionnaire correspond à un `from: Y` dans le YAML correspondant
- [ ] Les dictionnaires d'autres langues peuvent être partiels — le fallback latin est géré par la Forge (fusion AOT)

---

**Fin du Contrat de Données Amont v1.3.1 — ❄️ GELÉ**

_Document créé le 2026-03-07. Révisé le 2026-04-09 (v1.3.1 — GELÉ). Modifications v1.2 : slug/version/transfers/Groupe E. Modifications v1.3 : zéro String YAML, dictionnaires i18n, Groupe F (V-I1, V-I2), §4.4, §9.5. Corrections v1.3.1 (contrat gelé) : V-T4 (offset uint strictement positif) ; V3a étendue aux dates de transfert ; exemple `offset` corrigé. Référence : `specification.md` v2.0.5._
