# Épiphanie : Faille dans le contrat de données (v1.7.0)

Pour l'Épiphanie en France (transférée au dimanche entre le 2 et le 8 janvier), **la spécification actuelle ne contient aucune ancre primitive capable de résoudre cette date.**

* L'ancre `epiphania` définie au §3.2 calcule le "premier dimanche *strictement postérieur* au 6 janvier" (DOY garanti [6, 12]). Elle est conçue pour le **Baptême du Seigneur**, pas pour l'Épiphanie elle-même.
* Si vous utilisez `anchor: epiphania` pour l'Épiphanie française, le calcul sera faux (ex: si le 6 janvier est un mardi, l'ancre `epiphania` renverra le dimanche 11, alors que l'Épiphanie en France doit tomber le dimanche 4).

**Action requise sur la spécification :**
Pour maintenir le pipeline déterministe en O(1), vous devez amender la spécification (ajouter une ancre primitive dans la table §3.2 du parseur Rust).

**Proposition d'amendement (à intégrer dans votre moteur Rust à l'Étape 2) :**
```rust
// Nouvelle primitive : epiphania_transferrata (ou nom équivalent)
fn resolve_epiphania_transferrata(year: u16) -> u16 {
    let jan_2_doy = doy_from_date(1, 2); // doy = 1
    let jan_2_weekday = weekday(year, jan_2_doy);
    let days_to_sunday = if jan_2_weekday == 0 { 0 } else { 7 - jan_2_weekday };
    jan_2_doy + days_to_sunday as u16 // Renvoie le DOY du dimanche dans [Jan 2, Jan 8]
}
```

Une fois cette primitive ajoutée au parseur (et à la table des enums autorisés par V4a), le fichier YAML en France sera :

**Fichier : `mobile/in_epiphania_domini.yaml`**
```yaml
version: 1
category: 0
mobile:
  anchor: epiphania_transferrata # ou le nom que vous choisirez pour l'ancre
  offset: 0
```

