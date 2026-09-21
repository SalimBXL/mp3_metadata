use super::*;
use tempfile::Builder;

#[test]
fn test_mp3_file_extension() {
    assert!(!mp3_file_exists("a_kind_of_magic.txt").unwrap());
}

#[test]
fn test_mp3_file_exists() {
    let named_tempfile = Builder::new()
        .prefix("my-temporary-note")
        .suffix(".mp3")
        .rand_bytes(5)
        .tempfile()
        .unwrap();
    assert!(mp3_file_exists(named_tempfile.path()).unwrap());
}

#[test]
fn test_mp3_file_does_not_exist() {
    assert!(!mp3_file_exists("non_existent_file.mp3").unwrap());
}

#[test]
fn test_read_mp3_file_not_found() {
    assert!(matches!(
        read_mp3_file("non_existent_file.mp3"),
        Err(Mp3Error::NotFound(_))
    ));
}

#[test]
fn test_read_mp3_file_without_id3_tag() {
    // Un fichier .mp3 sans tag ID3v2 se lit sans erreur.
    use std::io::Write;

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&[0xFF; 64]).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();

    assert!(mp3.id3v2.is_none());
    assert_eq!(mp3.size, 64);
    // 0xFF répété ressemble au repère de synchronisation mais échoue
    // sur l'index de débit (1111, réservé) : aucune frame valide.
    assert!(mp3.audio_format.is_none());
}

/// En-tête de frame MPEG-1 Layer III, 320 kbps, 44100 Hz, stéréo —
/// calculé bit à bit, voir mpeg.rs.
const MPEG_FRAME_HEADER: [u8; 4] = [0xFF, 0xFB, 0xE0, 0x00];

#[test]
fn test_read_mp3_file_detects_audio_format_after_tag() {
    use std::io::Write;

    let (tag, _) = tag_with_trailer(0);
    let mut trailer = MPEG_FRAME_HEADER.to_vec();
    trailer.extend_from_slice(&[0xAAu8; 996]); // ~1000 octets d'audio "réels"

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&trailer).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();
    let format = mp3
        .audio_format
        .expect("une frame MPEG valide était présente");

    assert_eq!(format.header.bitrate_kbps, 320);
    assert_eq!(format.header.sample_rate_hz, 44100);
    // 1000 octets à 320 kbps : quelques centièmes de seconde.
    assert!(format.duration_secs > 0.0 && format.duration_secs < 1.0);
}

#[test]
fn test_read_mp3_file_without_tag_detects_audio_format_from_byte_zero() {
    use std::io::Write;

    let mut data = MPEG_FRAME_HEADER.to_vec();
    data.extend_from_slice(&[0xAAu8; 100]);

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&data).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();
    let format = mp3
        .audio_format
        .expect("une frame MPEG valide était présente dès le début");

    assert_eq!(format.header.bitrate_kbps, 320);
}

#[test]
fn test_read_mp3_file_with_audio_includes_bytes_consumed_by_the_probe() {
    // Régression : la sonde MPEG avance le curseur du fichier au-delà
    // du début de l'audio. Sans un retour en arrière avant la lecture
    // complète, --load-audio perdrait les tout premiers octets.
    use std::io::Write;

    let (tag, _) = tag_with_trailer(0);
    let mut trailer = MPEG_FRAME_HEADER.to_vec();
    trailer.extend_from_slice(&[0xAAu8; 100]);

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&trailer).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file_with_audio(file.path()).unwrap();

    assert_eq!(mp3.audio.map(|a| a.data), Some(trailer));
}

/// Construit un tag ID3v2.3 minimal (une seule frame TIT2) suivi
/// d'octets qui simulent des données audio.
fn tag_with_trailer(trailer_len: usize) -> (Vec<u8>, Vec<u8>) {
    let mut tag = Vec::new();
    tag.extend_from_slice(b"ID3");
    tag.extend_from_slice(&[3, 0, 0]); // version 2.3.0, flags 0
    tag.extend_from_slice(&[0, 0, 0, 13]); // taille synchsafe du corps : 13
    tag.extend_from_slice(b"TIT2");
    tag.extend_from_slice(&3u32.to_be_bytes()); // taille de frame
    tag.extend_from_slice(&[0, 0]); // flags de frame
    tag.extend_from_slice(&[3, b'H', b'i']); // encoding UTF-8, "Hi"

    let trailer = vec![0xAA; trailer_len];
    (tag, trailer)
}

#[test]
fn test_read_mp3_file_reads_tag_without_loading_the_trailer() {
    use std::io::Write;

    let (tag, trailer) = tag_with_trailer(500);
    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&trailer).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();

    assert_eq!(mp3.id3v2.as_ref().and_then(|t| t.title()), Some("Hi"));
    assert_eq!(mp3.size, tag.len() + trailer.len());
    assert!(mp3.audio.is_none());
}

#[test]
fn test_read_mp3_file_with_audio_loads_the_trailer() {
    use std::io::Write;

    let (tag, trailer) = tag_with_trailer(500);
    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&trailer).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file_with_audio(file.path()).unwrap();

    assert_eq!(mp3.audio.map(|a| a.data), Some(trailer));
}

/// Construit les 128 octets d'un tag ID3v1 minimal (titre "Hi" en
/// ID3v1.1, piste 7).
fn id3v1_bytes(title: &str) -> [u8; 128] {
    let mut tag = [0u8; 128];
    tag[0..3].copy_from_slice(b"TAG");
    let title_bytes = title.as_bytes();
    tag[3..3 + title_bytes.len()].copy_from_slice(title_bytes);
    tag[97 + 28] = 0; // marqueur ID3v1.1
    tag[97 + 29] = 7; // numéro de piste
    tag[127] = 17; // Rock
    tag
}

