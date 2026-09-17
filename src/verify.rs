//! Vérification des métadonnées d'un tag ID3v2 auprès de MusicBrainz.
//!
//! Cette fonctionnalité fait une requête réseau vers
//! `https://musicbrainz.org/ws/2/recording`, l'API de recherche
//! d'enregistrements de MusicBrainz, à partir du titre et de l'artiste lus
//! dans le tag local, et compare le résultat le plus pertinent aux valeurs
//! locales.
//!
//! # Portée volontairement limitée
//!
//! - La comparaison de texte est insensible à la casse, mais uniquement
//!   pour les caractères ASCII (`eq_ignore_ascii_case`) : `"café"` et
//!   `"CAFÉ"` sont considérés différents. Un repli Unicode complet
//!   demanderait une dépendance supplémentaire (`unicase` ou équivalent),
//!   jugée disproportionnée pour ce besoin.
//! - La requête Lucene envoyée à MusicBrainz n'échappe que les guillemets
//!   du titre et de l'artiste, pas l'ensemble des caractères spéciaux de
//!   la syntaxe Lucene (`+ - && || ! ( ) { } [ ] ^ ~ * ? : \`). Un titre
//!   contenant l'un de ces caractères peut donner une requête mal formée
//!   et donc [`VerifyError::NoMatch`] plutôt qu'une vraie erreur réseau.
//! - Un enregistrement MusicBrainz peut être associé à plusieurs éditions
//!   (`releases`), chacune avec son propre titre d'album et sa propre
//!   date. Seule la première de la liste renvoyée par l'API sert de
//!   référence pour comparer l'album et l'année.
//!
//! # Étiquette de l'API
//!
//! MusicBrainz demande un en-tête `User-Agent` identifiant l'application
//! (voir <https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting>) et
//! limite à environ une requête par seconde par application. Cette
//! fonction n'en fait qu'une par appel ; en boucle sur plusieurs fichiers,
//! il faudrait ajouter une pause entre les appels.

use crate::id3::header::Id3v2Tag;
use serde::Deserialize;
use std::fmt;

const MUSICBRAINZ_SEARCH_URL: &str = "https://musicbrainz.org/ws/2/recording";

/// Identifie l'application auprès de MusicBrainz, comme leur étiquette
/// d'utilisation le demande.
const USER_AGENT: &str = concat!(
    "mp3_metadata/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/SalimBXL/mp3_metadata )"
);

/// Erreur survenue lors de la vérification en ligne d'un tag.
///
/// Distincte de [`crate::Mp3Error`] : celle-ci ne concerne que le réseau et
/// la réponse de MusicBrainz, jamais la lecture ou le décodage du fichier
/// MP3 lui-même.
#[derive(Debug)]
pub enum VerifyError {
    /// Le tag local ne porte ni titre ni artiste : rien d'assez précis à
    /// chercher sur MusicBrainz.
    NothingToSearch,
    /// Erreur réseau, ou statut HTTP d'erreur renvoyé par MusicBrainz.
    Request(Box<ureq::Error>),
    /// La réponse a été reçue mais son corps JSON n'a pas pu être lu dans
    /// la forme attendue.
    Response(std::io::Error),
    /// La recherche n'a renvoyé aucun enregistrement.
    NoMatch,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::NothingToSearch => {
                write!(
                    f,
                    "le tag local ne contient ni titre ni artiste : rien à vérifier"
                )
            }
            VerifyError::Request(err) => {
                write!(f, "requête à MusicBrainz échouée : {err}")
            }
            VerifyError::Response(err) => {
                write!(f, "réponse MusicBrainz illisible : {err}")
            }
            VerifyError::NoMatch => {
                write!(
                    f,
                    "aucun enregistrement correspondant trouvé sur MusicBrainz"
                )
            }
        }
    }
}

impl std::error::Error for VerifyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VerifyError::Request(err) => Some(err),
            VerifyError::Response(err) => Some(err),
            VerifyError::NothingToSearch | VerifyError::NoMatch => None,
        }
    }
}

