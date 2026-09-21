use super::*;

// ----- AudioFormat -----

#[test]
fn test_audio_format_duration_at_320kbps() {
    let header = parse_frame_header([0xFF, 0xFB, 0xE0, 0x00]).unwrap();
    // 320 kbps pendant 10 secondes = 320_000 * 10 / 8 octets.
    let audio_bytes = 320_000 * 10 / 8;

    let format = AudioFormat::from_header_and_audio_bytes(header, audio_bytes);

    assert!((format.duration_secs - 10.0).abs() < 0.01);
}

#[test]
fn test_audio_format_display_matches_expected_layout() {
    let header = parse_frame_header([0xFF, 0xFB, 0xE0, 0x00]).unwrap();
    // 320 kbps pendant 264 secondes (4 min 24 s) tapées rond.
    let format = AudioFormat::from_header_and_audio_bytes(header, 320_000 * 264 / 8);
    let text = format.to_string();

    assert!(text.starts_with("Audio\n"));
    assert!(text.contains("MPEG       : MPEG-1 Layer III"));
    assert!(text.contains("Bitrate    : 320 kbps"));
    assert!(text.contains("Sample rate: 44.1 kHz"));
    assert!(text.contains("Channels   : Stereo"));
    // "~" : c'est une estimation CBR, pas une durée exacte (voir la
    // construction via `from_header_and_audio_bytes`, sans en-tête
    // Xing/Info/VBRI).
    assert!(text.contains("Duration   : ~4:24"));
}
// ----- samples_per_frame -----

#[test]
fn test_samples_per_frame_mpeg1_layer3() {
    assert_eq!(
        samples_per_frame(MpegVersion::V1, MpegLayer::LayerIII),
        1152
    );
}

#[test]
fn test_samples_per_frame_mpeg2_layer3_is_half_of_mpeg1() {
    assert_eq!(samples_per_frame(MpegVersion::V2, MpegLayer::LayerIII), 576);
    assert_eq!(
        samples_per_frame(MpegVersion::V2_5, MpegLayer::LayerIII),
        576
    );
}

#[test]
fn test_samples_per_frame_layer1_and_2_do_not_depend_on_version() {
    assert_eq!(samples_per_frame(MpegVersion::V1, MpegLayer::LayerI), 384);
    assert_eq!(samples_per_frame(MpegVersion::V2, MpegLayer::LayerI), 384);
    assert_eq!(samples_per_frame(MpegVersion::V1, MpegLayer::LayerII), 1152);
    assert_eq!(samples_per_frame(MpegVersion::V2, MpegLayer::LayerII), 1152);
}
// ----- side_info_len -----

#[test]
fn test_side_info_len_mpeg1_stereo_is_32() {
    assert_eq!(side_info_len(MpegVersion::V1, ChannelMode::Stereo), 32);
    assert_eq!(side_info_len(MpegVersion::V1, ChannelMode::JointStereo), 32);
    assert_eq!(side_info_len(MpegVersion::V1, ChannelMode::DualChannel), 32);
}

#[test]
fn test_side_info_len_mpeg1_mono_is_17() {
    assert_eq!(side_info_len(MpegVersion::V1, ChannelMode::Mono), 17);
}

#[test]
fn test_side_info_len_mpeg2_stereo_is_17() {
    assert_eq!(side_info_len(MpegVersion::V2, ChannelMode::Stereo), 17);
    assert_eq!(side_info_len(MpegVersion::V2_5, ChannelMode::Stereo), 17);
}

#[test]
fn test_side_info_len_mpeg2_mono_is_9() {
    assert_eq!(side_info_len(MpegVersion::V2, ChannelMode::Mono), 9);
}
// ----- parse_xing_stream_info -----

/// Champs utilisés par `xing_frame` — un struct plutôt que six
/// paramètres positionnels, pour que chaque valeur (`0x1`, `2000`,
/// `None`...) soit nommée à l'appel plutôt qu'à deviner par sa
/// position.
struct XingFrameFields<'a> {
    version: MpegVersion,
    channel_mode: ChannelMode,
    tag: &'a [u8; 4],
    flags: u32,
    frame_count: u32,
    total_bytes: Option<u32>,
}

/// Construit une frame factice : en-tête à 4 octets, information
/// annexe silencieuse (des zéros suffisent, on ne la décode pas), puis
/// un en-tête Xing/Info à `tag`, avec `flags` et, dans cet ordre, le
/// nombre de frames puis le nombre d'octets si les bits correspondants
/// sont posés.
fn xing_frame(fields: XingFrameFields) -> Vec<u8> {
    let mut frame = vec![0u8; 4 + side_info_len(fields.version, fields.channel_mode)];
    frame.extend_from_slice(fields.tag);
    frame.extend_from_slice(&fields.flags.to_be_bytes());
    if fields.flags & 0x1 != 0 {
        frame.extend_from_slice(&fields.frame_count.to_be_bytes());
    }
    if let Some(bytes) = fields.total_bytes {
        frame.extend_from_slice(&bytes.to_be_bytes());
    }
    frame
}

