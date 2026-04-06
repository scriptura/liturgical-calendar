// liturgical-calendar-core/src/types.rs
//
// Types de domaine canoniques (spec §4.1–4.4).
// Chaque enum est `#[repr(u8)]` — représentation binaire exacte dans le `.kald`.
// `try_from_u8` est le seul point d'entrée externe (INV-FFI-3).
//
// DIVERGENCE flags (nota bene) :
//   Le prompt initial (brief) plaçait Nature aux bits 4–6 et Color aux bits 11–13.
//   La spec §3.4 (canonique) et le roadmap §1.3 définissent :
//     bits 4–7  → Color  (4 bits, masque 0x000F après >> 4)
//     bits 11–13→ Nature (3 bits, masque 0x0007 après >> 11)
//   C'est la spec §3.4 qui fait foi. Les masques d'extraction dans entry.rs
//   suivent ce layout.

/// Erreur de décodage d'un champ de domaine depuis `flags`.
///
/// `Copy` et sans allocation — compatible avec `no_std` (spec §1.1 roadmap).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    /// Valeur numérique hors de l'intervalle valide pour cet enum.
    InvalidValue(u8),
}

// ─────────────────────────────────────────────────────────────────────────────
// 4.1 Precedence — bits 0–3 de `flags` (4 bits)
// ─────────────────────────────────────────────────────────────────────────────

/// Rang liturgique effectif, résolu définitivement par la Forge (cache AOT).
///
/// Axe ordinal de résolution de collision. Valeur numérique **inverse** :
/// une valeur plus faible signifie une priorité plus haute.
/// Comparaison entière pure — aucune branche de correspondance au runtime.
///
/// *Tabella dierum liturgicorum — NALC 1969. Ordre figé. Aucune modification autorisée.*
/// Valeurs 13–15 réservées système (validation V2).
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precedence {
    /// Triduum Sacrum (priorité absolue).
    TriduumSacrum                     = 0,
    /// Solennités fixes majeures (Noël, Épiphanie, Ascension…).
    SollemnitatesFixaeMaior           = 1,
    /// Dimanches privilégiés majeurs (Avent, Carême, Pâques).
    DominicaePrivilegiataeMaior       = 2,
    /// Féries privilégiées majeures (Mercredi des Cendres, Semaine Sainte).
    FeriaePrivilegiataeMaior          = 3,
    /// Solennités générales du calendrier universel.
    SollemnitatesGenerales            = 4,
    /// Solennités propres (diocésaines, religieuses).
    SollemnitatesPropria              = 5,
    /// Fêtes du Seigneur inscrites au calendrier général.
    FestaDomini                       = 6,
    /// Dimanches du temps ordinaire.
    DominicaePerAnnum                 = 7,
    /// Fêtes de la BVM et des saints du calendrier général.
    FestaBMVEtSanctorumGenerales      = 8,
    /// Fêtes propres.
    FestaPropria                      = 9,
    /// Féries d'Avent et octave de Noël (17–24 décembre).
    FeriaeAdventusEtOctavaNativitatis = 10,
    /// Mémoires obligatoires.
    MemoriaeObligatoriae              = 11,
    /// Féries du temps ordinaire et mémoires facultatives.
    FeriaePerAnnumEtMemoriaeAdLibitum = 12,
    // 13–15 : réservés système — V2 interdit ces valeurs dans les entrées YAML.
}

