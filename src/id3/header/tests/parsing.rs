use super::super::*;

// ----- declared_tag_body_size -----

#[test]
fn test_declared_tag_body_size_valid_header() {
    let header = [b'I', b'D', b'3', 3, 0, 0, 0, 0, 0, 13];
    assert_eq!(declared_tag_body_size(&header), Some(13));
}

#[test]
fn test_declared_tag_body_size_no_signature_returns_none() {
    let header = [0u8; 10];
    assert_eq!(declared_tag_body_size(&header), None);
}

#[test]
fn test_declared_tag_body_size_matches_real_file_bytes() {
    // Octets exacts observés en tête de a_kind_of_magic.mp3.
    let mut header = [0u8; 10];
    header[0..3].copy_from_slice(b"ID3");
    header[3] = 3;
    header[6..10].copy_from_slice(&[0x00, 0x01, 0x5B, 0x61]);
    assert_eq!(declared_tag_body_size(&header), Some(28129));
}

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

// ----- read_tag : cas d'erreur et absence de tag -----

#[test]
fn test_read_tag_too_small() {
    let data = [0u8; 5];
    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::TooSmall { len: 5 })
    ));
}

#[test]
fn test_read_tag_without_id3_signature_returns_none() {
    let mut data = [0u8; 10];
    data[0..3].copy_from_slice(b"XYZ");
    assert!(read_tag(&data).unwrap().is_none());
}

#[test]
fn test_read_tag_invalid_tag_size() {
    let mut data = build_tag_bytes(3, 0, 0, &[0u8; 100]);
    data.truncate(20);

    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::InvalidTagSize { .. })
    ));
}

#[test]
fn test_read_tag_propagates_frame_error() {
    let body = build_frame_bytes(b"TIT2", &[9, b'H', b'i']);
    let data = build_tag_bytes(3, 0, 0, &body);

    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::UnknownTextEncoding { encoding: 9 })
    ));
}

#[test]
fn test_read_tag_unsupported_version_propagates_from_read_frame() {
    // ID3v2.1 n'existe pas dans la spécification ; l'erreur vient bien
    // de la tentative de lecture des frames, pas d'une vérification
    // séparée dans read_tag.
    let body = build_frame_bytes(b"TIT2", &text_body("Hello"));
    let data = build_tag_bytes(1, 0, 0, &body);

    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::UnsupportedVersion { major: 1 })
    ));
}

// ----- read_tag : cas nominaux -----

#[test]
fn test_read_tag_header_fields() {
    let data = build_tag_bytes(3, 0, 0x80, &[0u8; 50]);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.version, Id3Version { major: 3, minor: 0 });
    // Le flag d'unsynchronisation ne change rien ici : le corps ne
    // contient que du padding, aucune paire 0xFF 0x00 à retirer.
    assert_eq!(tag.flags, 0x80);
    assert_eq!(tag.size, 50);
}

#[test]
fn test_read_tag_empty_body_has_no_frames() {
    let data = build_tag_bytes(4, 0, 0, &[]);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.size, 0);
    assert!(tag.frames.is_empty());
}

#[test]
fn test_read_tag_body_of_pure_padding_has_no_frames() {
    let data = build_tag_bytes(3, 0, 0, &[0u8; 50]);
    let tag = read_tag(&data).unwrap().unwrap();

    assert!(tag.frames.is_empty());
}

#[test]
fn test_read_tag_reads_all_frames_in_order() {
    let mut body = build_frame_bytes(b"TIT2", &text_body("A Kind of Magic"));
    body.extend(build_frame_bytes(b"TPE1", &text_body("Queen")));
    body.extend(build_frame_bytes(b"TALB", &text_body("Greatest Hits")));
    let data = build_tag_bytes(3, 0, 0, &body);

    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 3);
    assert_eq!(&tag.frames[0].id, b"TIT2");
    assert_eq!(&tag.frames[1].id, b"TPE1");
    assert_eq!(&tag.frames[2].id, b"TALB");
}

#[test]
fn test_read_tag_stops_at_padding() {
    let mut body = build_frame_bytes(b"TIT2", &text_body("Hello"));
    body.extend_from_slice(&[0u8; 40]); // padding de fin de tag
    let data = build_tag_bytes(3, 0, 0, &body);

    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 1);
}

#[test]
fn test_read_tag_reads_last_frame_without_padding() {
    // Régression : la borne de lecture portait sur `size` au lieu de
    // `10 + size`, ce qui faisait perdre la dernière frame d'un tag
    // rempli exactement, sans padding de fin.
    let mut body = build_frame_bytes(b"TIT2", &text_body("Hello"));
    body.extend(build_frame_bytes(b"TPE1", &text_body("Queen")));
    let data = build_tag_bytes(3, 0, 0, &body);

    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 2);
    assert_eq!(tag.artist(), Some("Queen"));
}

/// Construit les octets d'une frame ID3v2.4 : id, taille *synchsafe*,
/// flags, puis le corps.
fn build_frame_bytes_v2_4(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = body.len() as u32;
    let synchsafe_size = [
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(id);
    data.extend_from_slice(&synchsafe_size);
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(body);
    data
}

#[test]
fn test_read_tag_v2_4_uses_synchsafe_frame_sizes() {
    // Corps de 200 octets : au-delà de 127, brut et synchsafe
    // divergent. La frame est encodée en synchsafe, comme l'exige la
    // version 4 déclarée dans l'en-tête du tag.
    let text = "x".repeat(199); // + 1 octet d'encoding = 200
    let body = text_body(&text);
    let frame_bytes = build_frame_bytes_v2_4(b"TIT2", &body);

    let data = build_tag_bytes(4, 0, 0, &frame_bytes);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 1);
    assert_eq!(tag.title(), Some(text.as_str()));
}

