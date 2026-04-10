# 🏗️ Document d'Architecture : Pipeline de Compilation Liturgique (AOT)

Ce document spécifie le flux de transformation des données, de la source de vérité (SOT) jusqu'à la projection en mémoire. L'architecture repose sur une séparation stricte : la complexité algorithmique est confinée au _build-time_ (Forge), garantissant un _runtime_ (Engine) à coût d'accès constant $O(1)$, sans allocation mémoire.

## 📥 1. Ingestion et Validation (Source of Truth)

L'entrée du pipeline est un graphe de données dénormalisé, conçu pour être lisible et éditable.

- **Topologie Pure (YAML) :** Les fichiers sources ne contiennent aucune chaîne de caractères (_Zero String_). Ils définissent uniquement des identifiants (`FeastID`), des ancres temporelles et des règles de transfert.
- **Dictionnaires (i18n) :** Les libellés sont isolés dans des fichiers distincts par langue.
- **Assertions de Forge (Validation Statique) :** Avant tout calcul, le graphe est validé contre un ensemble strict d'invariants (V1-V6 pour la structure, V-T1-V-T3 pour les transferts). Tout échec (ex: graphe cyclique, dictionnaire orphelin) interrompt la compilation.

## ⚙️ 2. Résolution et Canonicalisation (Heavy Lifting)

La Forge exécute la logique métier pour générer un état déterministe sur une plage temporelle fixe (ex: 1969-2399).

- **Résolution des Ancres Mobiles :** Exécution AOT stricte selon un ordre garanti sans dépendances cycliques. Les ancres acycliques (`nativitas`, `epiphania`, `adventus`) sont résolues en $O(1)$ avant le calcul du graphe pascal (Meeus/Jones/Butcher).
- **Résolution des Collisions (Transfers) :** La Forge simule les superpositions sur chaque jour de la plage cible. Les règles de préséance sont appliquées pour matérialiser l'état final. L'état canonique d'un jour devient une constante absolue.
- **Fusion LITS (i18n) :** Corrélation spatio-temporelle entre les `FeastID` et les dictionnaires. La Forge résout les manques en appliquant le _fallback_ canonique (latin), éliminant tout branchement conditionnel lié à la langue au _runtime_.

## 📦 3. Matérialisation et Data Layout (Binary Packing)

L'état résolu est sérialisé dans des formats binaires conçus exclusivement pour la localité spatiale en mémoire (optimisation Cache L1/L2).

- **Le Fichier de Structure (`.kald`) :** * **Data Layout :** Tableau plat continu. Chaque entrée (*slot*) possède un *stride\* fixe de 64 bits (8 octets).
  - **Composition du Stride :** `[ FeastID (u32) | Flags/LiturgicalPeriod (u16) | Metadata (u16) ]`.
  - **Topologie :** Les jours manquants (ex: 29 février en année non-bissextile) sont remplis par des _Padding Entries_.
- **Le Fichier de Dictionnaire (`.lits`) :**
  - **Architecture Tripartite :** Séparation en trois blocs contigus : _Index Array_ (offsets fixes) -> _Length Array_ -> _Data Blob_ (textes concaténés).
  - **Empreinte :** Le dictionnaire embarque un _hash_ cryptographique liant sa validité à un fichier `.kald` spécifique.

## 🚀 4. Projection Runtime (The Engine)

L'Engine est une bibliothèque purement mécanique (`#![no_std]`, `no_alloc`). Il ignore la sémantique liturgique et se comporte comme un projecteur de mémoire.

- **Indexation Absolue :** L'accès à un jour donné ne nécessite aucune itération. L'Engine calcule l'offset via une arithmétique simple : `index = (année - 1969) * 366 + DOY`.
- **Extraction $O(1)$ :** La lecture des 8 octets à l'offset calculé charge l'intégralité de l'état du jour dans les registres CPU.
- **Projection Sémantique (Bitwise) :** Les états opérationnels (comme `LiturgicalPeriod`) sont extraits via un masquage binaire (`(flags >> 8) & 0x0007`). Aucun branchement `if/else` n'est nécessaire pour déterminer si un jour est en Carême ou dans la Semaine Sainte.
- **Résolution LITS :** La récupération d'une chaîne de caractères s'effectue en trois accès mémoire contigus (Index -> Length -> Slice du Blob), garantissant un affichage instantané et sécurisé (bounds checking statique).