#[test]
fn test_parse_xing_stream_info_frame_count_only() {
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x1, // seulement le bit "nombre de frames"
        frame_count: 1234,
        total_bytes: None,
    });

    let info = parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).unwrap();

    assert_eq!(info.frame_count, 1234);
    assert_eq!(info.total_bytes, None);
}

#[test]
fn test_parse_xing_stream_info_frame_count_and_bytes() {
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x3, // bits "nombre de frames" et "nombre d'octets"
        frame_count: 1234,
        total_bytes: Some(999_999),
    });

    let info = parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).unwrap();

    assert_eq!(info.frame_count, 1234);
    assert_eq!(info.total_bytes, Some(999_999));
}

#[test]
fn test_parse_xing_stream_info_recognizes_info_tag_too() {
    // "Info" : LAME en CBR, mêmes métadonnées que "Xing" en VBR.
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Info",
        flags: 0x1,
        frame_count: 1234,
        total_bytes: None,
    });
    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_some());
}

#[test]
fn test_parse_xing_stream_info_none_without_frame_count_bit() {
    // Bit 0 non posé : rien d'exploitable, même avec l'étiquette présente.
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x0,
        frame_count: 1234,
        total_bytes: None,
    });
    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_none());
}

#[test]
fn test_parse_xing_stream_info_none_without_recognized_tag() {
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Nope",
        flags: 0x1,
        frame_count: 1234,
        total_bytes: None,
    });
    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_none());
}

#[test]
fn test_parse_xing_stream_info_uses_the_right_offset_per_channel_mode() {
    // Même en-tête Xing, mais l'information annexe qui le précède n'a
    // pas la même taille en mono : le chercher au décalage stéréo sur
    // une frame mono doit échouer.
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Mono,
        tag: b"Xing",
        flags: 0x1,
        frame_count: 1234,
        total_bytes: None,
    });

    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Mono).is_some());
    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_none());
}

#[test]
fn test_parse_xing_stream_info_truncated_frame_returns_none() {
    // En-tête Xing annoncé mais coupé avant son champ de flags.
    let mut frame = vec![0u8; 4 + side_info_len(MpegVersion::V1, ChannelMode::Stereo)];
    frame.extend_from_slice(b"Xing");
    assert!(parse_xing_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_none());
}
// ----- parse_vbri_stream_info -----

fn vbri_frame(frame_count: u32, total_bytes: u32) -> Vec<u8> {
    let mut frame = vec![0u8; VBRI_OFFSET];
    frame.extend_from_slice(b"VBRI");
    frame.extend_from_slice(&1u16.to_be_bytes()); // version
    frame.extend_from_slice(&0u16.to_be_bytes()); // délai
    frame.extend_from_slice(&0u16.to_be_bytes()); // qualité
    frame.extend_from_slice(&total_bytes.to_be_bytes());
    frame.extend_from_slice(&frame_count.to_be_bytes());
    frame
}

#[test]
fn test_parse_vbri_stream_info_reads_frame_count_and_bytes() {
    let frame = vbri_frame(5678, 2_000_000);
    let info = parse_vbri_stream_info(&frame).unwrap();

    assert_eq!(info.frame_count, 5678);
    assert_eq!(info.total_bytes, Some(2_000_000));
}

#[test]
fn test_parse_vbri_stream_info_fixed_offset_ignores_channel_mode() {
    // Contrairement à Xing/Info, l'offset ne dépend ni de la version
    // ni du mode de canaux : un en-tête VBRI construit sans egard pour
    // ces deux paramètres doit quand même être trouvé.
    let frame = vbri_frame(1, 1);
    assert!(parse_vbri_stream_info(&frame).is_some());
}

#[test]
fn test_parse_vbri_stream_info_none_without_tag() {
    let frame = vec![0u8; VBRI_OFFSET + 20];
    assert!(parse_vbri_stream_info(&frame).is_none());
}
// ----- vbr_stream_info -----

#[test]
fn test_vbr_stream_info_prefers_xing_over_vbri() {
    // Un fichier ne devrait jamais porter les deux, mais si c'était le
    // cas, Xing/Info est cherché en premier (voir la doc).
    let frame = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x1,
        frame_count: 111,
        total_bytes: None,
    });
    let info = vbr_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).unwrap();
    assert_eq!(info.frame_count, 111);
}