impl Precedence {
    /// Décode un octet brut en `Precedence`.
    ///
    /// Retourne `Err(DomainError::InvalidValue(val))` pour les valeurs 13–15
    /// (réservées système — validation V2) et toute valeur > 15.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0  => Ok(Self::TriduumSacrum),
            1  => Ok(Self::SollemnitatesFixaeMaior),
            2  => Ok(Self::DominicaePrivilegiataeMaior),
            3  => Ok(Self::FeriaePrivilegiataeMaior),
            4  => Ok(Self::SollemnitatesGenerales),
            5  => Ok(Self::SollemnitatesPropria),
            6  => Ok(Self::FestaDomini),
            7  => Ok(Self::DominicaePerAnnum),
            8  => Ok(Self::FestaBMVEtSanctorumGenerales),
            9  => Ok(Self::FestaPropria),
            10 => Ok(Self::FeriaeAdventusEtOctavaNativitatis),
            11 => Ok(Self::MemoriaeObligatoriae),
            12 => Ok(Self::FeriaePerAnnumEtMemoriaeAdLibitum),
            _  => Err(DomainError::InvalidValue(val)), // 13–15 réservés, >15 impossible via masque 0x0F
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4.2 Nature — bits 11–13 de `flags` (3 bits)
// ─────────────────────────────────────────────────────────────────────────────

/// Axe sémantique de la célébration.
///
/// La Nature ne dicte jamais la force d'éviction — seule `Precedence` est
/// utilisée pour l'éviction. Ce découplage est la justification structurelle
/// du modèle 2D (spec §4.2).
///
/// `PartialOrd`/`Ord` dérivés par discriminant (`repr(u8)`) — sans signification
/// liturgique, satisfont uniquement la contrainte de typage des collections ordonnées.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Nature {
    /// Solennité.
    Sollemnitas  = 0,
    /// Fête.
    Festum       = 1,
    /// Mémoire (obligatoire ou facultative).
    Memoria      = 2,
    /// Férie (y compris les dimanches — cf. spec §4.2).
    Feria        = 3,
    /// Commémoration.
    Commemoratio = 4,
    // 5–7 : réservés.
}

