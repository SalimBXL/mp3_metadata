use super::*;

const V2_2: Id3Version = Id3Version { major: 2, minor: 0 };
const V2_3: Id3Version = Id3Version { major: 3, minor: 0 };
const V2_4: Id3Version = Id3Version { major: 4, minor: 0 };

// ----- map_v2_2_id -----

#[test]
fn test_map_v2_2_id_known_frames() {
    assert_eq!(map_v2_2_id(*b"TT2"), *b"TIT2");
    assert_eq!(map_v2_2_id(*b"TP1"), *b"TPE1");
    assert_eq!(map_v2_2_id(*b"COM"), *b"COMM");
    assert_eq!(map_v2_2_id(*b"PIC"), *b"APIC");
}

#[test]
fn test_map_v2_2_id_unknown_frame_padded_with_null() {
    assert_eq!(map_v2_2_id(*b"XYZ"), [b'X', b'Y', b'Z', 0]);
}

// ----- read_frame : dispatch de version -----

#[test]
fn test_read_frame_unsupported_version_returns_err() {
    let data = [0u8; 20];
    assert!(matches!(
        read_frame(&data, 0, Id3Version { major: 1, minor: 0 }, false),
        Err(Mp3Error::UnsupportedVersion { major: 1 })
    ));
}

// ----- read_frame : ID3v2.3 / ID3v2.4 (en-tête 10 octets) -----

fn build_frame_bytes_v2_3(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(id);
    data.extend_from_slice(&(body.len() as u32).to_be_bytes());
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(body);
    data
}

/// Comme `build_frame_bytes_v2_3`, mais encode la taille en synchsafe,
/// pour tester la lecture v2.4.
fn build_frame_bytes_v2_4(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    build_frame_bytes_v2_4_with_flags(id, 0, body)
}

/// Comme `build_frame_bytes_v2_4`, avec des flags de frame explicites
/// (ex. [`FRAME_UNSYNCHRONISATION_FLAG`]) plutôt que toujours 0.
fn build_frame_bytes_v2_4_with_flags(id: &[u8; 4], flags: u16, body: &[u8]) -> Vec<u8> {
    let size = body.len() as u32;
    let synchsafe = [
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ];
    let mut data = Vec::new();
    data.extend_from_slice(id);
    data.extend_from_slice(&synchsafe);
    data.extend_from_slice(&flags.to_be_bytes());
    data.extend_from_slice(body);
    data
}

fn text_body(text: &str) -> Vec<u8> {
    let mut body = vec![3];
    body.extend_from_slice(text.as_bytes());
    body
}

#[test]
fn test_read_frame_v2_3_valid_decodes_content() {
    let data = build_frame_bytes_v2_3(b"TIT2", &text_body("Hello"));
    let frame = read_frame(&data, 0, V2_3, false).unwrap().unwrap();

    assert_eq!(&frame.id, b"TIT2");
    assert_eq!(frame.size, 6);
    assert_eq!(frame.content, FrameContent::Text(vec!["Hello".to_string()]));
    assert_eq!(frame.next_offset, 10 + 6);
}

#[test]
fn test_read_frame_v2_3_at_nonzero_offset() {
    let mut data = vec![0xAA; 20];
    data.extend(build_frame_bytes_v2_3(b"TPE1", &text_body("Queen")));

    let frame = read_frame(&data, 20, V2_3, false).unwrap().unwrap();

    assert_eq!(&frame.id, b"TPE1");
    assert_eq!(frame.offset, 20);
    assert_eq!(frame.next_offset, 20 + 10 + 6);
}

#[test]
fn test_read_frame_v2_3_zero_size_body() {
    let data = build_frame_bytes_v2_3(b"TCON", b"");
    let frame = read_frame(&data, 0, V2_3, false).unwrap().unwrap();

    assert_eq!(frame.size, 0);
    assert_eq!(frame.content, FrameContent::Empty);
    assert_eq!(frame.next_offset, 10);
}

#[test]
fn test_read_frame_v2_3_uses_raw_be32_not_synchsafe() {
    // Un corps de 200 octets : > 127, donc une taille synchsafe et une
    // taille brute divergent. En v2.3, la taille est brute.
    let body = vec![0u8; 200];
    let data = build_frame_bytes_v2_3(b"APIC", &body);
    let frame = read_frame(&data, 0, V2_3, false).unwrap().unwrap();

    assert_eq!(frame.size, 200);
    assert_eq!(frame.next_offset, 10 + 200);
}

#[test]
fn test_read_frame_v2_4_decodes_synchsafe_size() {
    // Corps de 200 octets, encodé en synchsafe : sans le bon décodage,
    // la frame serait mal découpée.
    let body = vec![0u8; 200];
    let data = build_frame_bytes_v2_4(b"APIC", &body);
    let frame = read_frame(&data, 0, V2_4, false).unwrap().unwrap();

    assert_eq!(frame.size, 200);
    assert_eq!(frame.next_offset, 10 + 200);
}

