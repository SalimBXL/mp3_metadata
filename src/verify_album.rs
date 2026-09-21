//! Recherche MusicBrainz ciblée par album ([`search_by_album`]), en
//! complément de la recherche par morceau du module parent — voir la vue
//! d'ensemble dans [`super`].

use super::{
    ArtistCredit, FieldMatch, REQUEST_INTERVAL, USER_AGENT, VerificationReport, VerifyError,
    compare_field, normalize_words, quote_lucene, release_year, word_overlap,
};
use crate::id3::header::Id3v2Tag;
use serde::Deserialize;

/// Base des requêtes de recherche (`?query=...`) et de lookup
/// (`/{id}?inc=recordings`) d'éditions — voir [`search_by_album`].
const MUSICBRAINZ_RELEASE_URL: &str = "https://musicbrainz.org/ws/2/release";

/// Nombre d'éditions candidates dont [`search_by_album`] va récupérer la
/// liste de pistes. Volontairement petit : chaque candidat coûte une
/// requête de lookup supplémentaire (voir l'étiquette de l'API en tête de
/// module), et une recherche par titre d'album a affaire à bien moins de
/// candidats ambigus qu'une recherche par titre de morceau (voir la
/// portée limitée en tête de module) — les tout premiers résultats
/// suffisent presque toujours.
const RELEASE_LOOKUP_CANDIDATES: usize = 3;
/// Cherche directement l'édition locale (`tag.album()`) auprès de
/// MusicBrainz, plutôt que de partir du titre du morceau — voir la
/// stratégie combinée et ses limites en tête de module.
///
/// Cherche jusqu'à [`RELEASE_LOOKUP_CANDIDATES`] éditions pour
/// `release:"album" AND artist:"artiste"` (une requête), et pour chacune,
/// dans l'ordre de pertinence renvoyé par MusicBrainz, récupère sa liste
/// de pistes (une requête de lookup par candidat) jusqu'à en trouver une
/// dont une piste correspond au titre local — voir [`best_matching_track`].
/// S'arrête au premier succès plutôt que d'examiner tous les candidats.
///
/// Renvoie `None`, jamais d'erreur pour une recherche infructueuse : sans
/// album local, si la recherche ne trouve aucune édition, ou si aucun
/// candidat examiné n'a de piste correspondant au titre local.
///
/// # Erreurs
///
/// Uniquement pour un échec réseau ou de lecture de réponse JSON — jamais
/// pour une recherche infructueuse (voir ci-dessus). [`verify_tag`] avale
/// ces erreurs : cette recherche n'est qu'un complément à la recherche
/// principale par titre/artiste.
fn search_by_album(tag: &Id3v2Tag) -> Result<Option<VerificationReport>, VerifyError> {
    let Some(album) = tag.album() else {
        return Ok(None);
    };

    let mut parts = vec![format!("release:{}", quote_lucene(album))];
    if let Some(artist) = tag.artist() {
        parts.push(format!("artist:{}", quote_lucene(artist)));
    }
    let query = parts.join(" AND ");

    let response: ReleaseSearchResponse = ureq::get(MUSICBRAINZ_RELEASE_URL)
        .set("User-Agent", USER_AGENT)
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", &RELEASE_LOOKUP_CANDIDATES.to_string())
        .call()?
        .into_json()?;

    for (index, release) in response
        .releases
        .iter()
        .take(RELEASE_LOOKUP_CANDIDATES)
        .enumerate()
    {
        if index > 0 {
            std::thread::sleep(REQUEST_INTERVAL);
        }

        let lookup = fetch_release_lookup(&release.id)?;
        if let Some(report) = build_album_report(tag, release, &lookup) {
            return Ok(Some(report));
        }
    }

    Ok(None)
}
/// Récupère la liste de pistes d'une édition auprès de
/// [`MUSICBRAINZ_RELEASE_URL`] (`/{id}?inc=recordings`) — la partie réseau
/// de [`search_by_album`], isolée pour que sa logique de choix de piste
/// ([`build_album_report`]) reste testable sans réseau.
fn fetch_release_lookup(release_id: &str) -> Result<ReleaseLookup, VerifyError> {
    let url = format!("{MUSICBRAINZ_RELEASE_URL}/{release_id}");
    let lookup: ReleaseLookup = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .query("inc", "recordings")
        .query("fmt", "json")
        .call()?
        .into_json()?;
    Ok(lookup)
}
/// Construit un rapport à partir d'une édition candidate et de sa liste de
/// pistes déjà récupérée (voir [`fetch_release_lookup`]), en retenant
/// celle dont le titre correspond le mieux au titre local — voir
/// [`best_matching_track`]. `None` si aucune piste ne correspond
/// suffisamment (y compris si l'édition n'a pas de piste du tout).
///
/// L'album étant celui qu'on a explicitement cherché, `album` concorde
/// presque toujours avec le tag local — sauf si MusicBrainz l'a renvoyé
/// sous une forme différente malgré la recherche (ex. avec un sous-titre
/// que le tag local n'a pas).
fn build_album_report(
    tag: &Id3v2Tag,
    release: &RemoteReleaseSearchResult,
    lookup: &ReleaseLookup,
) -> Option<VerificationReport> {
    let track = best_matching_track(tag, lookup.media.iter().flat_map(|medium| &medium.tracks))?;

    Some(VerificationReport {
        recording_id: track.recording.id.clone(),
        score: release.score,
        title: compare_field(tag.title(), &track.title),
        artist: compare_field(tag.artist(), &release.artist()),
        album: Some(compare_field(tag.album(), &release.title)),
        year: release
            .date
            .as_deref()
            .map(|date| compare_field(tag.year(), release_year(date))),
        via_album: true,
    })
}
/// Choisit, parmi les pistes d'une édition, celle dont le titre correspond
/// le mieux au titre local — même logique de recouvrement de mots que
/// [`best_matching_release`] (un titre local et MusicBrainz ne s'écrivent
/// pas toujours à l'identique), mais sans repli sur la première piste en
/// l'absence de recouvrement : une édition a en général bien plus de
/// pistes que de mots dans son titre, un repli hasarderait un morceau
/// probablement faux plutôt que d'admettre l'absence de correspondance.
/// `None` si le tag local n'a pas de titre, ou si aucune piste n'a de mot
/// en commun avec lui.
fn best_matching_track<'a>(
    tag: &Id3v2Tag,
    tracks: impl IntoIterator<Item = &'a ReleaseTrack>,
) -> Option<&'a ReleaseTrack> {
    let local_words = normalize_words(tag.title()?);
    if local_words.is_empty() {
        return None;
    }

    tracks
        .into_iter()
        .map(|track| {
            (
                track,
                word_overlap(&local_words, &normalize_words(&track.title)),
            )
        })
        .max_by_key(|(_, score)| *score)
        .filter(|(_, score)| *score > 0)
        .map(|(track, _)| track)
}
//
// ---------- Réponses JSON de l'API pour la recherche par album ----------
//
// Deux formes différentes : la recherche (`/release?query=...`) renvoie un
// résumé par édition candidate, sans ses pistes ; le lookup
// (`/release/{id}?inc=recordings`) renvoie une seule édition mais avec le
// détail de ses pistes (voir search_by_album). Ce sont deux réponses
// distinctes de l'API MusicBrainz, d'où deux jeux de structures.
//
#[derive(Debug, Deserialize)]
struct ReleaseSearchResponse {
    #[serde(default)]
    releases: Vec<RemoteReleaseSearchResult>,
}
#[derive(Debug, Deserialize)]
struct RemoteReleaseSearchResult {
    id: String,
    #[serde(default)]
    score: Option<u32>,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
}
impl RemoteReleaseSearchResult {
    /// Voir [`RemoteRecording::artist`] — même limite (pas de
    /// `joinphrase`).
    fn artist(&self) -> String {
        self.artist_credit
            .iter()
            .map(|credit| credit.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}
#[derive(Debug, Deserialize)]
struct ReleaseLookup {
    #[serde(default)]
    media: Vec<Medium>,
}
/// Un support physique ou logique d'une édition (ex. un CD d'un coffret à
/// plusieurs disques), avec ses pistes.
#[derive(Debug, Deserialize)]
struct Medium {
    #[serde(default)]
    tracks: Vec<ReleaseTrack>,
}
#[derive(Debug, Deserialize)]
struct ReleaseTrack {
    title: String,
    recording: RecordingRef,
}
/// Enregistrement associé à une piste, tel que renvoyé par un lookup
/// d'édition — seul le MBID nous intéresse ici, pas les autres champs
/// qu'un enregistrement porte par ailleurs (voir [`RemoteRecording`]).
#[derive(Debug, Deserialize)]
struct RecordingRef {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id3::frame::{Frame, FrameContent};
    use crate::id3::header::{Id3Version, Id3v2Tag};

    /// Construit un tag local minimal directement (sans passer par les
    /// octets bruts) : seuls les champs texte utilisés par les tests ici
    /// nous intéressent.
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

    // ----- RemoteReleaseSearchResult::artist -----

    #[test]
    fn test_remote_release_search_result_artist_joins_multiple_credits() {
        let release = RemoteReleaseSearchResult {
            id: "x".to_string(),
            score: None,
            title: "T".to_string(),
            date: None,
            artist_credit: vec![
                ArtistCredit {
                    name: "Artist A".to_string(),
                },
                ArtistCredit {
                    name: "Artist B".to_string(),
                },
            ],
        };
        assert_eq!(release.artist(), "Artist A Artist B");
    }

    // ----- best_matching_track -----

    fn tracks_two_songs() -> Vec<ReleaseTrack> {
        vec![
            ReleaseTrack {
                title: "Under Pressure".to_string(),
                recording: RecordingRef {
                    id: "rec-under-pressure".to_string(),
                },
            },
            ReleaseTrack {
                title: "A Kind of Magic".to_string(),
                recording: RecordingRef {
                    id: "rec-a-kind-of-magic".to_string(),
                },
            },
        ]
    }

    #[test]
    fn test_best_matching_track_exact_title_match() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]);
        let tracks = tracks_two_songs();

