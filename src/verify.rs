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
//! - Un même titre correspond souvent à plusieurs enregistrements
//!   MusicBrainz distincts (version studio, live, remaster, session
//!   radio...), avec un score de pertinence textuelle identique entre
//!   eux. [`verify_tag`] ne tranche pas seul entre eux : il renvoie un
//!   rapport par candidat à égalité de score (voir
//!   [`top_scored_recordings`]), à charge pour l'appelant de choisir.
//! - Un enregistrement MusicBrainz peut être associé à plusieurs éditions
//!   (`releases`), chacune avec son propre titre d'album et sa propre
//!   date — un morceau souvent réédité peut en avoir des dizaines.
//!   [`best_matching_release`] retient celle dont le titre correspond à
//!   l'album du tag local s'il y en a une, et ne retombe sur la première
//!   de la liste qu'à défaut (ou si le tag local n'a pas d'album) : rien
//!   ne garantit alors sa pertinence.
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
        writeln!(
            f,
            "Lien           : https://musicbrainz.org/recording/{}",
            self.recording_id
        )?;
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
/// présent dans le tag local) et renvoie un rapport par enregistrement
/// candidat parmi les mieux notés — voir [`top_scored_recordings`].
/// S'il n'y en a qu'un, le `Vec` renvoyé n'a qu'un élément.
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
pub fn verify_tag(tag: &Id3v2Tag) -> Result<Vec<VerificationReport>, VerifyError> {
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

    let candidates = top_scored_recordings(&response.recordings);
    if candidates.is_empty() {
        return Err(VerifyError::NoMatch);
    }

    Ok(candidates
        .into_iter()
        .map(|recording| build_report(tag, recording))
        .collect())
}

/// Retient, parmi les enregistrements renvoyés par la recherche, tous
/// ceux qui partagent le score de pertinence maximal.
///
/// Un même titre correspond souvent à plusieurs enregistrements
/// MusicBrainz distincts — version studio, live, remaster, session radio
/// — tous avec le même score de pertinence textuelle (titre/artiste), que
/// MusicBrainz ne départage pas plus finement. Plutôt que d'en choisir un
/// seul arbitrairement, on les renvoie tous : c'est à l'appelant de
/// trancher.
fn top_scored_recordings(recordings: &[RemoteRecording]) -> Vec<&RemoteRecording> {
    let top_score = recordings
        .iter()
        .filter_map(|recording| recording.score)
        .max();
    recordings
        .iter()
        .filter(|recording| recording.score == top_score)
        .collect()
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
    let release = best_matching_release(tag, &remote.releases);

    VerificationReport {
        recording_id: remote.id.clone(),
        score: remote.score,
        title: compare_field(tag.title(), &remote.title),
        artist: compare_field(tag.artist(), &remote.artist()),
        album: release.map(|release| compare_field(tag.album(), &release.title)),
        year: release
            .and_then(|release| release.date.as_deref())
            .map(|date| compare_field(tag.year(), release_year(date))),
    }
}

/// Choisit l'édition la plus pertinente pour comparer l'album et l'année.
///
/// Une égalité stricte échoue trop souvent en pratique : MusicBrainz et un
/// tag local ne nomment pas toujours une édition de la même façon (ex.
/// `"Greatest Hits Vol.2"` localement contre `"Greatest Hits II"` chez
/// MusicBrainz — aucun des deux n'est faux, ce sont deux conventions de
/// nommage différentes pour la même édition). On compare donc les deux
/// titres par recouvrement de mots (voir [`word_overlap`]) et on retient
/// l'édition avec le plus grand recouvrement, à condition qu'il soit non
/// nul. Sans album local, sans mot en commun avec aucune édition, ou sans
/// édition du tout, on retombe sur la première renvoyée par l'API — sans
/// garantie de pertinence dans ce cas.
fn best_matching_release<'a>(
    tag: &Id3v2Tag,
    releases: &'a [RemoteRelease],
) -> Option<&'a RemoteRelease> {
    let Some(local_album) = tag.album() else {
        return releases.first();
    };

    let local_words = normalize_words(local_album);
    if local_words.is_empty() {
        return releases.first();
    }

    releases
        .iter()
        .map(|release| {
            (
                release,
                word_overlap(&local_words, &normalize_words(&release.title)),
            )
        })
        .max_by_key(|(_, score)| *score)
        .filter(|(_, score)| *score > 0)
        .map(|(release, _)| release)
        .or_else(|| releases.first())
}

