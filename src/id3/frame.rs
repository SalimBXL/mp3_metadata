use crate::error::Mp3Error;
use crate::id3::header::Id3Version;
use crate::id3::synchsafe_to_u32;

/// Contenu décodé d'une frame ID3v2.
///
/// Une valeur `FrameContent` est produite par [`decode_frame`], qui choisit
/// la variante appropriée en fonction de l'identifiant de la frame
/// (`TIT2`, `APIC`, `COMM`, etc.), et est stockée dans [`Frame::content`]
/// au moment de la lecture.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameContent {
    /// Frame vide : l'en-tête déclare une taille de 0 octet, il n'y a rien
    /// à décoder. Ce n'est pas une erreur.
    Empty,

    /// Contenu d'une frame texte, c'est-à-dire toute frame dont
    /// l'identifiant commence par `T`, à l'exception de `TXXX`
    /// (voir [`FrameContent::UserText`]).
    ///
    /// Le `Vec` contient une entrée par valeur : la spécification ID3v2.4
    /// autorise plusieurs valeurs dans une même frame texte, séparées par
    /// un caractère nul. En ID3v2.3 il n'y a en pratique qu'une seule
    /// valeur, et le `Vec` est de longueur 1.
    Text(Vec<String>),

    /// Contenu d'une frame `TXXX` : une paire description / valeur définie
    /// par l'application qui a écrit le tag.
    UserText { description: String, value: String },

    /// Contenu d'une frame `COMM` (commentaire) ou `USLT` (paroles non
    /// synchronisées). Ces deux frames partagent exactement la même
    /// disposition : octet d'encoding, code langue sur 3 octets,
    /// description terminée par un nul, puis le texte.
    FullText {
        /// Code langue ISO-639-2 sur 3 caractères (ex. `eng`, `fra`).
        language: String,
        /// Description courte, souvent vide.
        description: String,
        text: String,
    },

    /// Contenu d'une frame `APIC` (image jointe, ex. pochette d'album).
    ///
    /// Décodage valable pour ID3v2.3 et ID3v2.4. En ID3v2.2, la frame
    /// équivalente (`PIC`) code le format d'image sur 3 lettres (`JPG`,
    /// `PNG`, ...) plutôt qu'une chaîne MIME terminée par un nul : sur un
    /// tag v2.2, `mime_type` et `description` seront mal découpés. Ce cas
    /// n'est pas géré.
    Picture {
        /// Type MIME de l'image, tel que déclaré dans la frame
        /// (ex. `image/jpeg`).
        mime_type: String,
        /// Rôle de l'image selon la spécification ID3v2 (`3` = pochette
        /// avant, `4` = pochette arrière, etc.).
        picture_type: u8,
        description: String,
        /// Données brutes de l'image.
        data: Vec<u8>,
    },

    /// Contenu brut d'une frame dont l'identifiant n'est pas reconnu.
    Unknown(Vec<u8>),
}

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
///   que sa taille dépasse 127 octets.
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
) -> Result<Option<Frame>, Mp3Error> {
    match version.major {
        0 | 1 => Err(Mp3Error::UnsupportedVersion {
            major: version.major,
        }),
        2 => read_frame_v2_2(id3_data, offset),
        3 => read_frame_v2_3_or_later(id3_data, offset, false),
        _ => read_frame_v2_3_or_later(id3_data, offset, true),
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
/// `synchsafe_size` sélectionne le décodage de taille approprié.
fn read_frame_v2_3_or_later(
    id3_data: &[u8],
    offset: usize,
    synchsafe_size: bool,
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
    let size = if synchsafe_size {
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

    let content = decode_frame(&frame_id, &id3_data[header_end..frame_end])?;

    Ok(Some(Frame {
        id: frame_id,
        size,
        flags,
        content,
        offset,
        next_offset: frame_end,
    }))
}

/// Décode le corps d'une frame ID3v2 selon son identifiant.
///
/// - Toute frame dont l'identifiant commence par `T`, sauf `TXXX`, est une
///   frame texte et passe par [`decode_text_values`].
/// - `TXXX` porte une paire description / valeur définie par l'utilisateur.
/// - `COMM` et `USLT` partagent la disposition langue / description / texte.
/// - `APIC` porte une image, avec son type MIME et sa description.
/// - Toute autre frame est conservée telle quelle dans
///   [`FrameContent::Unknown`].
///
/// # Erreurs
///
/// - [`Mp3Error::UnknownTextEncoding`] si l'octet d'encoding en tête du
///   corps de la frame n'est pas l'une des quatre valeurs reconnues.
/// - [`Mp3Error::InvalidTextData`] si le texte est mal formé, ou si un
///   champ terminé par un nul ne l'est pas (frame tronquée ou corrompue).
fn decode_frame(frame_id: &[u8; 4], frame_data: &[u8]) -> Result<FrameContent, Mp3Error> {
    if frame_data.is_empty() {
        return Ok(FrameContent::Empty);
    }

    let content = match frame_id {
        b"TXXX" => {
            let (description, value) = decode_described_text(frame_data, 0)?;
            FrameContent::UserText { description, value }
        }

        // Toute autre frame `T***` est une frame texte.
        [b'T', ..] => FrameContent::Text(decode_text_values(frame_data)?),

        b"COMM" | b"USLT" => {
            // Octet d'encoding, puis 3 octets de code langue, puis la
            // disposition description / texte.
            if frame_data.len() < 4 {
                return Err(Mp3Error::InvalidTextData {
                    encoding: frame_data[0],
                });
            }
            let language = frame_data[1..4].iter().map(|&b| b as char).collect();
            let (description, text) = decode_described_text(frame_data, 3)?;
            FrameContent::FullText {
                language,
                description,
                text,
            }
        }

        b"APIC" => decode_picture_frame(frame_data)?,

        _ => FrameContent::Unknown(frame_data.to_vec()),
    };

    Ok(content)
}

/// Décode le corps d'une frame `APIC`.
///
/// Disposition : octet d'encoding, type MIME en ISO-8859-1 terminé par un
/// nul, octet de type d'image, description terminée par un nul (dans
/// l'encoding déclaré), puis les données brutes de l'image.
fn decode_picture_frame(frame_data: &[u8]) -> Result<FrameContent, Mp3Error> {
    let (&encoding, rest) = frame_data
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding: 0 })?;

    // Le type MIME est toujours en ISO-8859-1, quel que soit l'encoding
    // déclaré pour la description.
    let (mime_bytes, rest) =
        split_at_terminator(0, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;
    let mime_type = decode_string(0, mime_bytes)?;

    let (&picture_type, rest) = rest
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding })?;

    let (description_bytes, data) =
        split_at_terminator(encoding, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;
    let description = decode_string(encoding, description_bytes)?;

    Ok(FrameContent::Picture {
        mime_type,
        picture_type,
        description,
        data: data.to_vec(),
    })
}

/// Décode une frame bâtie sur le motif « description terminée par un nul,
/// puis texte » : `TXXX`, `COMM`, `USLT`.
///
/// `skip` est le nombre d'octets à ignorer entre l'octet d'encoding et la
/// description (0 pour `TXXX`, 3 pour le code langue de `COMM` et `USLT`).
fn decode_described_text(frame_data: &[u8], skip: usize) -> Result<(String, String), Mp3Error> {
    let (&encoding, rest) = frame_data
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding: 0 })?;

    let rest = rest
        .get(skip..)
        .ok_or(Mp3Error::InvalidTextData { encoding })?;

    let (description_bytes, text_bytes) =
        split_at_terminator(encoding, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;

    Ok((
        decode_string(encoding, description_bytes)?,
        decode_string(encoding, strip_trailing_terminator(encoding, text_bytes))?,
    ))
}

