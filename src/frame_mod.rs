//! Lecture des frames ID3v2 : repère leurs limites (en-tête de 6 octets
//! en ID3v2.2, 10 au-delà), gère l'unsynchronisation — globale ou propre
//! à une frame (ID3v2.4, voir [`FRAME_UNSYNCHRONISATION_FLAG`]) — puis
//! délègue le décodage du contenu au sous-module [`decode`].

use crate::error::Mp3Error;
use crate::id3::header::Id3Version;
use crate::id3::{deunsynchronize, synchsafe_to_u32};
use std::borrow::Cow;

mod decode;
use decode::decode_frame;
pub use decode::FrameContent;

/// Bit "Unsynchronisation" des format flags d'une frame ID3v2.4 (octet
/// bas des deux octets de [`Frame::flags`]) — voir
/// [`read_frame_v2_3_or_later`]. N'existe qu'en ID3v2.4 ; ID3v2.3 n'a pas
/// cette option par frame, seule l'unsynchronisation globale du tag (voir
/// [`crate::id3::deunsynchronize`]) y est possible.
const FRAME_UNSYNCHRONISATION_FLAG: u16 = 0x0002;
/// Une frame ID3v2 individuelle (ex. `TIT2` pour le titre, `TPE1` pour
/// l'artiste, `APIC` pour une image de couverture).
///
/// Une valeur `Frame` est typiquement construite par [`read_frame`], qui
/// analyse une frame à partir d'un décalage donné et en décode
/// immédiatement le contenu dans [`Frame::content`]. Les octets bruts de
/// la frame ne sont donc pas conservés, sauf pour les frames non
/// reconnues, où ils restent accessibles via [`FrameContent::Unknown`].
///
/// Pour un tag ID3v2.2, [`Frame::id`] porte l'identifiant *converti* vers
/// son équivalent 4 lettres (ex. `TT2` devient `TIT2`), voir
/// [`map_v2_2_id`] ; les frames sans équivalent connu conservent leurs 3
/// lettres d'origine, complétées d'un octet nul.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Identifiant de la frame sur 4 octets (ex. `b"TIT2"`, `b"TPE1"`).
    pub id: [u8; 4],
    /// Taille du corps de la frame en octets, telle que déclarée dans
    /// l'en-tête de la frame.
    pub size: u32,
    /// Flags de la frame. Toujours à 0 pour ID3v2.2, qui n'a pas ce champ.
    pub flags: u16,
    /// Contenu décodé de la frame.
    pub content: FrameContent,
    /// Décalage (en octets, depuis le début des données passées à
    /// [`read_frame`]) auquel cette frame a commencé à être lue.
    pub offset: usize,
    /// Décalage (en octets, dans le même référentiel que [`Frame::offset`])
    /// auquel commence la frame suivante.
    pub next_offset: usize,
}
impl Frame {
    /// Renvoie l'identifiant de la frame sous forme de chaîne lisible.
    pub fn id_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.id)
    }

    /// Renvoie la première valeur textuelle de la frame, si celle-ci porte
    /// du texte.
    ///
    /// Couvre [`FrameContent::Text`] (première valeur),
    /// [`FrameContent::UserText`] (la valeur, pas la description) et
    /// [`FrameContent::FullText`] (le texte, pas la description). Renvoie
    /// `None` pour une image, une frame vide ou une frame non reconnue.
    pub fn as_text(&self) -> Option<&str> {
        match &self.content {
            FrameContent::Text(values) => values.first().map(String::as_str),
            FrameContent::UserText { value, .. } => Some(value),
            FrameContent::FullText { text, .. } => Some(text),
            _ => None,
        }
    }
}
/// Affiche un résumé lisible d'une frame ID3v2 : identifiant, flags,
/// taille, position, et un aperçu de son contenu décodé.
impl std::fmt::Display for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = match &self.content {
            FrameContent::Empty => String::new(),
            FrameContent::Text(values) => values.join(" / "),
            FrameContent::UserText { description, value } => {
                format!("{description} = {value}")
            }
            FrameContent::FullText {
                language,
                description,
                text,
            } => {
                if description.is_empty() {
                    format!("[{language}] {text}")
                } else {
                    format!("[{language}] {description} : {text}")
                }
            }
            FrameContent::Picture {
                mime_type,
                picture_type,
                description,
                data,
            } => {
                format!(
                    "Image ({mime_type}, type {picture_type}, {} octets) {description}",
                    data.len()
                )
            }
            FrameContent::Unknown(data) => format!("Unknown data: {} octets", data.len()),
        };

        writeln!(f, "- FRAME -----------------------")?;
        writeln!(f, "Id          : {}", self.id_str())?;
        writeln!(f, "Flags       : {:02X}", self.flags)?;
        writeln!(f, "Taille      : {} octets", self.size)?;
        writeln!(f, "Offset      : {}", self.offset)?;
        writeln!(f, "Next offset : {}", self.next_offset)?;
        writeln!(f, "Data        : {data}")?;
        write!(f, "-------------------------------")
    }
}
/// Convertit un identifiant de frame ID3v2.2 (3 lettres) vers son
/// équivalent ID3v2.3/2.4 (4 lettres), pour les frames les plus courantes.
///
/// Un identifiant sans correspondance connue est conservé tel quel,
/// complété d'un octet nul en 4ᵉ position — heuristique qui suffit à
/// classer correctement les frames texte, puisque `decode_frame`
/// reconnaît toute frame texte à son premier caractère `T`.
fn map_v2_2_id(id: [u8; 3]) -> [u8; 4] {
    match &id {
        b"TT2" => *b"TIT2",
        b"TP1" => *b"TPE1",
        b"TP2" => *b"TPE2",
        b"TAL" => *b"TALB",
        b"TRK" => *b"TRCK",
        b"TYE" => *b"TYER",
        b"TCO" => *b"TCON",
        b"TCM" => *b"TCOM",
        b"COM" => *b"COMM",
        b"ULT" => *b"USLT",
        b"PIC" => *b"APIC",
        b"TXX" => *b"TXXX",
        _ => [id[0], id[1], id[2], 0],
    }
}
/// Lit une frame ID3v2 à partir d'un décalage donné, et en décode le
/// contenu.
///
/// La disposition de l'en-tête de frame dépend de la version majeure
/// portée par `version` :
///
/// - **ID3v2.2** (`major == 2`) : en-tête de 6 octets (3 octets d'id + 3
///   octets de taille, big-endian brut, pas de flags). Voir
///   [`map_v2_2_id`] pour la conversion de l'identifiant.
/// - **ID3v2.3** (`major == 3`) : en-tête de 10 octets, taille en
///   big-endian brut sur 4 octets.
/// - **ID3v2.4** (`major >= 4`) : en-tête de 10 octets, mais la taille est
///   un entier *synchsafe* (voir [`synchsafe_to_u32`]) — un fichier v2.4
///   lu avec un décodage brut aurait une frame sur deux mal découpée dès
///   que sa taille dépasse 127 octets. Version qui introduit aussi
///   l'unsynchronisation propre à une frame individuelle (voir
///   [`FRAME_UNSYNCHRONISATION_FLAG`]), retirée ici avant décodage — en
///   plus de celle du tag entier (voir [`crate::id3::deunsynchronize`]),
///   la seule qu'ID3v2.3 connaisse.
///
/// `tag_already_unsynced` indique si l'appelant a déjà retiré
/// l'unsynchronisation globale du tag ([`crate::id3::deunsynchronize`])
/// avant d'appeler cette fonction : si c'est le cas, ce corps de frame est
/// déjà propre, et le bit d'unsynchronisation propre à la frame — même
/// posé — n'est alors plus consulté, pour ne pas appliquer
/// [`deunsynchronize`] une seconde fois sur des octets déjà nettoyés
/// (l'opération n'est pas idempotente : une vraie séquence `0xFF 0x00`
/// légitimement présente dans le contenu déjà propre serait tronquée par
/// erreur).
///
/// # Retour
///
/// - `Ok(Some(frame))` si une frame valide a pu être lue à `offset`.
/// - `Ok(None)` si l'id de la frame est composé uniquement d'octets nuls
///   (padding de fin de tag) : ce n'est pas une erreur.
///
/// # Erreurs
///
/// - [`Mp3Error::UnsupportedVersion`] si `version.major` est inférieur à 2
///   (aucune version ID3v2 valide ne descend en dessous).
/// - [`Mp3Error::FrameTooShort`] s'il ne reste pas assez d'octets pour un
///   en-tête de frame complet.
/// - [`Mp3Error::FrameSizeOverflow`] si la taille déclarée dépasse les
///   octets disponibles.
/// - Toute erreur de décodage renvoyée par [`decode_frame`].
pub fn read_frame(
    id3_data: &[u8],
    offset: usize,
    version: Id3Version,
    tag_already_unsynced: bool,
) -> Result<Option<Frame>, Mp3Error> {
    match version.major {
        0 | 1 => Err(Mp3Error::UnsupportedVersion {
            major: version.major,
        }),
        2 => read_frame_v2_2(id3_data, offset),
        3 => read_frame_v2_3_or_later(id3_data, offset, false, tag_already_unsynced),
        _ => read_frame_v2_3_or_later(id3_data, offset, true, tag_already_unsynced),
    }
}
/// Lit une frame au format ID3v2.2 : en-tête de 6 octets, sans flags.
fn read_frame_v2_2(id3_data: &[u8], offset: usize) -> Result<Option<Frame>, Mp3Error> {
    let header_end = offset
        .checked_add(6)
        .ok_or(Mp3Error::FrameTooShort { offset })?;
    if id3_data.len() < header_end {
        return Err(Mp3Error::FrameTooShort { offset });
    }

    let raw_id: [u8; 3] = id3_data[offset..offset + 3]
        .try_into()
        .expect("slice de 3 octets, conversion infaillible");

    if raw_id == [0, 0, 0] {
        // Padding de fin de tag.
        return Ok(None);
    }

    let size_bytes = &id3_data[offset + 3..header_end];
    let size = u32::from_be_bytes([0, size_bytes[0], size_bytes[1], size_bytes[2]]);

    let frame_end = header_end
        .checked_add(size as usize)
        .filter(|&end| end <= id3_data.len())
        .ok_or(Mp3Error::FrameSizeOverflow {
            offset,
            declared: size,
            available: id3_data.len(),
        })?;

    let frame_id = map_v2_2_id(raw_id);
    let content = decode_frame(&frame_id, &id3_data[header_end..frame_end])?;

    Ok(Some(Frame {
        id: frame_id,
        size,
        flags: 0,
        content,
        offset,
        next_offset: frame_end,
    }))
}
/// Lit une frame au format ID3v2.3 ou ID3v2.4 : en-tête de 10 octets.
/// `is_v2_4_or_later` sélectionne le décodage de taille approprié (entier
/// synchsafe ou brut) et détermine si le bit
/// [`FRAME_UNSYNCHRONISATION_FLAG`] doit être consulté (propre à
/// ID3v2.4). `tag_already_unsynced` désactive cette consultation même
/// s'il est posé — voir [`read_frame`].
fn read_frame_v2_3_or_later(
    id3_data: &[u8],
    offset: usize,
    is_v2_4_or_later: bool,
    tag_already_unsynced: bool,
) -> Result<Option<Frame>, Mp3Error> {
    let header_end = offset
        .checked_add(10)
        .ok_or(Mp3Error::FrameTooShort { offset })?;
    if id3_data.len() < header_end {
        return Err(Mp3Error::FrameTooShort { offset });
    }

    let frame_id: [u8; 4] = id3_data[offset..offset + 4]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");

    if frame_id == [0, 0, 0, 0] {
        // Padding de fin de tag.
        return Ok(None);
    }

    let size_bytes: [u8; 4] = id3_data[offset + 4..offset + 8]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");
    let size = if is_v2_4_or_later {
        synchsafe_to_u32(size_bytes)
    } else {
        u32::from_be_bytes(size_bytes)
    };
    let flags = u16::from_be_bytes(
        id3_data[offset + 8..header_end]
            .try_into()
            .expect("slice de 2 octets, conversion infaillible"),
    );

    let frame_end = header_end
        .checked_add(size as usize)
        .filter(|&end| end <= id3_data.len())
        .ok_or(Mp3Error::FrameSizeOverflow {
            offset,
            declared: size,
            available: id3_data.len(),
        })?;

    let raw_frame_data = &id3_data[header_end..frame_end];

    // Unsynchronisation propre à cette frame (ID3v2.4 uniquement — voir
    // FRAME_UNSYNCHRONISATION_FLAG) : ne s'applique que si le tag entier
    // n'a pas déjà été désunsynchronisé plus haut (voir la doc de
    // `read_frame` pour pourquoi appliquer les deux serait incorrect,
    // pas seulement redondant).
    let should_deunsync =
        !tag_already_unsynced && is_v2_4_or_later && flags & FRAME_UNSYNCHRONISATION_FLAG != 0;
    let frame_data: Cow<[u8]> = if should_deunsync {
        Cow::Owned(deunsynchronize(raw_frame_data))
    } else {
        Cow::Borrowed(raw_frame_data)
    };

    let content = decode_frame(&frame_id, &frame_data)?;

    Ok(Some(Frame {
        id: frame_id,
        size,
        flags,
        content,
        offset,
        next_offset: frame_end,
    }))
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests {
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
}
