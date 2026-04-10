# Note d'Attention Architecturale — Borne `from: 1969` et Portée Temporelle du Corpus

**Type** : Note architecturale (non-ADR — invariant de conception à documenter)  
**Date** : 2026-04-10  
**Contexte** : `liturgical-calendar-forge` — §0 et §4 du contrat / plage [1969, 2399]

---

## L'invariant

La borne inférieure admise pour tout champ `from` dans un bloc `history` est **1969**. La plage de compilation supportée par la Forge est [1969, 2399]. Tout `from < 1969` est une erreur fatale.

---

## Pourquoi 1969

1969 est l'année d'entrée en vigueur du *Calendarium Romanum Generale* réformé, promulgué par la Constitution apostolique *Mysterii Paschalis* de Paul VI (14 février 1969). Ce calendrier constitue la **source de vérité liturgique du corpus** — le Missel Romain de 1970 (édition typique) en est la première application.

Modéliser des fêtes antérieures à 1969 (calendrier tridentin, calendrier préréformé) dépasserait le scope déclaré du corpus et introduirait une hétérogénéité structurelle non gérée : des fêtes supprimées en 1969, des rangs incompatibles avec la table de `precedence` NALC 1969, des couleurs liturgiques différentes.

---

## Implications pour le corpus actuel

### Fêtes à `from: 1969` implicite

La majorité des fêtes du corpus n'ont pas de bloc `history` multi-entrées : leur `from: 1969` signifie "présente depuis la réforme de 1969 sans modification structurelle connue". Ce n'est pas une affirmation que la fête n'existait pas avant 1969 — c'est une affirmation que **dans le scope de ce corpus**, son histoire commence en 1969.

### Fêtes supprimées entre 1969 et aujourd'hui

Certaines fêtes présentes dans le corpus pourraient avoir été supprimées du Calendarium Romanum Generale après 1969. Le corpus actuel ne modélise pas les suppressions (absence de `to` fermant). Si une fête doit être déclarée supprimée à partir d'une année Y, son dernier bloc `history` doit recevoir `to: Y`. L'absence d'entrée pour les années postérieures à Y est correcte — `resolve_feast_for_year` retourne `Ok(None)`.

**Ce pattern n'a pas encore été utilisé dans le corpus généré.** Sa nécessité éventuelle est à identifier lors de la relecture liturgique.

### Fêtes créées après 1969 et avant 2026

Les annotations `# YYYY (creation)` dans la liste source couvrent la plage 1980–2021. Ces fêtes ont un `from: YYYY` et sont absentes du dataset pour les années antérieures à YYYY. C'est le comportement correct — `resolve_feast_for_year` retourne `Ok(None)` pour Y < YYYY.

---

## Point de vigilance : fêtes présentes avant 1969 avec rang modifié en 1969

Certaines fêtes existaient dans le calendrier pré-réformé avec un rang différent. Le corpus modélise uniquement le rang post-1969. Si une étude liturgique ultérieure souhaite modéliser la transition (rang pré-1969 → rang 1969), la borne inférieure de la plage devrait être étendue en dessous de 1969 — ce qui est explicitement hors scope et nécessiterait une révision du contrat.

**Recommandation** : documenter ce scope limit dans le README du corpus pour éviter des contributions hors périmètre.

---

## La borne supérieure 2399

La plage s'étend jusqu'à 2399 pour couvrir plusieurs siècles de planification liturgique sans intervention humaine sur le corpus. Cette borne est arbitraire mais suffisamment large. Elle implique que les algorithmes de calcul (Meeus/Jones/Butcher pour Pâques, résolution des ancres) doivent être validés sur toute la plage — pas seulement sur les années courantes.

**Point d'attention** : l'algorithme de Meeus/Jones/Butcher est valide pour toute année grégorienne, mais des vérifications de cohérence sur les années limites (1969, 2399) sont recommandées comme tests de régression AOT.

---

## Références

- `liturgical-scheme.md` v1.3.3 — §0 : invariants absolus, plage [1969, 2399]
- `liturgical-scheme.md` v1.3.3 — §4.1 : sémantique de `from` et `to`
- `specification.md` v2.2 — Étape 2 : algorithme de Meeus/Jones/Butcher