/// Décode le corps d'une frame texte en une liste de valeurs.
///
/// Le premier octet indique l'encoding, le reste contient une ou plusieurs
/// valeurs séparées par un caractère nul (plusieurs valeurs ne sont
/// légales qu'en ID3v2.4, mais les accepter partout est sans risque). Le
/// terminateur final, que beaucoup d'encodeurs ajoutent, ne produit pas de
/// valeur vide supplémentaire.
///
/// En UTF-16, chaque valeur porte son propre BOM : le découpage est fait
/// au niveau des octets, avant décodage, pour que chaque valeur soit
/// décodée avec le sien.
fn decode_text_values(frame_data: &[u8]) -> Result<Vec<String>, Mp3Error> {
    let Some((&encoding, mut rest)) = frame_data.split_first() else {
        // Pas d'octet d'encoding : rien à décoder, ce n'est pas une erreur.
        return Ok(Vec::new());
    };

    let mut values = Vec::new();
    while !rest.is_empty() {
        match split_at_terminator(encoding, rest) {
            Some((value_bytes, remainder)) => {
                values.push(decode_string(encoding, value_bytes)?);
                rest = remainder;
            }
            // Pas de terminateur : le reste forme la dernière valeur.
            None => {
                values.push(decode_string(encoding, rest)?);
                break;
            }
        }
    }

    Ok(values)
}