#[test]
fn test_read_frame_v2_4_small_size_matches_v2_3() {
    // En dessous de 128 octets, brut et synchsafe coïncident : les
    // deux lectures doivent s'accorder.
    let data_v3 = build_frame_bytes_v2_3(b"TIT2", &text_body("Hi"));
    let data_v4 = build_frame_bytes_v2_4(b"TIT2", &text_body("Hi"));

    let frame_v3 = read_frame(&data_v3, 0, V2_3, false).unwrap().unwrap();
    let frame_v4 = read_frame(&data_v4, 0, V2_4, false).unwrap().unwrap();

    assert_eq!(frame_v3.size, frame_v4.size);
    assert_eq!(frame_v3.content, frame_v4.content);
}

// ----- read_frame : unsynchronisation propre à une frame (ID3v2.4) -----
//
// Un identifiant de frame non reconnu (`XTST`) est utilisé plutôt
// qu'une frame texte : `FrameContent::Unknown` conserve les octets
// tels quels, sans le découpage sur terminateur nul qu'appliquerait
// `decode_text_values` — ce qui rendrait la présence ou l'absence du
// bourrage plus difficile à distinguer directement sur les octets.

/// Corps de frame contenant un `0xFF` suivi d'un octet nul « vrai » —
/// ce qui, une fois unsynchronisé, aurait été stocké avec un `0x00`
/// de bourrage inséré juste après le `0xFF` (voir `deunsynchronize`).
/// `stuffed` choisit laquelle des deux formes (stockée ou déjà propre)
/// est produite.
fn raw_body_with_ff_00(stuffed: bool) -> Vec<u8> {
    if stuffed {
        vec![0xFF, 0x00, 0x00, b'A'] // stocké : FF, bourrage, vrai 00, 'A'
    } else {
        vec![0xFF, 0x00, b'A'] // déjà propre : FF, vrai 00, 'A'
    }
}

#[test]
fn test_read_frame_v2_4_removes_frame_level_unsynchronisation() {
    let data = build_frame_bytes_v2_4_with_flags(
        b"XTST",
        FRAME_UNSYNCHRONISATION_FLAG,
        &raw_body_with_ff_00(true), // stocké : FF 00 00 41
    );

    let frame = read_frame(&data, 0, V2_4, false).unwrap().unwrap();

    // Ramené à FF 00 41 : le bourrage a bien été retiré.
    assert_eq!(frame.content, FrameContent::Unknown(vec![0xFF, 0x00, b'A']));
}

#[test]
fn test_read_frame_v2_4_without_the_flag_keeps_raw_bytes() {
    // Mêmes octets stockés, mais sans le bit d'unsynchronisation :
    // aucun retrait ne doit avoir lieu.
    let data = build_frame_bytes_v2_4_with_flags(
        b"XTST",
        0,
        &raw_body_with_ff_00(true), // FF 00 00 41, laissé tel quel
    );

    let frame = read_frame(&data, 0, V2_4, false).unwrap().unwrap();

    assert_eq!(
        frame.content,
        FrameContent::Unknown(vec![0xFF, 0x00, 0x00, b'A'])
    );
}

#[test]
fn test_read_frame_v2_3_ignores_the_v2_4_only_flag_bit() {
    // Même motif de bits que FRAME_UNSYNCHRONISATION_FLAG, mais en
    // v2.3 où ce bit n'existe pas : ne doit rien déclencher.
    let mut data = build_frame_bytes_v2_3(b"XTST", &raw_body_with_ff_00(true));
    data[9] = FRAME_UNSYNCHRONISATION_FLAG as u8; // octet bas des flags v2.3

    let frame = read_frame(&data, 0, V2_3, false).unwrap().unwrap();

    assert_eq!(
        frame.content,
        FrameContent::Unknown(vec![0xFF, 0x00, 0x00, b'A'])
    );
}

#[test]
fn test_read_frame_v2_4_skips_frame_flag_when_tag_already_unsynced() {
    // Le tag entier a déjà été désunsynchronisé par l'appelant (voir
    // `read_tag`) : le corps est donc déjà propre (FF 00 41, pas de
    // bourrage supplémentaire), même si le bit de la frame est posé.
    // Le consulter quand même couperait à tort le 0x00 légitime.
    let data = build_frame_bytes_v2_4_with_flags(
        b"XTST",
        FRAME_UNSYNCHRONISATION_FLAG,
        &raw_body_with_ff_00(false), // déjà propre : FF 00 41
    );

    let frame = read_frame(&data, 0, V2_4, true).unwrap().unwrap();

    assert_eq!(frame.content, FrameContent::Unknown(vec![0xFF, 0x00, b'A']));
}

#[test]
fn test_read_frame_propagates_decode_error() {
    let data = build_frame_bytes_v2_3(b"TIT2", &[9, b'H', b'i']);
    assert!(matches!(
        read_frame(&data, 0, V2_3, false),
        Err(Mp3Error::UnknownTextEncoding { encoding: 9 })
    ));
}