/// Découpe une chaîne en mots alphanumériques, en minuscules ASCII (même
/// limite que [`compare_field`] : les accents ne sont pas repliés).
fn normalize_words(s: &str) -> std::collections::HashSet<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Nombre de mots communs entre deux ensembles déjà normalisés par
/// [`normalize_words`].
fn word_overlap(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> usize {
    a.intersection(b).count()
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

    // ----- normalize_words / word_overlap -----

    #[test]
    fn test_normalize_words_splits_on_punctuation_and_lowercases() {
        assert_eq!(
            normalize_words("Greatest Hits Vol.2"),
            ["greatest", "hits", "vol", "2"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }

    #[test]
    fn test_normalize_words_empty_for_punctuation_only() {
        assert!(normalize_words("...").is_empty());
    }

    #[test]
    fn test_word_overlap_counts_shared_words() {
        let a = normalize_words("Greatest Hits Vol.2");
        let b = normalize_words("Greatest Hits II");
        assert_eq!(word_overlap(&a, &b), 2); // "greatest", "hits"
    }

    #[test]
    fn test_word_overlap_zero_when_unrelated() {
        let a = normalize_words("Greatest Hits Vol.2");
        let b = normalize_words("Big in Japan");
        assert_eq!(word_overlap(&a, &b), 0);
    }

    // ----- top_scored_recordings -----

    fn recording(id: &str, score: u32, release_title: &str) -> RemoteRecording {
        RemoteRecording {
            id: id.to_string(),
            score: Some(score),
            title: "A Kind of Magic".to_string(),
            artist_credit: vec![ArtistCredit {
                name: "Queen".to_string(),
            }],
            releases: vec![RemoteRelease {
                title: release_title.to_string(),
                date: None,
            }],
        }
    }

    #[test]
    fn test_top_scored_recordings_returns_all_tied_at_max() {
        // Cas concret : une version studio et une version live à égalité
        // de score — les deux doivent ressortir, pas une seule.
        let live = recording("live", 100, "On Air");
        let studio = recording("studio", 100, "Greatest Hits II");
        let cover_band = recording("cover", 60, "Tribute Album");
        let recordings = [live, studio, cover_band];

        let top = top_scored_recordings(&recordings);

        assert_eq!(top.len(), 2);
        assert!(top.iter().any(|r| r.id == "live"));
        assert!(top.iter().any(|r| r.id == "studio"));
    }

    #[test]
    fn test_top_scored_recordings_single_winner() {
        let a = recording("a", 100, "On Air");
        let b = recording("b", 80, "Greatest Hits II");
        let recordings = [a, b];

        let top = top_scored_recordings(&recordings);

        assert_eq!(top.len(), 1);
        assert_eq!(top[0].id, "a");
    }

    #[test]
    fn test_top_scored_recordings_empty_input() {
        assert!(top_scored_recordings(&[]).is_empty());
    }

    // ----- best_matching_release -----

    fn releases_greatest_hits_ii_and_big_in_japan() -> Vec<RemoteRelease> {
        vec![
            RemoteRelease {
                title: "Big in Japan".to_string(),
                date: Some("1994-05-01".to_string()),
            },
            RemoteRelease {
                title: "Greatest Hits II".to_string(),
                date: Some("1991-10-28".to_string()),
            },
        ]
    }

    #[test]
    fn test_best_matching_release_exact_match_wins() {
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits II")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
        assert_eq!(release.date.as_deref(), Some("1991-10-28"));
    }

    #[test]
    fn test_best_matching_release_finds_overlap_despite_different_naming() {
        // Régression concrète : le tag local dit "Greatest Hits Vol.2",
        // MusicBrainz dit "Greatest Hits II" — aucune correspondance
        // exacte, mais un net recouvrement ("greatest", "hits") face à
        // zéro recouvrement avec "Big in Japan".
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits Vol.2")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
    }

    #[test]
    fn test_best_matching_release_match_is_ascii_case_insensitive() {
        let tag = sample_local_tag(&[(b"TALB", "greatest hits vol.2")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_no_word_overlap() {
        let tag = sample_local_tag(&[(b"TALB", "Something Else Entirely")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan"); // la première, faute de mieux
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_local_has_no_album() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]); // pas de TALB
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan");
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_local_album_is_only_punctuation() {
        let tag = sample_local_tag(&[(b"TALB", "...")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan");
    }

    #[test]
    fn test_best_matching_release_none_when_no_releases() {
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits II")]);
        assert!(best_matching_release(&tag, &[]).is_none());
    }
}
