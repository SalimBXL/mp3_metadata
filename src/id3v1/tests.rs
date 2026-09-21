use super::*;

/// Champs utilisés par `build_tag` — un struct plutôt que six
/// paramètres positionnels, pour que les tests qui n'en font varier
/// qu'un ou deux restent lisibles (`TagFields { genre: 255,
/// ..Default::default() }`) sans avoir à compter des `b""` vides.
#[derive(Default)]
struct TagFields<'a> {
    title: &'a [u8],
    artist: &'a [u8],
    album: &'a [u8],
    year: &'a [u8],
    comment: &'a [u8],
    genre: u8,
}

/// Construit les 128 octets d'un tag ID3v1, champs alignés sur leurs
/// offsets réels ; ce qui n'est pas fourni reste à zéro.
fn build_tag(fields: TagFields) -> [u8; ID3V1_LEN] {
    let mut data = [0u8; ID3V1_LEN];
    data[0..3].copy_from_slice(b"TAG");
    data[3..3 + fields.title.len()].copy_from_slice(fields.title);
    data[33..33 + fields.artist.len()].copy_from_slice(fields.artist);
    data[63..63 + fields.album.len()].copy_from_slice(fields.album);
    data[93..93 + fields.year.len()].copy_from_slice(fields.year);
    data[97..97 + fields.comment.len()].copy_from_slice(fields.comment);
    data[127] = fields.genre;
    data
}

#[test]
fn test_read_id3v1_tag_without_signature_returns_none() {
    let data = [0u8; ID3V1_LEN];
    assert!(read_id3v1_tag(&data).is_none());
}

#[test]
fn test_read_id3v1_tag_classic() {
    let data = build_tag(TagFields {
        title: b"A Kind of Magic",
        artist: b"Queen",
        album: b"Greatest Hits",
        year: b"1991",
        comment: b"Super chanson",
        genre: 17, // Rock
    });

    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.title, "A Kind of Magic");
    assert_eq!(tag.artist, "Queen");
    assert_eq!(tag.album, "Greatest Hits");
    assert_eq!(tag.year, "1991");
    assert_eq!(tag.comment, "Super chanson");
    assert_eq!(tag.track, None);
    assert_eq!(tag.genre, 17);
    assert_eq!(tag.genre_name(), Some("Rock"));
}

#[test]
fn test_read_id3v1_tag_v1_1_track_number() {
    let mut data = build_tag(TagFields {
        title: b"Titre",
        comment: b"Commentaire",
        ..Default::default()
    });
    // Marqueur ID3v1.1 : octet 28 du commentaire nul, octet 29 = piste.
    data[97 + 28] = 0;
    data[97 + 29] = 7;

    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.comment, "Commentaire");
    assert_eq!(tag.track, Some(7));
}

#[test]
fn test_read_id3v1_tag_without_track_number_keeps_full_comment() {
    // 30 octets de commentaire, aucun octet nul : pas de marqueur
    // ID3v1.1, les deux derniers octets font partie du texte.
    let comment = [b'X'; 30];
    let data = build_tag(TagFields {
        comment: &comment,
        ..Default::default()
    });

    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.comment, "X".repeat(30));
    assert_eq!(tag.track, None);
}

#[test]
fn test_read_id3v1_tag_pads_fields_with_spaces() {
    // Remplissage par espaces plutôt que par octets nuls, rencontré
    // chez certains encodeurs plus anciens.
    let mut title = [b' '; 30];
    title[..5].copy_from_slice(b"Space");
    let data = build_tag(TagFields {
        title: &title,
        ..Default::default()
    });

    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.title, "Space");
}