impl From<ureq::Error> for VerifyError {
    fn from(err: ureq::Error) -> Self {
        VerifyError::Request(Box::new(err))
    }
}

impl From<std::io::Error> for VerifyError {
    fn from(err: std::io::Error) -> Self {
        VerifyError::Response(err)
    }
}

/// Résultat d'une comparaison entre un champ du tag local et la valeur
/// correspondante renvoyée par MusicBrainz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldMatch {
    /// Les deux valeurs coïncident (comparaison ASCII insensible à la
    /// casse — voir la portée limitée en tête de module).
    Match,
    /// Les deux valeurs diffèrent.
    Mismatch { remote: String },
    /// Le tag local n'a pas ce champ ; rien à comparer, mais MusicBrainz
    /// propose une valeur.
    LocalMissing { remote: String },
}

impl fmt::Display for FieldMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldMatch::Match => write!(f, "concorde"),
            FieldMatch::Mismatch { remote } => {
                write!(f, "diffère (MusicBrainz : \"{remote}\")")
            }
            FieldMatch::LocalMissing { remote } => {
                write!(f, "absent localement (MusicBrainz : \"{remote}\")")
            }
        }
    }
}

/// Résultat de la vérification d'un tag auprès de MusicBrainz : l'
/// enregistrement retenu, et la comparaison champ par champ.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// Identifiant MusicBrainz (MBID) de l'enregistrement retenu.
    pub recording_id: String,
    /// Score de pertinence renvoyé par la recherche (0 à 100).
    pub score: Option<u32>,
    pub title: FieldMatch,
    pub artist: FieldMatch,
    /// `None` si l'enregistrement distant n'est associé à aucune édition.
    pub album: Option<FieldMatch>,
    /// `None` si l'enregistrement distant n'est associé à aucune édition
    /// datée.
    pub year: Option<FieldMatch>,
}

impl fmt::Display for VerificationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let score = self
            .score
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string());

        writeln!(f, "- VÉRIFICATION MUSICBRAINZ -----")?;
        writeln!(f, "Enregistrement : {} (score {score})", self.recording_id)?;
        writeln!(f, "Titre   : {}", self.title)?;
        writeln!(f, "Artiste : {}", self.artist)?;
        if let Some(album) = &self.album {
            writeln!(f, "Album   : {album}")?;
        }
        if let Some(year) = &self.year {
            writeln!(f, "Année   : {year}")?;
        }
        write!(f, "-------------------------------")
    }
}

/// Vérifie le titre, l'artiste, l'album et l'année d'un tag ID3v2 auprès de
/// MusicBrainz.
///
/// Cherche par titre et/ou artiste (au moins l'un des deux doit être
/// présent dans le tag local), retient l'enregistrement le mieux noté
/// parmi les résultats, et compare ses champs à ceux du tag local.
///
/// # Erreurs
///
/// - [`VerifyError::NothingToSearch`] si le tag local n'a ni titre ni
///   artiste.
/// - [`VerifyError::Request`] pour toute erreur réseau ou tout statut HTTP
///   d'erreur renvoyé par MusicBrainz.
/// - [`VerifyError::Response`] si le corps de la réponse ne peut pas être
///   lu dans la forme JSON attendue.
/// - [`VerifyError::NoMatch`] si la recherche ne renvoie aucun résultat.
pub fn verify_tag(tag: &Id3v2Tag) -> Result<VerificationReport, VerifyError> {
    let title = tag.title();
    let artist = tag.artist();

    if title.is_none() && artist.is_none() {
        return Err(VerifyError::NothingToSearch);
    }

    let query = build_query(title, artist);

    let response: SearchResponse = ureq::get(MUSICBRAINZ_SEARCH_URL)
        .set("User-Agent", USER_AGENT)
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", "5")
        .call()?
        .into_json()?;

    // Le score de pertinence est normalement déjà décroissant dans la
    // réponse, mais on ne s'y fie pas : on prend explicitement le meilleur.
    let best = response
        .recordings
        .into_iter()
        .max_by_key(|recording| recording.score.unwrap_or(0))
        .ok_or(VerifyError::NoMatch)?;

    Ok(build_report(tag, &best))
}