#[test]
fn test_vbr_stream_info_falls_back_to_vbri() {
    let frame = vbri_frame(222, 50_000);
    let info = vbr_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).unwrap();
    assert_eq!(info.frame_count, 222);
}

#[test]
fn test_vbr_stream_info_none_when_neither_present() {
    let frame = vec![0u8; 200];
    assert!(vbr_stream_info(&frame, MpegVersion::V1, ChannelMode::Stereo).is_none());
}
// ----- AudioFormat::from_probe -----

fn mpeg1_stereo_frame_header_bytes() -> [u8; 4] {
    [0xFF, 0xFB, 0xE0, 0x00] // MPEG1 Layer III, 320 kbps, 44100 Hz, stéréo
}

#[test]
fn test_from_probe_uses_xing_frame_count_for_exact_duration() {
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    // `header` étant déjà décodé et passé séparément, les 4 premiers
    // octets de la frame elle-même n'ont pas besoin d'être un vrai
    // en-tête : seul leur décalage compte (voir xing_frame).
    let mut probe = vec![0xAAu8; 10]; // remplissage avant la frame
    probe.extend(xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x1,
        frame_count: 2000, // 2000 frames Layer III MPEG1 = 2000 * 1152 échantillons
        total_bytes: None,
    }));

    let format = AudioFormat::from_probe(&probe, 10, header, 999_999_999);

    assert!(format.duration_is_exact);
    let expected_secs: f64 = 2000.0 * 1152.0 / 44100.0;
    assert!((format.duration_secs - expected_secs).abs() < 0.001);
}

#[test]
fn test_from_probe_computes_average_bitrate_from_declared_bytes() {
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    let probe = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x3,
        frame_count: 2000,
        total_bytes: Some(1_000_000),
    });

    let format = AudioFormat::from_probe(&probe, 0, header, 0);
    let expected_secs: f64 = 2000.0 * 1152.0 / 44100.0;
    let expected_kbps = ((1_000_000.0 * 8.0) / expected_secs / 1000.0).round() as u32;

    assert_eq!(format.average_bitrate_kbps, Some(expected_kbps));
}

#[test]
fn test_from_probe_average_bitrate_falls_back_to_audio_bytes_without_declared_size() {
    // L'en-tête ne déclare pas le nombre d'octets (bit 1 absent) :
    // `audio_bytes`, passé par l'appelant, sert de repli.
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    let probe = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x1,
        frame_count: 2000,
        total_bytes: None,
    });

    let format = AudioFormat::from_probe(&probe, 0, header, 500_000);
    let expected_secs: f64 = 2000.0 * 1152.0 / 44100.0;
    let expected_kbps = ((500_000.0 * 8.0) / expected_secs / 1000.0).round() as u32;

    assert_eq!(format.average_bitrate_kbps, Some(expected_kbps));
}

#[test]
fn test_from_probe_falls_back_to_cbr_estimate_without_vbr_header() {
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    let probe = mpeg1_stereo_frame_header_bytes().to_vec(); // pas de Xing/VBRI
    let audio_bytes = 320_000 * 10 / 8; // 320 kbps pendant 10 s

    let format = AudioFormat::from_probe(&probe, 0, header, audio_bytes);

    assert!(!format.duration_is_exact);
    assert_eq!(format.average_bitrate_kbps, None);
    assert!((format.duration_secs - 10.0).abs() < 0.01);
}

#[test]
fn test_from_probe_frame_offset_past_probe_end_falls_back_to_cbr() {
    // `frame_offset` hors bornes de `probe` : ne doit jamais paniquer,
    // seulement se rabattre sur l'estimation CBR (aucun en-tête
    // Xing/VBRI trouvable dans une fenêtre vide).
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    let probe = mpeg1_stereo_frame_header_bytes().to_vec();

    let format = AudioFormat::from_probe(&probe, 999, header, 320_000 * 10 / 8);

    assert!(!format.duration_is_exact);
}

#[test]
fn test_display_shows_average_bitrate_label_when_present() {
    let header = parse_frame_header(mpeg1_stereo_frame_header_bytes()).unwrap();
    let probe = xing_frame(XingFrameFields {
        version: MpegVersion::V1,
        channel_mode: ChannelMode::Stereo,
        tag: b"Xing",
        flags: 0x3,
        frame_count: 2000,
        total_bytes: Some(1_000_000),
    });
    let format = AudioFormat::from_probe(&probe, 0, header, 0);
    let text = format.to_string();

    assert!(text.contains("(moyen)"));
    assert!(!text.contains("Duration   : ~")); // durée exacte, pas de tilde
}