#[test]
fn test_read_mp3_file_reads_trailing_id3v1_tag() {
    use std::io::Write;

    let (tag, _) = tag_with_trailer(0);
    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&id3v1_bytes("Hi")).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();
    let id3v1 = mp3.id3v1.expect("un tag ID3v1 était présent en fin de fichier");

    assert_eq!(id3v1.title, "Hi");
    assert_eq!(id3v1.track, Some(7));
    assert_eq!(id3v1.genre_name(), Some("Rock"));
    // Le tag ID3v2 lu au début du fichier reste indépendant du tag
    // ID3v1 lu à la fin.
    assert_eq!(mp3.id3v2.as_ref().and_then(|t| t.title()), Some("Hi"));
}

/// Construit un tag ID3v2.3 avec tous les champs communs à ID3v1
/// (title, artist, album, year, comment, track, genre), pour vérifier
/// l'alignement de l'affichage côte à côte des deux formats.
fn full_id3v2_tag_bytes() -> Vec<u8> {
    fn frame(id: &[u8; 4], encoding_and_text: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(id);
        data.extend_from_slice(&(encoding_and_text.len() as u32).to_be_bytes());
        data.extend_from_slice(&[0, 0]); // flags
        data.extend_from_slice(encoding_and_text);
        data
    }
    fn text(s: &str) -> Vec<u8> {
        let mut body = vec![3]; // UTF-8
        body.extend_from_slice(s.as_bytes());
        body
    }

    let mut body = frame(b"TIT2", &text("Titre"));
    body.extend(frame(b"TPE1", &text("Artiste")));
    body.extend(frame(b"TALB", &text("Album")));
    body.extend(frame(b"TYER", &text("1999")));
    let mut comm = vec![3];
    comm.extend_from_slice(b"eng\0Commentaire");
    body.extend(frame(b"COMM", &comm));
    body.extend(frame(b"TRCK", &text("3/12")));
    body.extend(frame(b"TCON", &text("Rock")));

    let mut tag = Vec::new();
    tag.extend_from_slice(b"ID3");
    tag.extend_from_slice(&[3, 0, 0]); // version 2.3.0, flags 0
    let size = body.len() as u32;
    tag.push(((size >> 21) & 0x7F) as u8);
    tag.push(((size >> 14) & 0x7F) as u8);
    tag.push(((size >> 7) & 0x7F) as u8);
    tag.push((size & 0x7F) as u8);
    tag.extend_from_slice(&body);
    tag
}

#[test]
fn test_id3v1_and_id3v2_display_align_shared_fields_line_by_line() {
    use std::io::Write;

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&full_id3v2_tag_bytes()).unwrap();
    file.write_all(&id3v1_bytes("Titre")).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();
    let id3v2_text = mp3.id3v2.unwrap().to_string();
    let id3v1_text = mp3.id3v1.unwrap().to_string();

    let line_of = |text: &str, label: &str| {
        text.lines()
            .position(|l| l.starts_with(label))
            .unwrap_or_else(|| panic!("champ {label:?} introuvable dans {text:?}"))
    };

    for label in ["Title", "Artist", "Album", "Year", "Comment", "Track", "Genre"] {
        assert_eq!(
            line_of(&id3v2_text, label),
            line_of(&id3v1_text, label),
            "le champ {label:?} n'est pas sur la même ligne dans les deux affichages"
        );
    }
}

#[test]
fn test_read_mp3_file_without_trailing_tag_has_no_id3v1() {
    use std::io::Write;

    // Trailer assez long pour que les 128 derniers octets du fichier
    // soient entièrement à l'intérieur (donc bien testés), sans
    // commencer par la signature "TAG".
    let (tag, trailer) = tag_with_trailer(200);
    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&tag).unwrap();
    file.write_all(&trailer).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();

    assert!(mp3.id3v1.is_none());
}

#[test]
fn test_read_mp3_file_shorter_than_id3v1_tag_has_no_id3v1() {
    use std::io::Write;

    // 20 octets : plus que les 10 requis pour l'en-tête, mais moins
    // que les 128 d'un tag ID3v1 -- aucune tentative de lecture ne
    // doit être faite en dessous de la fin du fichier.
    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&[0xFFu8; 20]).unwrap();
    file.flush().unwrap();

    let mp3 = read_mp3_file(file.path()).unwrap();

    assert!(mp3.id3v1.is_none());
}

#[test]
fn test_read_mp3_file_invalid_tag_size_does_not_require_the_declared_bytes_to_exist() {
    // L'en-tête annonce un corps de 100 octets, mais le fichier ne
    // contient que 5 octets après l'en-tête : l'erreur doit venir de
    // la taille annoncée, et non d'un échec de lecture générique --
    // ce qui suppose de l'avoir détectée sans tenter de lire les 100
    // octets promis.
    use std::io::Write;

    let mut data = Vec::new();
    data.extend_from_slice(b"ID3");
    data.extend_from_slice(&[3, 0, 0]);
    data.extend_from_slice(&[0, 0, 0, 100]); // annonce 100 octets...
    data.extend_from_slice(&[0u8; 5]); // ...mais il n'y en a que 5

    let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
    file.write_all(&data).unwrap();
    file.flush().unwrap();

    assert!(matches!(
        read_mp3_file(file.path()),
        Err(Mp3Error::InvalidTagSize {
            declared: 100,
            available
        }) if available == data.len()
    ));
}