        let track = best_matching_track(&tag, tracks.iter()).unwrap();

        assert_eq!(track.recording.id, "rec-a-kind-of-magic");
    }

    #[test]
    fn test_best_matching_track_none_when_local_has_no_title() {
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits II")]); // pas de TIT2
        let tracks = tracks_two_songs();

        assert!(best_matching_track(&tag, tracks.iter()).is_none());
    }

    #[test]
    fn test_best_matching_track_none_when_no_word_overlap() {
        let tag = sample_local_tag(&[(b"TIT2", "Totally Unrelated Song")]);
        let tracks = tracks_two_songs();

        // Contrairement à best_matching_release, pas de repli sur la
        // première piste : mieux vaut ne rien renvoyer qu'un morceau
        // probablement faux.
        assert!(best_matching_track(&tag, tracks.iter()).is_none());
    }

    #[test]
    fn test_best_matching_track_empty_tracks() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]);
        assert!(best_matching_track(&tag, std::iter::empty()).is_none());
    }

    // ----- build_album_report -----

    fn sample_release_search_result() -> RemoteReleaseSearchResult {
        RemoteReleaseSearchResult {
            id: "release-abc".to_string(),
            score: Some(100),
            title: "Greatest Hits II".to_string(),
            date: Some("1991-10-28".to_string()),
            artist_credit: vec![ArtistCredit {
                name: "Queen".to_string(),
            }],
        }
    }

    fn sample_release_lookup() -> ReleaseLookup {
        ReleaseLookup {
            media: vec![Medium {
                tracks: tracks_two_songs(),
            }],
        }
    }

    #[test]
    fn test_build_album_report_all_fields_match() {
        let tag = sample_local_tag(&[
            (b"TIT2", "A Kind of Magic"),
            (b"TPE1", "Queen"),
            (b"TALB", "Greatest Hits II"),
            (b"TYER", "1991"),
        ]);
        let report =
            build_album_report(&tag, &sample_release_search_result(), &sample_release_lookup())
                .unwrap();

        assert_eq!(report.recording_id, "rec-a-kind-of-magic");
        assert_eq!(report.title, FieldMatch::Match);
        assert_eq!(report.artist, FieldMatch::Match);
        assert_eq!(report.album, Some(FieldMatch::Match));
        assert_eq!(report.year, Some(FieldMatch::Match));
        assert!(report.via_album);
    }

    #[test]
    fn test_build_album_report_none_when_no_track_matches_local_title() {
        let tag = sample_local_tag(&[(b"TIT2", "Bohemian Rhapsody")]); // pas sur cette édition
        let report =
            build_album_report(&tag, &sample_release_search_result(), &sample_release_lookup());

        assert!(report.is_none());
    }

    #[test]
    fn test_build_album_report_media_flattens_across_discs() {
        // Un coffret à deux disques : la piste cherchée est sur le second.
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]);
        let lookup = ReleaseLookup {
            media: vec![
                Medium {
                    tracks: vec![ReleaseTrack {
                        title: "Under Pressure".to_string(),
                        recording: RecordingRef {
                            id: "rec-under-pressure".to_string(),
                        },
                    }],
                },
                Medium {
                    tracks: vec![ReleaseTrack {
                        title: "A Kind of Magic".to_string(),
                        recording: RecordingRef {
                            id: "rec-a-kind-of-magic".to_string(),
                        },
                    }],
                },
            ],
        };

        let report = build_album_report(&tag, &sample_release_search_result(), &lookup).unwrap();

        assert_eq!(report.recording_id, "rec-a-kind-of-magic");
    }
}
