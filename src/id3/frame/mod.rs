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
mod tests;
