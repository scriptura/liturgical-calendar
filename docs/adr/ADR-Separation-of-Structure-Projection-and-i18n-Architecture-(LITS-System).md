# ADR-004 : Séparation Structure/Projection et Architecture i18n (Système LITS)

**Date :** 9 Avril 2026
**Statut :** Validé et Verrouillé
**Auteurs :** Architecte Principal & IA (Gemini)

## 1. Contexte et Problématique

Le moteur liturgique (`liturgical-calendar-core`) est conçu sous de strictes contraintes **DOD (Data-Oriented Design)**, AOT (Ahead-of-Time) et `#![no_std]` / `no_alloc`. L'objectif est un temps d'exécution en $O(1)$ avec une empreinte cache optimale.

L'intégration de l'internationalisation (i18n) et de l'historique des libellés (les noms des fêtes qui changent selon les années) menaçait ces invariants. Les chaînes de caractères brisent naturellement la localité mémoire (tailles variables) et la gestion des langues ou des fallbacks (ex: latin par défaut si le français manque) introduit des branchements conditionnels coûteux au runtime.

## 2. Alternatives Envisagées

Plusieurs modèles ont été étudiés pour intégrer les labels i18n :

- **Option A (Couplage Interne) :** Stocker des offsets de strings directement dans le fichier binaire principal `.kald`.
  - _Rejet :_ Détruit l'invariant d'un _stride_ fixe de 8 octets pour les données topologiques. Introduit des métadonnées linguistiques dans le moteur de calcul.
- **Option B (Fallback au Runtime) :** L'UI charge le français, et l'Engine implémente une logique "si non trouvé, chercher dans le dictionnaire latin".
  - _Rejet :_ Force l'Engine à posséder une logique métier et à gérer de multiples buffers simultanément (double FFI call, cache miss).
- **Option C (Format Dictionnaire Classique) :** Une table de hachage ou un arbre de recherche dans un fichier externe.
  - _Rejet :_ Parsing dynamique requis, perte du $O(1)$ déterministe.
- **Option D (La solution retenue - Projection ECS/AOT) :** Séparation stricte entre le Modèle (le `.kald`) et la Vue (le `.lits`), avec résolution AOT de toute la complexité.

## 3. La Décision Architecturale : Le système LITS

Nous avons opté pour le modèle d'une **Projection Immuable Dense (Option D)**. L'architecture traite l'i18n non pas comme une donnée de base, mais comme un calque de présentation généré à la compilation (AOT) par la Forge.

### 3.1. Invariants de la Forge (AOT)

1.  **Zéro String dans le modèle de données :** Le YAML source ne contient plus de texte. L'identité i18n est une clé composite déduite : `{slug}.{from}.{field}`.
2.  **Fusion du Fallback :** La Forge résout les traductions manquantes à la compilation. Un fichier `fr.lits` contient les pointeurs vers les chaînes françaises, mais _aussi_ vers les chaînes latines si la traduction manque. L'Engine n'a donc jamais de "trou" à gérer au runtime.

### 3.2. Layout Mémoire Bit-Perfect (`.lits`)

Le fichier `.lits` (Language Index Table System) est structuré pour maximiser la prédictibilité du CPU :

- **Header (64 bytes) :** Aligné sur une ligne de cache (cache-line). Contient le _Build Hash_ (SHA-256 complet du `.kald` parent) pour garantir le couplage fort et empêcher les désynchronisations au runtime.
- **Index Array (`[u32; 65536]`) :** Accès direct en $O(1)$ via le `FeastID`.
- **MSB Tagging (Branchement CPU) :** Le bit de poids fort (MSB) du `u32` dicte la nature du pointeur :
  - `offset == 0` : Sentinelle stricte (Pas de label existant).
  - `MSB == 0` : Fast-path (99% des cas). Les 31 bits restants pointent vers l'offset absolu de la chaîne.
  - `MSB == 1` : Slow-path historique. Les 31 bits pointent vers un `VersionBlock`.
- **VersionBlock et Padding :** Aligné rigoureusement sur 8 octets pour la sécurité cross-platform (WASM/x86). Utilisation d'un `count: u16` explicite plutôt qu'un terminateur `0` pour garantir la viabilité des slices (Bounds Checking) en Rust `unsafe`.
- **String Pool :** Chaînes UTF-8 contiguës, préfixées par un `u16` de longueur (évite le coût d'un `strlen` O(N) au runtime).

### 3.3. Interface FFI Zéro-Copie

L'Engine est purement `stateless`. Il retourne des slices (`*const u8`, `len`) pointant directement dans la zone mémoire mappée par l'appelant. Chaque accès est précédé d'un contrôle mathématique anti-overflow (`size <= len - offset`) pour garantir l'absence d'Undefined Behavior (UB).

## 4. Conséquences et Arbitrages

### Ce que nous gagnons :

- **Performance absolue :** Le chemin nominal est résolu en 3 accès mémoire contigus (Index -> Length -> Data). Zéro itération, plein cache L1.
- **Isolation :** L'Engine (Core) reste totalement agnostique à la langue et à l'historique des modifications textuelles.
- **Sécurité :** L'empreinte binaire (`content_hash`) bloque toute tentative de chargement d'un dictionnaire obsolète vis-à-vis de la topologie liturgique.

### Ce que nous payons (Trade-offs) :

- **Empreinte mémoire fixe :** L'Index Array consomme 256 Ko en permanence, même pour un calendrier très peu peuplé. _Arbitrage : C'est le prix à payer pour l'accès O(1). Pour des systèmes modernes, 256 Ko est négligeable._
- **Complexité de la Forge :** Le compilateur (côté `std`) devient beaucoup plus complexe, car il doit simuler les fallbacks, gérer le `registry.lock`, et packager la mémoire de manière parfaitement alignée. _Arbitrage : C'est la philosophie AOT. Déplacer la complexité du runtime vers le build-time._

## 5. Insight Architectural

Le modèle final est isomorphique au pattern ECS (Entity-Component-System) utilisé dans l'industrie du jeu vidéo AAA :

- Le **FeastID** est l'Entité.
- Le **`.kald`** stocke les composants de logique (Immuable, Dense).
- Le **`.lits`** est le système de rendu/projection (Overlay, Sparse).

---

Document rédigé le 9 avril 2026
