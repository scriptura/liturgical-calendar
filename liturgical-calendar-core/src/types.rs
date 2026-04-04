// types.rs — Types de domaine canoniques de l'Engine (specification.md §4).
//
// Tous les enums sont #[repr(u8)] — représentation binaire exacte garantie.
// Valeurs numériques figées depuis v1.0, inchangées en v2.0.
// DomainError : Copy, pas de String, primitifs uniquement (INV-W1).

use core::fmt;

// ─── Erreur de conversion depuis un discriminant hors domaine ─────────────────

/// Erreur produite par `try_from_u8` quand la valeur dépasse le domaine défini.
///
/// Champs primitifs uniquement — compatible `no_alloc` (INV-W1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainError {
    /// Identifiant du type : 0=Precedence, 1=Nature, 2=Color, 3=Season.
    pub type_tag: u8,
    /// Valeur u8 hors domaine reçue.
    pub value: u8,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DomainError(type_tag={}, value={})",
            self.type_tag, self.value
        )
    }
}

// ─── 4.1 Precedence ───────────────────────────────────────────────────────────

/// Rang de précédence liturgique (specification.md §4.1).
///
/// Axe ordinal de résolution de collision. **Valeur numérique inverse** :
/// valeur plus faible = priorité plus haute. Comparaison entière pure.
///
/// _Tabella dierum liturgicorum — NALC 1969. Ordre figé. Aucune modification autorisée._
///
/// Valeurs 13–15 réservées système : `try_from_u8` retourne `Err` pour ces valeurs.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precedence {
    /// Triduum Pascal — priorité absolue.
    TriduumSacrum = 0,
    /// Solennités fixes majeures (Noël, Épiphanie, etc.).
    SollemnitatesFixaeMaior = 1,
    /// Dimanches privilégiés majeurs (Rameaux, Pâques, Pentecôte).
    DominicaePrivilegiataeMaior = 2,
    /// Jours fériés privilégiés majeurs (Cendres, Semaine Sainte).
    FeriaePrivilegiataeMaior = 3,
    /// Solennités générales du calendrier universel.
    SollemnitatesGenerales = 4,
    /// Solennités propres (diocésaines, nationales).
    SollemnitatesPropria = 5,
    /// Fêtes du Seigneur.
    FestaDomini = 6,
    /// Dimanches per annum.
    DominicaePerAnnum = 7,
    /// Fêtes de la Vierge et des Saints du calendrier général.
    FestaBMVEtSanctorumGenerales = 8,
    /// Fêtes propres.
    FestaPropria = 9,
    /// Fériés de l'Avent et de l'Octave de Noël.
    FeriaeAdventusEtOctavaNativitatis = 10,
    /// Mémoires obligatoires.
    MemoriaeObligatoriae = 11,
    /// Fériés per annum et mémoires ad libitum.
    FeriaePerAnnumEtMemoriaeAdLibitum = 12,
    // 13–15 : réservés système — `try_from_u8` retourne Err.
}

impl Precedence {
    /// Convertit un `u8` en `Precedence`.
    ///
    /// Retourne `Err` pour les valeurs 13–15 (réservées) et 16–255 (hors domaine).
    ///
    /// # Exemple
    /// ```
    /// # use liturgical_calendar_core::types::Precedence;
    /// assert_eq!(Precedence::try_from_u8(0), Ok(Precedence::TriduumSacrum));
    /// assert!(Precedence::try_from_u8(13).is_err()); // réservé
    /// ```
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Precedence::TriduumSacrum),
            1 => Ok(Precedence::SollemnitatesFixaeMaior),
            2 => Ok(Precedence::DominicaePrivilegiataeMaior),
            3 => Ok(Precedence::FeriaePrivilegiataeMaior),
            4 => Ok(Precedence::SollemnitatesGenerales),
            5 => Ok(Precedence::SollemnitatesPropria),
            6 => Ok(Precedence::FestaDomini),
            7 => Ok(Precedence::DominicaePerAnnum),
            8 => Ok(Precedence::FestaBMVEtSanctorumGenerales),
            9 => Ok(Precedence::FestaPropria),
            10 => Ok(Precedence::FeriaeAdventusEtOctavaNativitatis),
            11 => Ok(Precedence::MemoriaeObligatoriae),
            12 => Ok(Precedence::FeriaePerAnnumEtMemoriaeAdLibitum),
            _ => Err(DomainError {
                type_tag: 0,
                value: val,
            }),
        }
    }
}

