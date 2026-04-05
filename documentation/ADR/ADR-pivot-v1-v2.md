# ADR : Pivot vers l'Architecture AOT-Only (v1.x -> v2.0)

**Statut** : Adopté / Remplace l'architecture hybride v1.x

**Contexte** : Passage d'un moteur de calcul dynamique à un projecteur de données statiques

**Date** : 08 Mars 2026

## 1. Le Changement de Paradigme

Le pivot de la v1.0 vers la v2.0 ne repose pas sur une correction technique, mais sur la reconnaissance d'un **invariant fondamental du domaine** :

> **La donnée liturgique n'est pas une loi physique, c'est une vue matérialisée d'un état du droit à un instant T.**

Vouloir coder un "algorithme perpétuel" pour la liturgie est une erreur conceptuelle. Les règles (saints, préséances, calendriers) sont issues de décrets humains et historiques. En v1.0, l'Engine tentait de simuler ce droit au runtime (Logic-Heavy). En v2.0, nous passons à une approche de **Compilation Liturgique** : la Forge compile le droit, l'Engine ne fait que lire le résultat.

## 2. Problématiques de l'Architecture v1.x (Hybride)

1. **Indirection Cognitive** : L'Engine devait "connaître" les règles de Pâques et les tables de préséance, rendant le code complexe et difficile à porter.
2. **Branches Coûteuses** : Le calcul dynamique imposait des `if/else` systématiques (années bissextiles, dates mobiles), dégradant la prédiction de branchement du CPU.
3. **Divergence de Calcul** : Multiplier les implémentations de l'Engine (Rust, Swift, C) augmentait le risque de voir apparaître des différences de calcul sur une même date.

## 3. La Décision : "Data over Logic"

Nous avons décidé de déporter **100% de l'intelligence métier** dans la Forge amont.

### Le Nouveau Modèle :

- **The Forge (Producer)** : Ingeste le YAML, résout les conflits, calcule les dates de Pâques sur 430 ans et génère un dataset binaire `.kald`.
- **The Engine (Consumer)** : Réduit à sa plus simple expression technique. C'est un projecteur $O(1)$ sans aucune connaissance de la liturgie.

## 4. Invariants Techniques v2.0 (DOD/AOT)

L'adoption du **Data-Oriented Design** a permis d'introduire des optimisations impossibles en v1.0 :

### 4.1 L'Index 59 Fixe (Padding Entry)

Pour éliminer les branches liées aux années bissextiles, chaque année dans le binaire occupe strictement **366 slots**.

- Le 29 février est **toujours** à l'index 59.
- Les années non-bissextiles utilisent cet index comme un slot de "padding" (`primary_id = 0`).
- **Impact** : L'arithmétique de pointeur devient triviale et constante.

### 4.2 Stride 64-bit et Alignement

Chaque `CalendarEntry` pèse **8 octets**. Les types `u16` (`primary_id`, `flags`, `secondary_index`) sont alignés sur des offsets pairs (0, 2, 4).

- **Impact** : Accès mémoire atomique, aligné, et compatible avec la vectorisation (SIMD).

### 4.3 Intégrité Cryptographique

Le fichier `.kald` contient un SHA-256 du dataset. L'Engine valide ce checksum en streaming (`no_alloc`).

- **Impact** : Garantie que le "Droit Liturgique" matérialisé n'a pas été corrompu entre la Forge et l'utilisateur.

## 5. Conséquences

### Positives :

- **Performance Absolue** : Accès $O(1)$. Aucune itération, aucun calcul de date mobile au runtime.
- **Portabilité Totale** : L'Engine `no_std` / `no_alloc` peut être intégré n'importe où (systèmes embarqués, WASM, drivers).
- **Découplage** : On peut mettre à jour les règles liturgiques (ex: nouveau saint canonisé) en republiant un fichier `.kald` sans jamais toucher une ligne de code de l'Engine.

### Négatives / Risques :

- **Coût de Stockage** : Le fichier binaire est plus lourd qu'un script de règles (~1,2 Mo pour 400 ans), ce qui est négligeable sur les systèmes modernes.
- **Plage Temporelle Fixe** : Le système ne fonctionne que sur la plage compilée (1969–2399). Au-delà, un nouveau dataset est nécessaire.

## 6. Conclusion

L'intelligence est désormais dans la **Forge**, la performance est dans l'**Engine**. En passant du logiciel au produit de données, nous avons atteint le niveau de prévisibilité et de robustesse requis pour un système de référence.

---

_Document archivé et intégré au dossier technique de la v2.0._