#[test]
fn test_read_tag_v2_2_maps_frame_ids() {
    // En-tête de frame v2.2 : 3 octets d'id, 3 octets de taille brute.
    let mut body = Vec::new();
    body.extend_from_slice(b"TT2");
    let content = text_body("Hello");
    body.extend_from_slice(&(content.len() as u32).to_be_bytes()[1..]);
    body.extend_from_slice(&content);

    let data = build_tag_bytes(2, 0, 0, &body);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.title(), Some("Hello"));
}

// ----- read_tag : unsynchronisation -----

/// Applique l'unsynchronisation en sens écriture : insère un octet nul
/// après chaque `0xFF`. C'est l'inverse de `deunsynchronize`, utilisé
/// ici pour simuler ce qu'un encodeur écrirait réellement sur disque.
fn stuff_for_test(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &byte in data {
        out.push(byte);
        if byte == 0xFF {
            out.push(0x00);
        }
    }
    out
}

#[test]
fn test_read_tag_removes_unsynchronisation_before_parsing_frames() {
    // Contenu réel de la frame (celui qu'on doit retrouver après
    // lecture) : encoding Latin-1 (tout octet y est une valeur
    // valide), puis un octet 0xFF suivi de 'A'. La taille déclarée
    // dans l'en-tête de frame porte sur CE contenu, pas sur sa forme
    // bourrée : c'est la taille "avant unsynchronisation" que décrit
    // la spécification.
    let true_content = [0u8, 0xFF, b'A'];
    let mut frame = Vec::new();
    frame.extend_from_slice(b"TIT2");
    frame.extend_from_slice(&(true_content.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&true_content);

    // Ce qu'un encodeur écrirait réellement : la séquence complète
    // (en-tête de frame compris), avec un 0x00 inséré après le 0xFF.
    let stuffed = stuff_for_test(&frame);
    assert_eq!(stuffed.len(), frame.len() + 1); // une seule paire à protéger

    let data = build_tag_bytes(3, 0, UNSYNCHRONISATION_FLAG, &stuffed);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 1);
    // 0xFF en Latin-1 est U+00FF.
    assert_eq!(tag.title(), Some("\u{FF}A"));
}

#[test]
fn test_read_tag_unsynchronisation_flag_off_keeps_stuffed_zero() {
    // Sans le flag, aucune transformation n'est appliquée : les octets
    // sont pris tels quels, 0xFF 0x00 compris. Encoding Latin-1, pour
    // que 0xFF soit une valeur de caractère valide plutôt qu'une
    // erreur de validation UTF-8 sans rapport avec ce qui est testé.
    let mut body = vec![0];
    body.extend_from_slice(&[b'A', 0xFF, 0x00, b'B']);
    let frame = build_frame_bytes(b"TIT2", &body);
    let data = build_tag_bytes(3, 0, 0, &frame); // pas de flag d'unsync

    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.frames.len(), 1);
    assert_eq!(tag.frames[0].size, body.len() as u32);
    // Le 0x00 injecté agit comme un terminateur normal et scinde le
    // texte en deux valeurs : si `deunsynchronize` s'était appliqué à
    // tort malgré l'absence du flag, ce 0x00 aurait disparu et les
    // deux valeurs auraient fusionné en une seule.
    assert_eq!(
        tag.frame(b"TIT2").unwrap().content,
        FrameContent::Text(vec!["A\u{FF}".to_string(), "B".to_string()])
    );
}

// ----- read_tag : extended header -----

#[test]
fn test_read_tag_skips_v2_3_extended_header() {
    // Extended header v2.3 : taille brute (4) = 6 (n'inclut pas les 4
    // octets de taille eux-mêmes), suivie de 6 octets quelconques.
    let mut ext_header = vec![0u8, 0, 0, 6];
    ext_header.extend_from_slice(&[0u8; 6]);

    let mut body = ext_header;
    body.extend(build_frame_bytes(b"TIT2", &text_body("Hello")));

    let data = build_tag_bytes(3, 0, EXTENDED_HEADER_FLAG, &body);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.title(), Some("Hello"));
}

#[test]
fn test_read_tag_skips_v2_4_extended_header() {
    // Extended header v2.4 : taille synchsafe qui compte
    // l'intégralité de l'extended header, elle-même comprise — ici 10
    // octets au total.
    let mut ext_header = vec![0u8, 0, 0, 10];
    ext_header.extend_from_slice(&[0u8; 6]);

    let mut body = ext_header;
    body.extend(build_frame_bytes(b"TIT2", &text_body("Hello")));

    let data = build_tag_bytes(4, 0, EXTENDED_HEADER_FLAG, &body);
    let tag = read_tag(&data).unwrap().unwrap();

    assert_eq!(tag.title(), Some("Hello"));
}

#[test]
fn test_read_tag_extended_header_too_short_returns_err() {
    // Le tag ne contient que 2 octets, pas assez pour le champ de
    // taille de l'extended header lui-même (4 octets).
    let data = build_tag_bytes(3, 0, EXTENDED_HEADER_FLAG, &[0u8; 2]);

    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::ExtendedHeaderTooShort { .. })
    ));
}

#[test]
fn test_read_tag_extended_header_declared_size_exceeds_body() {
    // Le champ de taille annonce un extended header plus grand que le
    // corps du tag.
    let ext_header = vec![0u8, 0, 0, 200]; // 4 + 200 > taille réelle
    let data = build_tag_bytes(3, 0, EXTENDED_HEADER_FLAG, &ext_header);

    assert!(matches!(
        read_tag(&data),
        Err(Mp3Error::ExtendedHeaderTooShort { .. })
    ));
}