#[test]
fn test_read_frame_v2_3_too_short_for_header() {
    let data = [0u8; 5];
    assert!(matches!(
        read_frame(&data, 0, V2_3, false),
        Err(Mp3Error::FrameTooShort { offset: 0 })
    ));
}

#[test]
fn test_read_frame_v2_3_exactly_too_short() {
    let data = [0u8; 9];
    assert!(matches!(
        read_frame(&data, 0, V2_3, false),
        Err(Mp3Error::FrameTooShort { offset: 0 })
    ));
}

#[test]
fn test_read_frame_v2_3_padding_returns_none() {
    let data = [0u8; 10];
    assert!(read_frame(&data, 0, V2_3, false).unwrap().is_none());
}

#[test]
fn test_read_frame_v2_3_declared_size_exceeds_available_data() {
    let mut data = build_frame_bytes_v2_3(b"APIC", &[0u8; 100]);
    data.truncate(15);

    assert!(matches!(
        read_frame(&data, 0, V2_3, false),
        Err(Mp3Error::FrameSizeOverflow { offset: 0, .. })
    ));
}

#[test]
fn test_read_frame_v2_3_offset_beyond_data() {
    let data = build_frame_bytes_v2_3(b"TIT2", &text_body("Hello"));
    let offset = data.len();
    assert!(matches!(
        read_frame(&data, offset, V2_3, false),
        Err(Mp3Error::FrameTooShort { .. })
    ));
}

#[test]
fn test_read_frame_v2_3_offset_overflow_does_not_panic() {
    let data = [0u8; 20];
    assert!(matches!(
        read_frame(&data, usize::MAX - 5, V2_3, false),
        Err(Mp3Error::FrameTooShort { .. })
    ));
}

#[test]
fn test_read_frame_v2_3_size_overflow_does_not_panic() {
    let mut data = vec![0u8; 10];
    data[0..4].copy_from_slice(b"TIT2");
    data[4..8].copy_from_slice(&u32::MAX.to_be_bytes());

    assert!(matches!(
        read_frame(&data, usize::MAX - 20, V2_3, false),
        Err(Mp3Error::FrameTooShort { .. })
    ));
}

// ----- read_frame : ID3v2.2 (en-tête 6 octets) -----

fn build_frame_bytes_v2_2(id: &[u8; 3], body: &[u8]) -> Vec<u8> {
    let size = body.len() as u32;
    let mut data = Vec::new();
    data.extend_from_slice(id);
    data.extend_from_slice(&size.to_be_bytes()[1..]); // 3 octets
    data.extend_from_slice(body);
    data
}

#[test]
fn test_read_frame_v2_2_maps_known_id_and_decodes_content() {
    let data = build_frame_bytes_v2_2(b"TT2", &text_body("Hello"));
    let frame = read_frame(&data, 0, V2_2, false).unwrap().unwrap();

    assert_eq!(&frame.id, b"TIT2");
    assert_eq!(frame.size, 6);
    assert_eq!(frame.flags, 0);
    assert_eq!(frame.content, FrameContent::Text(vec!["Hello".to_string()]));
    assert_eq!(frame.next_offset, 6 + 6);
}

#[test]
fn test_read_frame_v2_2_unmapped_id_kept_as_padded_id() {
    let data = build_frame_bytes_v2_2(b"XYZ", &[1, 2, 3]);
    let frame = read_frame(&data, 0, V2_2, false).unwrap().unwrap();

    assert_eq!(&frame.id, &[b'X', b'Y', b'Z', 0]);
    assert_eq!(frame.content, FrameContent::Unknown(vec![1, 2, 3]));
}

#[test]
fn test_read_frame_v2_2_padding_returns_none() {
    let data = [0u8; 6];
    assert!(read_frame(&data, 0, V2_2, false).unwrap().is_none());
}

#[test]
fn test_read_frame_v2_2_too_short_for_header() {
    let data = [0u8; 5];
    assert!(matches!(
        read_frame(&data, 0, V2_2, false),
        Err(Mp3Error::FrameTooShort { offset: 0 })
    ));
}

#[test]
fn test_read_frame_v2_2_declared_size_exceeds_available_data() {
    let mut data = build_frame_bytes_v2_2(b"PIC", &[0u8; 50]);
    data.truncate(10);

    assert!(matches!(
        read_frame(&data, 0, V2_2, false),
        Err(Mp3Error::FrameSizeOverflow { offset: 0, .. })
    ));
}

#[test]
fn test_read_frame_v2_2_at_nonzero_offset() {
    let mut data = vec![0xAA; 12];
    data.extend(build_frame_bytes_v2_2(b"TP1", &text_body("Queen")));

    let frame = read_frame(&data, 12, V2_2, false).unwrap().unwrap();

    assert_eq!(&frame.id, b"TPE1");
    assert_eq!(frame.offset, 12);
    assert_eq!(frame.next_offset, 12 + 6 + 6);
}
