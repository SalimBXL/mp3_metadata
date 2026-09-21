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

// ----- sort_recordings / recording_sort_key -----

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
fn test_sort_recordings_orders_by_score_descending_first() {
    let tag = sample_local_tag(&[]);
    let mut recordings = [
        recording("low", 60, "Tribute Album"),
        recording("high", 100, "On Air"),
    ];

    sort_recordings(&tag, &mut recordings);

    assert_eq!(recordings[0].id, "high");
    assert_eq!(recordings[1].id, "low");
}

#[test]
fn test_sort_recordings_breaks_score_ties_alphabetically_by_artist() {
    let tag = sample_local_tag(&[]);
    let mut b_artist = recording("b", 100, "On Air");
    b_artist.artist_credit = vec![ArtistCredit {
        name: "Bee Artist".to_string(),
    }];
    let mut a_artist = recording("a", 100, "On Air");
    a_artist.artist_credit = vec![ArtistCredit {
        name: "Aardvark Artist".to_string(),
    }];
    let mut recordings = [b_artist, a_artist];

    sort_recordings(&tag, &mut recordings);

    assert_eq!(recordings[0].id, "a"); // "Aardvark..." < "Bee..."
    assert_eq!(recordings[1].id, "b");
}

#[test]
fn test_sort_recordings_breaks_remaining_ties_by_title_then_year_then_album() {
    let tag = sample_local_tag(&[]);

    let mut older = recording("older", 100, "Album");
    older.releases[0].date = Some("1990-01-01".to_string());
    let mut newer = recording("newer", 100, "Album");
    newer.releases[0].date = Some("2000-01-01".to_string());
    // Même score, même artiste, même titre : seule l'année diffère.
    let mut recordings = [newer, older];

    sort_recordings(&tag, &mut recordings);

    assert_eq!(recordings[0].id, "older"); // "1990" < "2000"
    assert_eq!(recordings[1].id, "newer");
}

#[test]
fn test_sort_recordings_stable_order_when_everything_ties() {
    let tag = sample_local_tag(&[]);
    let mut recordings = [
        recording("first", 100, "Same Album"),
        recording("second", 100, "Same Album"),
    ];

    sort_recordings(&tag, &mut recordings);

    // Rien ne les distingue : l'ordre d'origine est préservé (tri
    // stable), pas une erreur.
    assert_eq!(recordings[0].id, "first");
    assert_eq!(recordings[1].id, "second");
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
