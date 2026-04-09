# 🏗️ Migration vers un modèle déclaratif absolu (Full YAML) pour le corpus liturgique

## 📅 Contexte

Durant la conception initiale (Jalon 1 / début Jalon 2), la résolution des anomalies liturgiques (notamment les collisions calendaires) reposait sur une approche hybride : les états de base étaient définis en YAML, tandis qu'une partie de la logique de résolution (les exceptions, les décalages de dates en cas de conflit) menaçait de s'infiltrer dans le code impératif Rust de la Forge. Cette approche violait l'invariant de séparation stricte Data/Logic propre au Data-Oriented Design (DOD) et introduisait une complexité algorithmique inutile dans le pipeline AOT.

## 🎯 Décision

Toutes les règles de calcul liturgique, incluant les comportements de repli lors des collisions, sont désormais strictement et intégralement encodées dans les données (fichiers YAML).
Le moteur Rust (la Forge) est réduit à un pur rôle de désérialisation, de validation structurelle et de projection binaire.

Les évolutions concrètes du schéma de données incluent :

1. **Délégation de l'identité au système de fichiers** : Suppression du champ `slug` dans le YAML. L'identifiant (FeastID) est déduit du nom du fichier (`file_stem()`), transformant une validation logicielle en contrainte structurelle.
2. **Introduction du bloc `transfers`** : Un graphe de résolution déclaratif dans le YAML (définissant la cible de la collision via `collides` et la géométrie du repli via `offset` ou `date`), éliminant les arbres de conditions `if/else` dans le parseur.

## ⚙️ Raisonnement (Invariants Architecturaux)

- **Pureté DOD (Data-Oriented Design)** : Le YAML devient l'unique source de vérité. Le code métier n'a plus à "connaître" l'histoire ou les exceptions de la liturgie, il ne manipule que des coordonnées temporelles et des poids (`precedence`).
- **Optimisation du Pipeline AOT** : La Forge ingère un layout plat et applique les règles de transfert géométriquement. Cela garantit un graphe de compilation déterministe en un seul passage, produisant une table binaire finale pré-calculée.
- **Sécurité de Typage (Zero-Cost)** : L'utilisation d'énumérations non-taggées (`#[serde(untagged)]`) en Rust permet un mapping direct depuis la structure déclarative YAML vers la mémoire, sans allocation intermédiaire pour des arbres de syntaxe complexes.
- **Alignement ECS** : Le runtime final (l'Engine) n'aura aucune logique de résolution à exécuter. Il se contentera d'itérer sur un tableau contigu de composants pré-résolus, maximisant la localité du cache CPU.

## 🚀 Conséquences et Impacts

- **Positifs** : Le code du parseur Rust est drastiquement réduit et devient agnostique des règles liturgiques. Le schéma des données est purifié (`version` au lieu de `format_version`, usage exclusif de `offset`). La complexité d'exécution au runtime est mathématiquement bornée (table lookup).
- **Négatifs** : Discontinuité du schéma (rupture de compatibilité avec les anciens YAML). Tout ajout d'une exception liturgique complexe nécessitera de trouver son expression sous forme de layout de données géométrique plutôt que d'ajouter une ligne de code, forçant une rigueur constante lors de la création des fiches YAML.
