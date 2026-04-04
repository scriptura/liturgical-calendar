# 📅 Liturgical Calendar

Un moteur de référence haute performance pour le calcul et la consultation du calendrier liturgique catholique (Novus Ordo).

Ce projet repose sur un changement de paradigme radical : la donnée liturgique n'est pas traitée comme un algorithme perpétuel, mais comme une vue matérialisée d'un état du droit à un instant T.

## 🚀 Philosophie : "Data over Logic"

Vouloir calculer dynamiquement le calendrier (dates de Pâques, préséances complexes, transferts de fêtes) à l'exécution entraîne une complexité technique inutile et des risques d'incohérence entre les plateformes.

Pour résoudre cela, l'intelligence métier a été entièrement déportée en amont. Le système se divise en deux composants asymétriques :

1. **La Forge (Le Compilateur)** : Elle ingère les règles liturgiques lisibles par l'homme (fichiers YAML), calcule les dates mobiles sur plusieurs siècles, résout les conflits calendaires, et génère un dataset binaire statique et cryptographiquement vérifié (`.kald`).
2. **L'Engine (Le Projecteur)** : C'est le runtime ultra-léger intégré dans vos applications. Il est réduit à sa plus simple expression technique : il ne contient aucune règle de liturgie, il se contente de lire le résultat binaire pré-calculé.

## ⚡ Spécificités Techniques

Cette séparation stricte offre des garanties de performance et de stabilité exceptionnelles :

- **Zéro calcul à l'exécution** : L'accès à n'importe quel jour de l'année se fait en un temps constant ultra-rapide (O(1)). L'Engine lit simplement une table continue, éliminant les branchements conditionnels (`if/else`) liés aux années bissextiles ou aux dates flottantes.
- **Le Binaire comme Table Maître** : Le fichier `.kald` agit comme l'unique source de vérité. Une mise à jour des règles liturgiques (comme la canonisation d'un saint) nécessite simplement de fournir un nouveau fichier, sans jamais toucher ni recompiler le code de l'Engine.
- **Empreinte mémoire minimale** : L'Engine fonctionne sans allocation mémoire dynamique (`no_alloc`) et sans recourir à la bibliothèque standard du système (`no_std`).
- **Portabilité universelle** : Conçu avec une interface C native (FFI), le moteur peut être embarqué partout : serveurs, applications mobiles iOS/Android, systèmes embarqués ou directement dans le navigateur via WebAssembly.

## ⏱️ Couverture Temporelle

Le calendrier pré-compilé couvre de manière exhaustive et déterministe la période allant de **1969** (réforme du calendrier romain) à **2399**.

## 🛠️ Utilisation et Intégration

_[à venir...]_
