use super::super::*;

/// Construit un en-tête ID3v2 valide suivi de `body` : la taille du
/// tag est celle de `body`, encodée en synchsafe integer.
fn build_tag_bytes(major: u8, minor: u8, flags: u8, body: &[u8]) -> Vec<u8> {
    let size = body.len() as u32;
    let mut data = Vec::new();
    data.extend_from_slice(b"ID3");
    data.push(major);
    data.push(minor);
    data.push(flags);
    data.push(((size >> 21) & 0x7F) as u8);
    data.push(((size >> 14) & 0x7F) as u8);
    data.push(((size >> 7) & 0x7F) as u8);
    data.push((size & 0x7F) as u8);
    data.extend_from_slice(body);
    data
}

/// Construit les octets d'une frame ID3v2.3 : id, taille big-endian
/// brute, flags, puis le corps. Les corps utilisés dans ces tests
/// restent sous 128 octets, où taille brute et synchsafe coïncident :
/// ces mêmes octets sont donc valides à relire en tant que frame v2.4.
fn build_frame_bytes(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(id);
    data.extend_from_slice(&(body.len() as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(body);
    data
}

/// Corps de frame texte en UTF-8 : octet d'encoding 3, puis le texte.
fn text_body(text: &str) -> Vec<u8> {
    let mut body = vec![3];
    body.extend_from_slice(text.as_bytes());
    body
}

// ----- accesseurs -----

fn sample_tag() -> Id3v2Tag {
    let mut body = build_frame_bytes(b"TIT2", &text_body("A Kind of Magic"));
    body.extend(build_frame_bytes(b"TPE1", &text_body("Queen")));
    body.extend(build_frame_bytes(b"TPE2", &text_body("Queen")));
    body.extend(build_frame_bytes(b"TALB", &text_body("Greatest Hits")));
    body.extend(build_frame_bytes(b"TRCK", &text_body("1/17")));
    body.extend(build_frame_bytes(b"TCON", &text_body("Rock")));
    body.extend(build_frame_bytes(b"TYER", &text_body("1991")));

    let mut comm = vec![3];
    comm.extend_from_slice(b"eng\0Super chanson");
    body.extend(build_frame_bytes(b"COMM", &comm));

    let mut apic = vec![0];
    apic.extend_from_slice(b"image/jpeg\0");
    apic.push(3);
    apic.push(0);
    apic.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
    body.extend(build_frame_bytes(b"APIC", &apic));

    read_tag(&build_tag_bytes(3, 0, 0, &body)).unwrap().unwrap()
}

#[test]
fn test_accessors_return_expected_text() {
    let tag = sample_tag();

    assert_eq!(tag.title(), Some("A Kind of Magic"));
    assert_eq!(tag.artist(), Some("Queen"));
    assert_eq!(tag.album_artist(), Some("Queen"));
    assert_eq!(tag.album(), Some("Greatest Hits"));
    assert_eq!(tag.track(), Some("1/17"));
    assert_eq!(tag.genre(), Some("Rock"));
    assert_eq!(tag.year(), Some("1991"));
}

#[test]
fn test_comment_returns_text_without_language_or_description() {
    assert_eq!(sample_tag().comment(), Some("Super chanson"));
}

// ----- Display -----

#[test]
fn test_display_includes_header_and_fields() {
    let tag = sample_tag();
    let text = tag.to_string();

    assert!(text.starts_with("ID3v2 (2.3.0, 9 frames)\n"));
    assert!(text.contains("Title      : A Kind of Magic"));
    assert!(text.contains("Artist     : Queen"));
    assert!(text.contains("Album      : Greatest Hits"));
    assert!(text.contains("Year       : 1991"));
    assert!(text.contains("Comment    : Super chanson"));
    assert!(text.contains("Track      : 1/17"));
    assert!(text.contains("Genre      : Rock"));
    // "Album Artist" : 12 caractères, dépasse la largeur de colonne
    // (11), donc pas d'espace avant ":".
    assert!(text.contains("Album Artist: Queen"));
    assert!(text.contains("Cover      : image/jpeg"));
}

#[test]
fn test_display_fields_are_in_the_same_order_as_id3v1() {
    // Les champs communs aux deux formats doivent apparaître dans le
    // même ordre, pour que l'affichage côte à côte (voir main.rs,
    // side_by_side) aligne chaque champ sur la même ligne que son
    // équivalent ID3v1.
    let tag = sample_tag();
    let text = tag.to_string();

    // Préfixe exact de chaque champ ("Album      : ", etc.), pour ne
    // pas confondre "Album" avec "Album Artist" qui partage son début.
    let field = |label: &str| text.find(&format!("{label:<11}: ")).unwrap();

    assert!(field("Title") < field("Artist"));
    assert!(field("Artist") < field("Album"));
    assert!(field("Album") < field("Year"));
    assert!(field("Year") < field("Comment"));
    assert!(field("Comment") < field("Track"));
    assert!(field("Track") < field("Genre"));
}

#[test]
fn test_display_shows_placeholder_for_missing_fields() {
    let body = build_frame_bytes(b"TIT2", &text_body("Solo"));
    let tag = read_tag(&build_tag_bytes(3, 0, 0, &body)).unwrap().unwrap();
    let text = tag.to_string();

    assert!(text.contains("Artist     : ?"));
    assert!(text.contains("Comment    : ?"));
    assert!(!text.contains("Cover")); // pas d'image, pas de ligne Cover
}

#[test]
fn test_accessors_return_none_for_absent_frames() {
    let tag = sample_tag();

    assert_eq!(tag.lyrics(), None);
    assert_eq!(tag.text(b"TXXX"), None);
}

#[test]
fn test_text_returns_none_for_non_text_frame() {
    assert_eq!(sample_tag().text(b"APIC"), None);
}

#[test]
fn test_year_falls_back_to_tdrc() {
    let body = build_frame_bytes(b"TDRC", &text_body("1986"));
    let tag = read_tag(&build_tag_bytes(4, 0, 0, &body)).unwrap().unwrap();

    assert_eq!(tag.year(), Some("1986"));
}

#[test]
fn test_pictures_yields_only_apic_frames() {
    let tag = sample_tag();
    let pictures: Vec<_> = tag.pictures().collect();

    assert_eq!(pictures.len(), 1);
    assert!(matches!(
        &pictures[0].content,
        FrameContent::Picture { mime_type, .. } if mime_type == "image/jpeg"
    ));
}

#[test]
fn test_frames_with_id_returns_every_match() {
    let mut first = vec![3];
    first.extend_from_slice(b"eng\0Great song");
    let mut second = vec![3];
    second.extend_from_slice(b"fra\0Super chanson");

    let mut body = build_frame_bytes(b"COMM", &first);
    body.extend(build_frame_bytes(b"COMM", &second));
    let tag = read_tag(&build_tag_bytes(3, 0, 0, &body)).unwrap().unwrap();

    assert_eq!(tag.frames_with_id(b"COMM").count(), 2);
    assert_eq!(tag.comment(), Some("Great song"));
}

// ----- extended_header_len -----

#[test]
fn test_extended_header_len_v2_3_excludes_size_field_itself() {
    let body = [0u8, 0, 0, 6, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        extended_header_len(&body, Id3Version { major: 3, minor: 0 }).unwrap(),
        10 // 4 (champ de taille) + 6 (valeur lue)
    );
}

#[test]
fn test_extended_header_len_v2_4_includes_size_field_itself() {
    let body = [0u8, 0, 0, 10, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        extended_header_len(&body, Id3Version { major: 4, minor: 0 }).unwrap(),
        10
    );
}
