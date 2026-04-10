# 🧪 Stratégie de Validation et d’Intégrité Binaire

Dans une architecture AOT où la logique est "pétrifiée" dans la donnée, la qualité ne se mesure pas par la couverture de code (Code Coverage) du runtime, mais par l'intégrité et la cohérence des artefacts produits. La stratégie repose sur un triple verrouillage par checksums.

## 1. Intégrité de l'Artefact (SHA-256)

Chaque fichier `.kald` généré par la Forge inclut, dans son header, une signature SHA-256 de l'intégralité du dataset.

- **Objectif :** Détecter toute corruption de donnée entre la phase de forge et l'utilisation (transport, stockage disque).
- **Implémentation Engine :** L'Engine effectue une vérification en streaming lors du chargement. Si le checksum calculé ne correspond pas au checksum embarqué, le moteur refuse l'exécution. Cette opération est réalisée sans allocation mémoire (`no_alloc`).

## 2. Synchronisation Structurelle (LITS-Sync Hash)

Le système i18n (`.lits`) et le système de structure (`.kald`) sont intrinsèquement liés par le `FeastID`.

- **Le Verrou :** La Forge génère un `content_hash` unique basé sur la topologie du calendrier au moment de la compilation. Ce hash est injecté dans les deux fichiers.
- **Validation :** Au runtime, l'Engine compare les hashs de synchronisation.
  - _Succès :_ Les offsets du dictionnaire correspondent exactement aux identifiants de la structure.
  - _Échec :_ Empêche l'affichage de textes décalés ou erronés suite à une mise à jour partielle des fichiers.

## 3. Tests de Non-Régression par "Binary Diffing"

La stabilité du système est garantie par la comparaison binaire entre les versions de production et les nouvelles versions candidates.

- **Processus :**
  1.  Génération d'un binaire de référence (Gold Master) sur une plage de 400 ans.
  2.  Modification de la Forge (optimisation de l'algorithme de transfert ou ajout d'ancres).
  3.  Génération du nouveau binaire.
  4.  **Bitwise Comparison :** Si la modification est purement algorithmique (refactoring), le binaire de sortie doit être identique au bit près.
- **Analyse de changement :** Si une évolution métier est attendue (ex: ajout d'une fête), l'outil de test isole les strides modifiés. Toute modification hors de la plage d'impact définie est considérée comme une régression structurelle.

## 4. Validation des Invariants au Runtime (Assertions "Zero-Cost")

Bien que la Forge valide tout en amont, l'Engine intègre des assertions de sécurité minimales pour protéger la mémoire.

- **Bounds Checking :** L'accès via l'index temporel `(année - 1969) * 366 + DOY` est systématiquement validé contre la taille du fichier mappé.
- **Magic Bytes :** Chaque fichier commence par un header d'identification (`0x4B414C44` pour KALD). Un échec de lecture des Magic Bytes interrompt immédiatement le pipeline de projection, protégeant l'application contre le chargement de fichiers incompatibles.

Cette stratégie déplace la confiance du code vers la donnée : si le checksum est valide et que le hash de synchronisation correspond, le comportement du moteur est garanti par construction.