impl Nature {
    /// Décode un octet brut en `Nature`.
    ///
    /// Retourne `Err` pour les valeurs 5–7 (réservées) et > 7.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Sollemnitas),
            1 => Ok(Self::Festum),
            2 => Ok(Self::Memoria),
            3 => Ok(Self::Feria),
            4 => Ok(Self::Commemoratio),
            _ => Err(DomainError::InvalidValue(val)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4.3 Color — bits 4–7 de `flags` (4 bits, masque 0x0F après >> 4)
// ─────────────────────────────────────────────────────────────────────────────

/// Couleur liturgique post-Vatican II.
///
/// Largeur portée de 3 bits (v1.0) à 4 bits (v2.0) pour extensibilité future.
/// Valeurs 0–5 définies, 6–15 réservées.
///
/// `PartialOrd`/`Ord` dérivés par discriminant — sans signification liturgique.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Color {
    /// Blanc — fêtes du Seigneur, Vierge, Confesseurs, Docteurs.
    Albus     = 0,
    /// Rouge — Passion, Apôtres, Martyrs, Pentecôte.
    Rubeus    = 1,
    /// Vert — temps ordinaire.
    Viridis   = 2,
    /// Violet — Avent, Carême.
    Violaceus = 3,
    /// Rose — Gaudete (Avent III), Laetare (Carême IV).
    Roseus    = 4,
    /// Noir — messes des défunts.
    Niger     = 5,
    // 6 : usage liturgique futur (or, argent — optionnel selon usages diocésains).
    // 7–15 : réservés.
}

impl Color {
    /// Décode un octet brut en `Color`.
    ///
    /// Retourne `Err` pour les valeurs 6–15 (réservées ou futur usage).
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Albus),
            1 => Ok(Self::Rubeus),
            2 => Ok(Self::Viridis),
            3 => Ok(Self::Violaceus),
            4 => Ok(Self::Roseus),
            5 => Ok(Self::Niger),
            _ => Err(DomainError::InvalidValue(val)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4.4 LiturgicalPeriod — bits 8–10 de `flags` (3 bits)
// ─────────────────────────────────────────────────────────────────────────────

/// Période opérationnelle résolue (cache AOT).
///
/// Projection technique matérialisée par la Forge — pas une taxonomie du Missel.
/// `DiesSancti` (Semaine Sainte) est un variant pleinement valide bien
/// qu'hétérogène sur le plan liturgique strict.
///
/// Ce type est le "firewall sémantique" décrit dans l'ADR :
/// il encode des segments mutuellement exclusifs indispensables au pipeline
/// déterministe. Toute tentative de "purification" runtime briserait l'invariant O(1).
///
/// Accès : `(flags >> 8) & 0x0007` — opération bitwise unique, SIMD-compatible.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiturgicalPeriod {
    /// Temps ordinaire (état par défaut, valeur 0).
    TempusOrdinarium    = 0,
    /// Avent.
    TempusAdventus      = 1,
    /// Temps de Noël.
    TempusNativitatis   = 2,
    /// Carême.
    TempusQuadragesimae = 3,
    /// Triduum Pascal.
    TriduumPaschale     = 4,
    /// Temps pascal.
    TempusPaschale      = 5,
    /// Semaine Sainte (Rameaux inclus → Mercredi Saint inclus).
    /// Variant opérationnel — subdivision du Carême, non du Missel strict.
    DiesSancti          = 6,
    // 7 : réservé.
}

impl LiturgicalPeriod {
    /// Décode un octet brut en `LiturgicalPeriod`.
    ///
    /// Retourne `Err` pour la valeur 7 (réservée) et > 7.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::TempusOrdinarium),
            1 => Ok(Self::TempusAdventus),
            2 => Ok(Self::TempusNativitatis),
            3 => Ok(Self::TempusQuadragesimae),
            4 => Ok(Self::TriduumPaschale),
            5 => Ok(Self::TempusPaschale),
            6 => Ok(Self::DiesSancti),
            _ => Err(DomainError::InvalidValue(val)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests unitaires — tâche 1.1 roadmap
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // Macro : roundtrip try_from_u8(v as u8) == Ok(v) pour chaque variant.
    macro_rules! roundtrip_all {
        ($enum:ident, [$($val:expr),+]) => {
            $(
                let v = $val;
                assert_eq!($enum::try_from_u8(v as u8), Ok(v),
                    "{}: roundtrip échoué pour {:?}", stringify!($enum), v);
            )+
        };
    }

    #[test]
    fn precedence_roundtrip() {
        roundtrip_all!(Precedence, [
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
            Precedence::FeriaePerAnnumEtMemoriaeAdLibitum
        ]);
    }

    #[test]
    fn precedence_reserved_values_are_errors() {
        // Valeurs 13–15 réservées système — V2 les interdit (spec §4.1).
        assert_eq!(Precedence::try_from_u8(13), Err(DomainError::InvalidValue(13)));
        assert_eq!(Precedence::try_from_u8(14), Err(DomainError::InvalidValue(14)));
        assert_eq!(Precedence::try_from_u8(15), Err(DomainError::InvalidValue(15)));
    }

    #[test]
    fn nature_roundtrip() {
        roundtrip_all!(Nature, [
            Nature::Sollemnitas,
            Nature::Festum,
            Nature::Memoria,
            Nature::Feria,
            Nature::Commemoratio
        ]);
    }

    #[test]
    fn nature_reserved_are_errors() {
        for v in 5u8..=7 {
            assert_eq!(Nature::try_from_u8(v), Err(DomainError::InvalidValue(v)));
        }
    }

    #[test]
    fn color_roundtrip() {
        roundtrip_all!(Color, [
            Color::Albus,
            Color::Rubeus,
            Color::Viridis,
            Color::Violaceus,
            Color::Roseus,
            Color::Niger
        ]);
    }

    #[test]
    fn color_reserved_are_errors() {
        for v in 6u8..=15 {
            assert_eq!(Color::try_from_u8(v), Err(DomainError::InvalidValue(v)));
        }
    }

    #[test]
    fn liturgical_period_roundtrip() {
        roundtrip_all!(LiturgicalPeriod, [
            LiturgicalPeriod::TempusOrdinarium,
            LiturgicalPeriod::TempusAdventus,
            LiturgicalPeriod::TempusNativitatis,
            LiturgicalPeriod::TempusQuadragesimae,
            LiturgicalPeriod::TriduumPaschale,
            LiturgicalPeriod::TempusPaschale,
            LiturgicalPeriod::DiesSancti
        ]);
    }

    #[test]
    fn liturgical_period_reserved_is_error() {
        assert_eq!(LiturgicalPeriod::try_from_u8(7), Err(DomainError::InvalidValue(7)));
    }
}