/// Découpe `data` au premier terminateur nul, et renvoie `(avant, après)`.
///
/// La taille du terminateur dépend de l'encoding : un octet nul pour
/// ISO-8859-1 et UTF-8, deux octets nuls alignés sur une frontière de
/// caractère pour les deux variantes d'UTF-16.
fn split_at_terminator(encoding: u8, data: &[u8]) -> Option<(&[u8], &[u8])> {
    match encoding {
        1 | 2 => {
            let index = data.chunks_exact(2).position(|pair| pair == [0, 0])? * 2;
            Some((&data[..index], &data[index + 2..]))
        }
        _ => {
            let index = data.iter().position(|&byte| byte == 0)?;
            Some((&data[..index], &data[index + 1..]))
        }
    }
}

/// Retire le terminateur nul final s'il est présent.
fn strip_trailing_terminator(encoding: u8, data: &[u8]) -> &[u8] {
    match encoding {
        1 | 2 => match data {
            [head @ .., 0, 0] if head.len() % 2 == 0 => head,
            _ => data,
        },
        _ => match data {
            [head @ .., 0] => head,
            _ => data,
        },
    }
}

/// Décode une suite d'octets en chaîne, selon un encoding ID3v2 :
///
/// - `0` : ISO-8859-1 (Latin-1)
/// - `1` : UTF-16 avec byte-order-mark (BOM), little ou big-endian
/// - `2` : UTF-16BE, sans BOM
/// - `3` : UTF-8 (ID3v2.4 uniquement)
fn decode_string(encoding: u8, data: &[u8]) -> Result<String, Mp3Error> {
    match encoding {
        0 => Ok(data.iter().map(|&byte| byte as char).collect()),

        1 => {
            if data.is_empty() {
                return Ok(String::new());
            }
            if data.len() < 2 {
                return Err(Mp3Error::InvalidTextData { encoding });
            }

            let bom = [data[0], data[1]];
            if !matches!(bom, [0xFF, 0xFE] | [0xFE, 0xFF]) {
                return Err(Mp3Error::InvalidTextData { encoding });
            }

            let little_endian = bom == [0xFF, 0xFE];
            decode_utf16(&data[2..], little_endian, encoding)
        }

        2 => decode_utf16(data, false, encoding),

        3 => std::str::from_utf8(data)
            .map(str::to_owned)
            .map_err(|_| Mp3Error::InvalidTextData { encoding }),

        _ => Err(Mp3Error::UnknownTextEncoding { encoding }),
    }
}