/// Construit la requête Lucene envoyée à MusicBrainz à partir du titre
/// et/ou de l'artiste locaux — voir les limites de l'échappement en tête
/// de module.
fn build_query(title: Option<&str>, artist: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(title) = title {
        parts.push(format!("recording:{}", quote_lucene(title)));
    }
    if let Some(artist) = artist {
        parts.push(format!("artist:{}", quote_lucene(artist)));
    }
    parts.join(" AND ")
}

/// Met `value` entre guillemets pour Lucene, en échappant les guillemets
/// qu'elle contiendrait déjà. N'échappe rien d'autre — voir les limites en
/// tête de module.
fn quote_lucene(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn build_report(tag: &Id3v2Tag, remote: &RemoteRecording) -> VerificationReport {
    let first_release = remote.releases.first();

    VerificationReport {
        recording_id: remote.id.clone(),
        score: remote.score,
        title: compare_field(tag.title(), &remote.title),
        artist: compare_field(tag.artist(), &remote.artist()),
        album: first_release.map(|release| compare_field(tag.album(), &release.title)),
        year: first_release
            .and_then(|release| release.date.as_deref())
            .map(|date| compare_field(tag.year(), release_year(date))),
    }
}

/// Extrait l'année d'une date MusicBrainz (`"1991-05-27"`, `"1991-05"` ou
/// `"1991"`), qui peut être partielle.
fn release_year(date: &str) -> &str {
    date.split('-').next().unwrap_or(date)
}

fn compare_field(local: Option<&str>, remote: &str) -> FieldMatch {
    match local {
        Some(local) if local.eq_ignore_ascii_case(remote) => FieldMatch::Match,
        Some(_) => FieldMatch::Mismatch {
            remote: remote.to_string(),
        },
        None => FieldMatch::LocalMissing {
            remote: remote.to_string(),
        },
    }
}

//
// ---------- Réponse JSON de l'API de recherche MusicBrainz ----------
//

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    recordings: Vec<RemoteRecording>,
}

#[derive(Debug, Deserialize)]
struct RemoteRecording {
    id: String,
    #[serde(default)]
    score: Option<u32>,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(default)]
    releases: Vec<RemoteRelease>,
}

impl RemoteRecording {
    /// Joint les crédits d'artiste par un espace. MusicBrainz fournit en
    /// réalité un `joinphrase` par crédit pour un rendu exact (ex.
    /// `"A feat. B"`) ; cette version plus simple ne l'utilise pas.
    fn artist(&self) -> String {
        self.artist_credit
            .iter()
            .map(|credit| credit.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Deserialize)]
struct ArtistCredit {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RemoteRelease {
    title: String,
    #[serde(default)]
    date: Option<String>,
}

//
// ---------- TESTS ----------
//
// Uniquement la logique pure (construction de requête, comparaison) : un
// test qui interrogerait le vrai MusicBrainz serait lent, dépendant du
// réseau, et fragile en CI.
//

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id3::frame::{Frame, FrameContent};
    use crate::id3::header::Id3Version;

    // ----- quote_lucene / build_query -----

