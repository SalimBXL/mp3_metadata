use super::*;

// ----- decode_string -----

#[test]
fn test_decode_string_latin1() {
    assert_eq!(decode_string(0, &[b'H', b'i', 0xE9]).unwrap(), "Hié");
}

#[test]
fn test_decode_string_latin1_empty() {
    assert_eq!(decode_string(0, &[]).unwrap(), "");
}

#[test]
fn test_decode_string_utf16_le_bom() {
    let data = [0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00];
    assert_eq!(decode_string(1, &data).unwrap(), "Hi");
}

#[test]
fn test_decode_string_utf16_be_bom() {
    let data = [0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
    assert_eq!(decode_string(1, &data).unwrap(), "Hi");
}

#[test]
fn test_decode_string_utf16_empty_is_not_an_error() {
    assert_eq!(decode_string(1, &[]).unwrap(), "");
}

#[test]
fn test_decode_string_utf16_invalid_bom_returns_err() {
    let data = [0x12, 0x34, 0x00, 0x48];
    assert!(matches!(
        decode_string(1, &data),
        Err(Mp3Error::InvalidTextData { encoding: 1 })
    ));
}

#[test]
fn test_decode_string_utf16_too_short_for_bom_returns_err() {
    assert!(matches!(
        decode_string(1, &[0xFF]),
        Err(Mp3Error::InvalidTextData { encoding: 1 })
    ));
}

#[test]
fn test_decode_string_utf16_odd_trailing_byte_ignored() {
    let data = [0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00, 0xAB];
    assert_eq!(decode_string(1, &data).unwrap(), "Hi");
}

#[test]
fn test_decode_string_utf16be_no_bom() {
    assert_eq!(decode_string(2, &[0x00, 0x48, 0x00, 0x69]).unwrap(), "Hi");
}

#[test]
fn test_decode_string_utf16be_invalid_surrogate_returns_err() {
    assert!(matches!(
        decode_string(2, &[0xD8, 0x00]),
        Err(Mp3Error::InvalidTextData { encoding: 2 })
    ));
}

#[test]
fn test_decode_string_utf8() {
    assert_eq!(decode_string(3, "Café".as_bytes()).unwrap(), "Café");
}

#[test]
fn test_decode_string_utf8_invalid_bytes_returns_err() {
    assert!(matches!(
        decode_string(3, &[0xFF, 0xFF]),
        Err(Mp3Error::InvalidTextData { encoding: 3 })
    ));
}

#[test]
fn test_decode_string_unknown_encoding_returns_err() {
    assert!(matches!(
        decode_string(9, b"Hi"),
        Err(Mp3Error::UnknownTextEncoding { encoding: 9 })
    ));
}

// ----- split_at_terminator / strip_trailing_terminator -----

#[test]
fn test_split_at_terminator_single_byte() {
    let (before, after) = split_at_terminator(0, b"abc\0def").unwrap();
    assert_eq!(before, b"abc");
    assert_eq!(after, b"def");
}

#[test]
fn test_split_at_terminator_none_when_absent() {
    assert!(split_at_terminator(0, b"abc").is_none());
}

#[test]
fn test_split_at_terminator_utf16_two_bytes() {
    let data = [0x41, 0x00, 0x00, 0x00, 0x42, 0x00];
    let (before, after) = split_at_terminator(1, &data).unwrap();
    assert_eq!(before, [0x41, 0x00]);
    assert_eq!(after, [0x42, 0x00]);
}

#[test]
fn test_split_at_terminator_utf16_ignores_misaligned_nulls() {
    let data = [0x41, 0x00, 0x00, 0x42];
    assert!(split_at_terminator(1, &data).is_none());
}

#[test]
fn test_strip_trailing_terminator() {
    assert_eq!(strip_trailing_terminator(0, b"abc\0"), b"abc");
    assert_eq!(strip_trailing_terminator(0, b"abc"), b"abc");
    assert_eq!(
        strip_trailing_terminator(1, &[0x41, 0x00, 0x00, 0x00]),
        [0x41, 0x00]
    );
}

// ----- decode_text_values -----

#[test]
fn test_decode_text_values_single() {
    let mut data = vec![3];
    data.extend_from_slice(b"Bohemian Rhapsody");
    assert_eq!(
        decode_text_values(&data).unwrap(),
        vec!["Bohemian Rhapsody".to_string()]
    );
}

#[test]
fn test_decode_text_values_strips_trailing_terminator() {
    let mut data = vec![3];
    data.extend_from_slice(b"Queen\0");
    assert_eq!(
        decode_text_values(&data).unwrap(),
        vec!["Queen".to_string()]
    );
}

#[test]
fn test_decode_text_values_multiple() {
    let mut data = vec![3];
    data.extend_from_slice(b"Rock\0Pop");
    assert_eq!(
        decode_text_values(&data).unwrap(),
        vec!["Rock".to_string(), "Pop".to_string()]
    );
}

#[test]
fn test_decode_text_values_no_text_after_encoding() {
    assert!(decode_text_values(&[3]).unwrap().is_empty());
}

#[test]
fn test_decode_text_values_utf16_each_value_keeps_its_bom() {
    let data = [
        1, 0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69,
    ];
    assert_eq!(
        decode_text_values(&data).unwrap(),
        vec!["Hi".to_string(), "Hi".to_string()]
    );
}

// ----- decode_frame -----

#[test]
fn test_decode_frame_empty_data_returns_empty() {
    assert_eq!(decode_frame(b"TIT2", &[]).unwrap(), FrameContent::Empty);
}

#[test]
fn test_decode_frame_any_t_frame_is_text() {
    let mut data = vec![3];
    data.extend_from_slice(b"Queen");

    for id in [
        b"TIT2", b"TPE1", b"TPE2", b"TALB", b"TRCK", b"TCON", b"TCOM", b"TPUB", b"TYER",
        b"TSSE",
    ] {
        assert!(
            matches!(decode_frame(id, &data).unwrap(), FrameContent::Text(_)),
            "frame {} devrait être décodée comme Text",
            String::from_utf8_lossy(id)
        );
    }
}

#[test]
fn test_decode_frame_txxx_is_not_a_plain_text_frame() {
    let mut data = vec![3];
    data.extend_from_slice(b"REPLAYGAIN\0-3.2 dB");

    assert_eq!(
        decode_frame(b"TXXX", &data).unwrap(),
        FrameContent::UserText {
            description: "REPLAYGAIN".to_string(),
            value: "-3.2 dB".to_string(),
        }
    );
}

#[test]
fn test_decode_frame_text_invalid_encoding_propagates_err() {
    let data = vec![9, b'H', b'i'];
    assert!(matches!(
        decode_frame(b"TIT2", &data),
        Err(Mp3Error::UnknownTextEncoding { encoding: 9 })
    ));
}

#[test]
fn test_decode_frame_comm_separates_language_and_description() {
    let mut data = vec![3];
    data.extend_from_slice(b"eng");
    data.extend_from_slice(b"\0Super chanson");

    assert_eq!(
        decode_frame(b"COMM", &data).unwrap(),
        FrameContent::FullText {
            language: "eng".to_string(),
            description: String::new(),
            text: "Super chanson".to_string(),
        }
    );
}

#[test]
fn test_decode_frame_comm_with_description() {
    let mut data = vec![3];
    data.extend_from_slice(b"fra");
    data.extend_from_slice(b"Note\0Excellent album");

    assert_eq!(
        decode_frame(b"COMM", &data).unwrap(),
        FrameContent::FullText {
            language: "fra".to_string(),
            description: "Note".to_string(),
            text: "Excellent album".to_string(),
        }
    );
}

#[test]
fn test_decode_frame_comm_utf16_like_real_file() {
    let data = [
        1, b'e', b'n', b'g', 0xFF, 0xFE, 0x00, 0x00, 0xFF, 0xFE, 0x20, 0x00,
    ];

    assert_eq!(
        decode_frame(b"COMM", &data).unwrap(),
        FrameContent::FullText {
            language: "eng".to_string(),
            description: String::new(),
            text: " ".to_string(),
        }
    );
}

#[test]
fn test_decode_frame_uslt_uses_same_layout_as_comm() {
    let mut data = vec![0];
    data.extend_from_slice(b"eng");
    data.extend_from_slice(b"XXX\0It's a kind of magic");

    assert_eq!(
        decode_frame(b"USLT", &data).unwrap(),
        FrameContent::FullText {
            language: "eng".to_string(),
            description: "XXX".to_string(),
            text: "It's a kind of magic".to_string(),
        }
    );
}

#[test]
fn test_decode_frame_comm_too_short_returns_err() {
    assert!(matches!(
        decode_frame(b"COMM", &[0, b'e', b'n']),
        Err(Mp3Error::InvalidTextData { .. })
    ));
}

#[test]
fn test_decode_frame_apic_extracts_mime_type() {
    let mut data = vec![0];
    data.extend_from_slice(b"image/jpeg\0");
    data.push(3);
    data.push(0);
    data.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);

    assert_eq!(
        decode_frame(b"APIC", &data).unwrap(),
        FrameContent::Picture {
            mime_type: "image/jpeg".to_string(),
            picture_type: 3,
            description: String::new(),
            data: vec![0xFF, 0xD8, 0xFF, 0xE0],
        }
    );
}

#[test]
fn test_decode_frame_apic_truncated_returns_err() {
    assert!(matches!(
        decode_frame(b"APIC", &[0, b'i', b'm', b'a']),
        Err(Mp3Error::InvalidTextData { .. })
    ));
}

#[test]
fn test_decode_frame_unknown_id() {
    let data = vec![1, 2, 3, 4, 5];
    assert_eq!(
        decode_frame(b"XXXX", &data).unwrap(),
        FrameContent::Unknown(data.clone())
    );
}