// ─── 4.2 Nature ───────────────────────────────────────────────────────────────

/// Nature liturgique de la célébration (specification.md §4.2).
///
/// Axe sémantique. **La Nature ne dicte jamais la force d'éviction** :
/// seule `Precedence` gouverne l'éviction.
///
/// Valeurs 5–7 réservées : `try_from_u8` retourne `Err`.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Nature {
    /// Solennité.
    Sollemnitas = 0,
    /// Fête.
    Festum = 1,
    /// Mémoire.
    Memoria = 2,
    /// Férie (y compris les Dimanches — voir spec §4.2).
    Feria = 3,
    /// Commémoration (position dans le Secondary Pool = signal).
    Commemoratio = 4,
    // 5–7 : réservés.
}

impl Nature {
    /// Convertit un `u8` en `Nature`. Retourne `Err` pour les valeurs 5–7 et 8–255.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Nature::Sollemnitas),
            1 => Ok(Nature::Festum),
            2 => Ok(Nature::Memoria),
            3 => Ok(Nature::Feria),
            4 => Ok(Nature::Commemoratio),
            _ => Err(DomainError {
                type_tag: 1,
                value: val,
            }),
        }
    }
}

// ─── 4.3 Color ────────────────────────────────────────────────────────────────

/// Couleur liturgique post-Vatican II (specification.md §4.3).
///
/// Largeur 4 bits dans `flags` (valeurs 0–6 définies, 7–15 réservés).
/// `try_from_u8` retourne `Err` pour les valeurs 6–15.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// Blanc — fêtes du Seigneur, Vierge, Confesseurs, Docteurs.
    Albus = 0,
    /// Rouge — Passion, Apôtres, Martyrs, Pentecôte.
    Rubeus = 1,
    /// Vert — Temps ordinaire.
    Viridis = 2,
    /// Violet — Avent, Carême.
    Violaceus = 3,
    /// Rose — Gaudete (Avent III), Laetare (Carême IV).
    Roseus = 4,
    /// Noir — Messes des défunts.
    Niger = 5,
    // 6 : usage liturgique futur (or/argent — optionnel selon usages diocésains).
    // 7–15 : réservés.
}

impl Color {
    /// Convertit un `u8` en `Color`. Retourne `Err` pour les valeurs 6–15 et 16–255.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Color::Albus),
            1 => Ok(Color::Rubeus),
            2 => Ok(Color::Viridis),
            3 => Ok(Color::Violaceus),
            4 => Ok(Color::Roseus),
            5 => Ok(Color::Niger),
            _ => Err(DomainError {
                type_tag: 2,
                value: val,
            }),
        }
    }
}

// ─── 4.4 Season ───────────────────────────────────────────────────────────────

/// Saison liturgique (specification.md §4.4).
///
/// Champ cache AOT : calculé par la Forge, non recalculé par l'Engine.
/// Largeur 3 bits dans `flags` (valeur 7 réservée).
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Season {
    /// Temps ordinaire (état par défaut).
    TempusOrdinarium = 0,
    /// Avent.
    TempusAdventus = 1,
    /// Temps de Noël.
    TempusNativitatis = 2,
    /// Carême.
    TempusQuadragesimae = 3,
    /// Triduum Pascal.
    TriduumPaschale = 4,
    /// Temps pascal.
    TempusPaschale = 5,
    /// Semaine Sainte (Rameaux–Mercredi Saint).
    DiesSancti = 6,
    // 7 : réservé.
}