#[test]
fn test_read_id3v1_tag_unknown_genre_index_has_no_name() {
    // au-delà de 191 : aucune table
    let data = build_tag(TagFields {
        genre: 255,
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.genre_name(), None);
}

#[test]
fn test_read_id3v1_tag_last_standard_genre_index_has_a_name() {
    let data = build_tag(TagFields {
        genre: 79, // Hard Rock
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.genre_name(), Some("Hard Rock"));
}

#[test]
fn test_read_id3v1_tag_winamp_extension_first_and_last_have_names() {
    let first = read_id3v1_tag(&build_tag(TagFields {
        genre: 80,
        ..Default::default()
    }))
    .unwrap();
    let last = read_id3v1_tag(&build_tag(TagFields {
        genre: 191,
        ..Default::default()
    }))
    .unwrap();

    assert_eq!(first.genre_name(), Some("Folk"));
    assert_eq!(last.genre_name(), Some("Psybient"));
}

#[test]
fn test_read_id3v1_tag_just_past_winamp_extension_has_no_name() {
    let data = build_tag(TagFields {
        genre: 192,
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.genre_name(), None);
}

#[test]
fn test_read_id3v1_tag_decodes_latin1_byte() {
    // 0xE9 = 'é' en Latin-1.
    let data = build_tag(TagFields {
        title: &[b'H', b'i', 0xE9],
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert_eq!(tag.title, "Hié");
}

#[test]
fn test_display_includes_all_fields_and_track() {
    let mut data = build_tag(TagFields {
        title: b"Titre",
        artist: b"Artiste",
        album: b"Album",
        year: b"1999",
        comment: b"Com",
        genre: 17,
    });
    data[97 + 28] = 0;
    data[97 + 29] = 3;
    let tag = read_id3v1_tag(&data).unwrap();
    let text = tag.to_string();

    assert!(text.starts_with("ID3v1\n"));
    assert!(text.contains("Title      : Titre"));
    assert!(text.contains("Artist     : Artiste"));
    assert!(text.contains("Album      : Album"));
    assert!(text.contains("Year       : 1999"));
    assert!(text.contains("Comment    : Com"));
    assert!(text.contains("Track      : 3"));
    assert!(text.contains("Genre      : Rock"));
}

#[test]
fn test_display_shows_placeholder_track_for_classic_tag() {
    let data = build_tag(TagFields::default());
    let tag = read_id3v1_tag(&data).unwrap();

    // La ligne reste présente (alignement avec ID3v2), mais avec un
    // placeholder plutôt qu'un vrai numéro de piste.
    assert!(tag.to_string().contains("Track      : ?"));
}

// ----- format_genre / is_official_genre -----

#[test]
fn test_format_genre_official_shows_name_without_asterisk() {
    assert_eq!(format_genre(17, Some("Rock")), "Rock");
}

#[test]
fn test_format_genre_winamp_extension_shows_name_with_asterisk() {
    assert_eq!(format_genre(145, Some("Anime")), "Anime*");
}

#[test]
fn test_format_genre_unrecognized_shows_index_with_asterisk() {
    assert_eq!(format_genre(255, None), "255*");
}

#[test]
fn test_is_official_genre() {
    assert!(is_official_genre(0));
    assert!(is_official_genre(79));
    assert!(!is_official_genre(80));
    assert!(!is_official_genre(191));
    assert!(!is_official_genre(255));
}

#[test]
fn test_display_marks_unrecognized_genre_with_an_asterisk() {
    // au-delà même de l'extension Winamp
    let data = build_tag(TagFields {
        genre: 255,
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert!(tag.to_string().contains("Genre      : 255*"));
}

#[test]
fn test_display_marks_winamp_extension_genre_with_an_asterisk() {
    // Anime, extension Winamp
    let data = build_tag(TagFields {
        genre: 145,
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();

    assert!(tag.to_string().contains("Genre      : Anime*"));
}

#[test]
fn test_display_known_genre_has_no_asterisk() {
    let data = build_tag(TagFields {
        genre: 79, // Hard Rock, dernier de la table
        ..Default::default()
    });
    let tag = read_id3v1_tag(&data).unwrap();
    let text = tag.to_string();

    assert!(text.contains("Genre      : Hard Rock"));
    assert!(!text.contains('*'));
}