    #[test]
    fn test_quote_lucene_escapes_embedded_quotes() {
        assert_eq!(quote_lucene(r#"He said "hi""#), r#""He said \"hi\"""#);
    }

    #[test]
    fn test_quote_lucene_plain_value() {
        assert_eq!(quote_lucene("Queen"), r#""Queen""#);
    }

    #[test]
    fn test_build_query_both_fields() {
        assert_eq!(
            build_query(Some("A Kind of Magic"), Some("Queen")),
            r#"recording:"A Kind of Magic" AND artist:"Queen""#
        );
    }

    #[test]
    fn test_build_query_title_only() {
        assert_eq!(build_query(Some("Hello"), None), r#"recording:"Hello""#);
    }

    #[test]
    fn test_build_query_artist_only() {
        assert_eq!(build_query(None, Some("Queen")), r#"artist:"Queen""#);
    }

    // ----- release_year -----

    #[test]
    fn test_release_year_full_date() {
        assert_eq!(release_year("1991-05-27"), "1991");
    }

    #[test]
    fn test_release_year_partial_date() {
        assert_eq!(release_year("1991"), "1991");
    }

    // ----- compare_field -----

    #[test]
    fn test_compare_field_match_is_ascii_case_insensitive() {
        assert_eq!(compare_field(Some("queen"), "Queen"), FieldMatch::Match);
    }

    #[test]
    fn test_compare_field_mismatch() {
        assert_eq!(
            compare_field(Some("The Beatles"), "Queen"),
            FieldMatch::Mismatch {
                remote: "Queen".to_string()
            }
        );
    }

    #[test]
    fn test_compare_field_local_missing() {
        assert_eq!(
            compare_field(None, "Queen"),
            FieldMatch::LocalMissing {
                remote: "Queen".to_string()
            }
        );
    }

    // ----- RemoteRecording::artist -----

    #[test]
    fn test_remote_recording_artist_joins_multiple_credits() {
        let recording = RemoteRecording {
            id: "x".to_string(),
            score: None,
            title: "T".to_string(),
            artist_credit: vec![
                ArtistCredit {
                    name: "Artist A".to_string(),
                },
                ArtistCredit {
                    name: "Artist B".to_string(),
                },
            ],
            releases: vec![],
        };
        assert_eq!(recording.artist(), "Artist A Artist B");
    }

    // ----- build_report -----

    /// Construit un tag local minimal directement (sans passer par les
    /// octets bruts) : seuls les champs texte utilisés par `build_report`
    /// nous intéressent ici.
    fn sample_local_tag(fields: &[(&[u8; 4], &str)]) -> Id3v2Tag {
        Id3v2Tag {
            version: Id3Version { major: 3, minor: 0 },
            flags: 0,
            size: 0,
            frames: fields
                .iter()
                .map(|(id, value)| Frame {
                    id: **id,
                    size: 0,
                    flags: 0,
                    content: FrameContent::Text(vec![value.to_string()]),
                    offset: 0,
                    next_offset: 0,
                })
                .collect(),
        }
    }

    fn sample_remote_recording() -> RemoteRecording {
        RemoteRecording {
            id: "abc-123".to_string(),
            score: Some(100),
            title: "A Kind of Magic".to_string(),
            artist_credit: vec![ArtistCredit {
                name: "Queen".to_string(),
            }],
            releases: vec![RemoteRelease {
                title: "Greatest Hits".to_string(),
                date: Some("1991-01-01".to_string()),
            }],
        }
    }

    #[test]
    fn test_build_report_all_fields_match() {
        let tag = sample_local_tag(&[
            (b"TIT2", "A Kind of Magic"),
            (b"TPE1", "Queen"),
            (b"TALB", "Greatest Hits"),
            (b"TYER", "1991"),
        ]);
        let report = build_report(&tag, &sample_remote_recording());

        assert_eq!(report.title, FieldMatch::Match);
        assert_eq!(report.artist, FieldMatch::Match);
        assert_eq!(report.album, Some(FieldMatch::Match));
        assert_eq!(report.year, Some(FieldMatch::Match));
    }

    #[test]
    fn test_build_report_year_compares_only_the_year_part_of_the_date() {
        let tag = sample_local_tag(&[(b"TYER", "1991")]);
        let report = build_report(&tag, &sample_remote_recording());

        // La date distante est "1991-01-01" ; seule l'année doit compter.
        assert_eq!(report.year, Some(FieldMatch::Match));
    }

    #[test]
    fn test_build_report_detects_mismatch() {
        let tag = sample_local_tag(&[(b"TIT2", "Une Autre Chanson")]);
        let report = build_report(&tag, &sample_remote_recording());

        assert_eq!(
            report.title,
            FieldMatch::Mismatch {
                remote: "A Kind of Magic".to_string()
            }
        );
    }

    #[test]
    fn test_build_report_no_release_means_no_album_or_year() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]);
        let mut remote = sample_remote_recording();
        remote.releases.clear();

        let report = build_report(&tag, &remote);

        assert_eq!(report.album, None);
        assert_eq!(report.year, None);
    }
}