impl Season {
    /// Convertit un `u8` en `Season`. Retourne `Err` pour la valeur 7 et 8–255.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Season::TempusOrdinarium),
            1 => Ok(Season::TempusAdventus),
            2 => Ok(Season::TempusNativitatis),
            3 => Ok(Season::TempusQuadragesimae),
            4 => Ok(Season::TriduumPaschale),
            5 => Ok(Season::TempusPaschale),
            6 => Ok(Season::DiesSancti),
            _ => Err(DomainError {
                type_tag: 3,
                value: val,
            }),
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Macro de roundtrip : try_from_u8(v as u8) == Ok(v) pour chaque variant.
    macro_rules! roundtrip_all {
        ($T:ty, [$($v:expr),+ $(,)?]) => {
            $( assert_eq!(<$T>::try_from_u8($v as u8), Ok($v)); )+
        };
    }

    #[test]
    fn precedence_roundtrip() {
        roundtrip_all!(
            Precedence,
            [
                Precedence::TriduumSacrum,
                Precedence::SollemnitatesFixaeMaior,
                Precedence::DominicaePrivilegiataeMaior,
                Precedence::FeriaePrivilegiataeMaior,
                Precedence::SollemnitatesGenerales,
                Precedence::SollemnitatesPropria,
                Precedence::FestaDomini,
                Precedence::DominicaePerAnnum,
                Precedence::FestaBMVEtSanctorumGenerales,
                Precedence::FestaPropria,
                Precedence::FeriaeAdventusEtOctavaNativitatis,
                Precedence::MemoriaeObligatoriae,
                Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
            ]
        );
    }

    #[test]
    fn precedence_reserved_values_are_err() {
        // Valeurs 13–15 réservées système (V2 de la spec interdit ces valeurs YAML).
        for v in [13u8, 14, 15, 16, 255] {
            assert!(
                Precedence::try_from_u8(v).is_err(),
                "Precedence::try_from_u8({v}) devrait être Err"
            );
        }
    }

    #[test]
    fn nature_roundtrip() {
        roundtrip_all!(
            Nature,
            [
                Nature::Sollemnitas,
                Nature::Festum,
                Nature::Memoria,
                Nature::Feria,
                Nature::Commemoratio,
            ]
        );
    }

    #[test]
    fn nature_reserved_values_are_err() {
        for v in [5u8, 6, 7, 255] {
            assert!(Nature::try_from_u8(v).is_err());
        }
    }

    #[test]
    fn color_roundtrip() {
        roundtrip_all!(
            Color,
            [
                Color::Albus,
                Color::Rubeus,
                Color::Viridis,
                Color::Violaceus,
                Color::Roseus,
                Color::Niger,
            ]
        );
    }

    #[test]
    fn color_reserved_values_are_err() {
        // Valeurs 6–15 réservées (4 bits dans flags, 7–15 réservés ; 6 = usage futur).
        for v in [6u8, 7, 15, 16, 255] {
            assert!(Color::try_from_u8(v).is_err());
        }
    }

    #[test]
    fn season_roundtrip() {
        roundtrip_all!(
            Season,
            [
                Season::TempusOrdinarium,
                Season::TempusAdventus,
                Season::TempusNativitatis,
                Season::TempusQuadragesimae,
                Season::TriduumPaschale,
                Season::TempusPaschale,
                Season::DiesSancti,
            ]
        );
    }

    #[test]
    fn season_reserved_values_are_err() {
        for v in [7u8, 8, 255] {
            assert!(Season::try_from_u8(v).is_err());
        }
    }

    #[test]
    fn precedence_ordering_is_inverse() {
        // Valeur numérique inverse : valeur plus faible = priorité plus haute.
        // TriduumSacrum (0) est strictement inférieur à tout autre rang.
        assert!(Precedence::TriduumSacrum < Precedence::SollemnitatesFixaeMaior);
        assert!(Precedence::SollemnitatesFixaeMaior < Precedence::MemoriaeObligatoriae);
        assert!(Precedence::MemoriaeObligatoriae < Precedence::FeriaePerAnnumEtMemoriaeAdLibitum);
    }
}