/// Assemble des paires d'octets en unités UTF-16 puis en chaîne.
fn decode_utf16(data: &[u8], little_endian: bool, encoding: u8) -> Result<String, Mp3Error> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| {
            let pair = [pair[0], pair[1]];
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect();

    String::from_utf16(&units).map_err(|_| Mp3Error::InvalidTextData { encoding })
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
            read_frame(&data, 0, Id3Version { major: 1, minor: 0 }),
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
        data.extend_from_slice(&[0, 0]);
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
        let frame = read_frame(&data, 0, V2_3).unwrap().unwrap();

        assert_eq!(&frame.id, b"TIT2");
        assert_eq!(frame.size, 6);
        assert_eq!(frame.content, FrameContent::Text(vec!["Hello".to_string()]));
        assert_eq!(frame.next_offset, 10 + 6);
    }

    #[test]
    fn test_read_frame_v2_3_at_nonzero_offset() {
        let mut data = vec![0xAA; 20];
        data.extend(build_frame_bytes_v2_3(b"TPE1", &text_body("Queen")));

        let frame = read_frame(&data, 20, V2_3).unwrap().unwrap();

        assert_eq!(&frame.id, b"TPE1");
        assert_eq!(frame.offset, 20);
        assert_eq!(frame.next_offset, 20 + 10 + 6);
    }

    #[test]
    fn test_read_frame_v2_3_zero_size_body() {
        let data = build_frame_bytes_v2_3(b"TCON", b"");
        let frame = read_frame(&data, 0, V2_3).unwrap().unwrap();

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
        let frame = read_frame(&data, 0, V2_3).unwrap().unwrap();

        assert_eq!(frame.size, 200);
        assert_eq!(frame.next_offset, 10 + 200);
    }

    #[test]
    fn test_read_frame_v2_4_decodes_synchsafe_size() {
        // Corps de 200 octets, encodé en synchsafe : sans le bon décodage,
        // la frame serait mal découpée.
        let body = vec![0u8; 200];
        let data = build_frame_bytes_v2_4(b"APIC", &body);
        let frame = read_frame(&data, 0, V2_4).unwrap().unwrap();

        assert_eq!(frame.size, 200);
        assert_eq!(frame.next_offset, 10 + 200);
    }

    #[test]
    fn test_read_frame_v2_4_small_size_matches_v2_3() {
        // En dessous de 128 octets, brut et synchsafe coïncident : les
        // deux lectures doivent s'accorder.
        let data_v3 = build_frame_bytes_v2_3(b"TIT2", &text_body("Hi"));
        let data_v4 = build_frame_bytes_v2_4(b"TIT2", &text_body("Hi"));

        let frame_v3 = read_frame(&data_v3, 0, V2_3).unwrap().unwrap();
        let frame_v4 = read_frame(&data_v4, 0, V2_4).unwrap().unwrap();

        assert_eq!(frame_v3.size, frame_v4.size);
        assert_eq!(frame_v3.content, frame_v4.content);
    }

    #[test]
    fn test_read_frame_propagates_decode_error() {
        let data = build_frame_bytes_v2_3(b"TIT2", &[9, b'H', b'i']);
        assert!(matches!(
            read_frame(&data, 0, V2_3),
            Err(Mp3Error::UnknownTextEncoding { encoding: 9 })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_too_short_for_header() {
        let data = [0u8; 5];
        assert!(matches!(
            read_frame(&data, 0, V2_3),
            Err(Mp3Error::FrameTooShort { offset: 0 })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_exactly_too_short() {
        let data = [0u8; 9];
        assert!(matches!(
            read_frame(&data, 0, V2_3),
            Err(Mp3Error::FrameTooShort { offset: 0 })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_padding_returns_none() {
        let data = [0u8; 10];
        assert!(read_frame(&data, 0, V2_3).unwrap().is_none());
    }

    #[test]
    fn test_read_frame_v2_3_declared_size_exceeds_available_data() {
        let mut data = build_frame_bytes_v2_3(b"APIC", &[0u8; 100]);
        data.truncate(15);

        assert!(matches!(
            read_frame(&data, 0, V2_3),
            Err(Mp3Error::FrameSizeOverflow { offset: 0, .. })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_offset_beyond_data() {
        let data = build_frame_bytes_v2_3(b"TIT2", &text_body("Hello"));
        let offset = data.len();
        assert!(matches!(
            read_frame(&data, offset, V2_3),
            Err(Mp3Error::FrameTooShort { .. })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_offset_overflow_does_not_panic() {
        let data = [0u8; 20];
        assert!(matches!(
            read_frame(&data, usize::MAX - 5, V2_3),
            Err(Mp3Error::FrameTooShort { .. })
        ));
    }

    #[test]
    fn test_read_frame_v2_3_size_overflow_does_not_panic() {
        let mut data = vec![0u8; 10];
        data[0..4].copy_from_slice(b"TIT2");
        data[4..8].copy_from_slice(&u32::MAX.to_be_bytes());

        assert!(matches!(
            read_frame(&data, usize::MAX - 20, V2_3),
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
        let frame = read_frame(&data, 0, V2_2).unwrap().unwrap();

        assert_eq!(&frame.id, b"TIT2");
        assert_eq!(frame.size, 6);
        assert_eq!(frame.flags, 0);
        assert_eq!(frame.content, FrameContent::Text(vec!["Hello".to_string()]));
        assert_eq!(frame.next_offset, 6 + 6);
    }

    #[test]
    fn test_read_frame_v2_2_unmapped_id_kept_as_padded_id() {
        let data = build_frame_bytes_v2_2(b"XYZ", &[1, 2, 3]);
        let frame = read_frame(&data, 0, V2_2).unwrap().unwrap();

        assert_eq!(&frame.id, &[b'X', b'Y', b'Z', 0]);
        assert_eq!(frame.content, FrameContent::Unknown(vec![1, 2, 3]));
    }

    #[test]
    fn test_read_frame_v2_2_padding_returns_none() {
        let data = [0u8; 6];
        assert!(read_frame(&data, 0, V2_2).unwrap().is_none());
    }

    #[test]
    fn test_read_frame_v2_2_too_short_for_header() {
        let data = [0u8; 5];
        assert!(matches!(
            read_frame(&data, 0, V2_2),
            Err(Mp3Error::FrameTooShort { offset: 0 })
        ));
    }

    #[test]
    fn test_read_frame_v2_2_declared_size_exceeds_available_data() {
        let mut data = build_frame_bytes_v2_2(b"PIC", &[0u8; 50]);
        data.truncate(10);

        assert!(matches!(
            read_frame(&data, 0, V2_2),
            Err(Mp3Error::FrameSizeOverflow { offset: 0, .. })
        ));
    }

    #[test]
    fn test_read_frame_v2_2_at_nonzero_offset() {
        let mut data = vec![0xAA; 12];
        data.extend(build_frame_bytes_v2_2(b"TP1", &text_body("Queen")));

        let frame = read_frame(&data, 12, V2_2).unwrap().unwrap();

        assert_eq!(&frame.id, b"TPE1");
        assert_eq!(frame.offset, 12);
        assert_eq!(frame.next_offset, 12 + 6 + 6);
    }
}
