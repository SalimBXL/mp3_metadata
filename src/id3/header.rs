use crate::error::Mp3Error;
use crate::id3::frame::{Frame, FrameContent, read_frame};
use crate::id3::{deunsynchronize, synchsafe_to_u32};
use std::borrow::Cow;

/// Bit 6 des flags de l'en-tête principal : un extended header est présent
/// en tête du corps du tag.
const EXTENDED_HEADER_FLAG: u8 = 0x40;

/// Bit 7 des flags de l'en-tête principal : le corps entier du tag
/// (extended header et frames compris) a été unsynchronisé à l'écriture.
const UNSYNCHRONISATION_FLAG: u8 = 0x80;

/// Version d'un tag ID3v2, telle qu'elle apparaît dans l'en-tête du fichier.
///
/// `major` et `minor` correspondent aux deux octets de version situés juste
/// après la signature `ID3` (par exemple, `major = 3, minor = 0` pour
/// ID3v2.3.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Id3Version {
    /// Version majeure (2, 3 ou 4 pour ID3v2.2, .3, .4 respectivement).
    pub major: u8,
    /// Version mineure (quasiment toujours `0` en pratique).
    pub minor: u8,
}

impl std::fmt::Display for Id3Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "2.{}.{}", self.major, self.minor)
    }
}

/// Tag ID3v2 complet extrait du début d'un fichier MP3 : l'en-tête **et**
/// toutes ses frames, déjà décodées.
///
/// Une valeur `Id3v2Tag` est construite par [`read_tag`]. Elle ne conserve
/// pas les octets bruts du tag : chaque frame porte son contenu décodé, et
/// les octets des frames non reconnues restent accessibles via
/// [`FrameContent::Unknown`].
#[derive(Debug, Clone)]
pub struct Id3v2Tag {
    /// Version du tag (2.2, 2.3 ou 2.4).
    pub version: Id3Version,
    /// Octet de flags du tag ID3v2 (bits d'options telles que
    /// l'unsynchronisation, la présence d'un extended header, etc.).
    pub flags: u8,
    /// Taille du tag ID3v2 en octets, telle que déclarée dans l'en-tête
    /// (n'inclut pas les 10 octets de l'en-tête lui-même).
    pub size: u32,
    /// Frames du tag, dans l'ordre où elles apparaissent.
    ///
    /// [`Frame::offset`] et [`Frame::next_offset`] sont relatifs au corps
    /// du tag *après* retrait de l'unsynchronisation et de l'extended
    /// header éventuels — pas à la position brute dans le fichier. Un tag
    /// unsynchronisé est recopié dans un tampon dédié à la lecture ; les
    /// positions qui y sont mesurées ne correspondent donc plus aux octets
    /// du fichier d'origine.
    pub frames: Vec<Frame>,
}

impl Id3v2Tag {
    /// Renvoie la taille du tag ID3v2 en kibioctets (Ko, base 1024).
    pub fn size_ko(&self) -> f64 {
        self.size as f64 / 1024.0
    }

    /// Renvoie la première frame portant l'identifiant `id`.
    ///
    /// La plupart des identifiants ID3v2 ne peuvent apparaître qu'une fois
    /// dans un tag ; pour ceux qui peuvent se répéter (`APIC`, `COMM`,
    /// `TXXX`), utiliser [`Id3v2Tag::frames_with_id`].
    pub fn frame(&self, id: &[u8; 4]) -> Option<&Frame> {
        self.frames.iter().find(|frame| &frame.id == id)
    }

    /// Renvoie toutes les frames portant l'identifiant `id`.
    pub fn frames_with_id<'a>(&'a self, id: &'a [u8; 4]) -> impl Iterator<Item = &'a Frame> {
        self.frames.iter().filter(move |frame| &frame.id == id)
    }

    /// Renvoie la première valeur textuelle de la frame `id`, si elle
    /// existe et porte du texte.
    pub fn text(&self, id: &[u8; 4]) -> Option<&str> {
        self.frame(id)?.as_text()
    }

    /// Titre du morceau (`TIT2`).
    pub fn title(&self) -> Option<&str> {
        self.text(b"TIT2")
    }

    /// Artiste principal (`TPE1`).
    pub fn artist(&self) -> Option<&str> {
        self.text(b"TPE1")
    }

    /// Artiste de l'album (`TPE2`), souvent utilisé pour les compilations.
    pub fn album_artist(&self) -> Option<&str> {
        self.text(b"TPE2")
    }

    /// Titre de l'album (`TALB`).
    pub fn album(&self) -> Option<&str> {
        self.text(b"TALB")
    }

    /// Année d'enregistrement. Essaie `TYER` (v2.2/v2.3) puis `TDRC`
    /// (v2.4, qui l'a remplacé).
    pub fn year(&self) -> Option<&str> {
        self.text(b"TYER").or_else(|| self.text(b"TDRC"))
    }

    /// Numéro de piste (`TRCK`), tel qu'il est écrit dans le tag — souvent
    /// sous la forme `3/12`, d'où le type `&str` plutôt qu'un entier.
    pub fn track(&self) -> Option<&str> {
        self.text(b"TRCK")
    }

    /// Genre musical (`TCON`).
    pub fn genre(&self) -> Option<&str> {
        self.text(b"TCON")
    }

    /// Texte du premier commentaire (`COMM`), sans sa langue ni sa
    /// description.
    pub fn comment(&self) -> Option<&str> {
        self.text(b"COMM")
    }

    /// Paroles non synchronisées (`USLT`).
    pub fn lyrics(&self) -> Option<&str> {
        self.text(b"USLT")
    }

    /// Renvoie les frames contenant une image (`APIC`).
    ///
    /// L'itérateur renvoie les frames elles-mêmes ; leur contenu se
    /// déstructure via [`FrameContent::Picture`].
    pub fn pictures(&self) -> impl Iterator<Item = &Frame> {
        self.frames
            .iter()
            .filter(|frame| matches!(frame.content, FrameContent::Picture { .. }))
    }
}

/// Affiche un résumé lisible du tag ID3v2 : version et nombre de frames
/// dans le titre de section, puis les champs dans le même ordre que
/// [`crate::Id3v1Tag`] (voir sa propre `Display`) pour que les deux
/// s'alignent ligne à ligne une fois affichés côte à côte — les champs
/// propres à ID3v2 (`Album Artist`, `Cover`) viennent après, sans
/// équivalent en face.
impl std::fmt::Display for Id3v2Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ID3v2 ({}, {} frames)", self.version, self.frames.len())?;
        writeln!(f, "{}", crate::SECTION_SEPARATOR)?;
        writeln!(f, "{:<11}: {}", "Title", self.title().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Artist", self.artist().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Album", self.album().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Year", self.year().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Comment", self.comment().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Track", self.track().unwrap_or("?"))?;
        writeln!(f, "{:<11}: {}", "Genre", self.genre().unwrap_or("?"))?;
        write!(
            f,
            "{:<11}: {}",
            "Album Artist",
            self.album_artist().unwrap_or("?")
        )?;

        for frame in self.pictures() {
            if let FrameContent::Picture { mime_type, .. } = &frame.content {
                write!(f, "\n{:<11}: {mime_type}", "Cover")?;
            }
        }

        Ok(())
    }
}

/// Longueur du corps du tag ID3v2 (hors en-tête principal de 10 octets),
/// lue dans les 10 premiers octets d'un fichier, sans rien lire de plus.
///
/// Sert à savoir combien d'octets lire avant même d'appeler [`read_tag`] —
/// utile pour ne charger que le tag en mémoire plutôt que le fichier
/// entier, voir [`crate::read_mp3_file`].
///
/// Renvoie `None` si ces 10 octets ne commencent pas par la signature
/// `ID3` : pas de tag, rien à lire de plus.
pub(crate) fn declared_tag_body_size(header: &[u8; 10]) -> Option<u32> {
    if &header[0..3] != b"ID3" {
        return None;
    }

    let size_bytes: [u8; 4] = header[6..10]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");
    Some(synchsafe_to_u32(size_bytes))
}

/// Analyse le tag ID3v2 situé au début d'un fichier MP3, en-tête et frames
/// comprises.
///
/// Lit les 10 premiers octets de `data` pour vérifier la signature `ID3`,
/// extraire la version, les flags et la taille du tag (toujours encodée en
/// synchsafe integer, quelle que soit la version). Si les flags indiquent
/// une unsynchronisation ou un extended header, le corps du tag est
/// d'abord préparé en conséquence avant que les frames n'en soient lues et
/// décodées séquentiellement — voir [`deunsynchronize`] et
/// [`extended_header_len`].
///
/// # Retour
///
/// - `Ok(None)` si `data` ne commence pas par la signature `ID3` : le
///   fichier n'a simplement pas de tag ID3v2, ce n'est pas une erreur.
/// - `Ok(Some(tag))` si le tag a pu être lu intégralement.
///
/// # Erreurs
///
/// - [`Mp3Error::TooSmall`] si `data` contient moins de 10 octets.
/// - [`Mp3Error::InvalidTagSize`] si la taille de tag déclarée dépasse la
///   taille réelle de `data`.
/// - [`Mp3Error::ExtendedHeaderTooShort`] si l'extended header déclaré ne
///   tient pas dans le corps du tag.
/// - [`Mp3Error::UnsupportedVersion`] si la version majeure du tag est
///   inférieure à 2.
/// - Toute erreur renvoyée par [`read_frame`] lors de la lecture ou du
///   décodage d'une frame.
///
/// # Limites connues
///
/// Le contenu de l'extended header (padding, CRC32) n'est pas exposé : il
/// est seulement ignoré pour retrouver le début des frames. L'unsynchronisation
/// propre à une frame individuelle (ID3v2.4, voir
/// [`crate::id3::frame::read_frame`]) est en revanche gérée, séparément de
/// celle de l'en-tête principal.
///
/// # Exemples
///
/// ```ignore
/// let data = std::fs::read("musique/chanson.mp3")?;
/// if let Some(tag) = read_tag(&data)? {
///     println!("{} — {}", tag.artist().unwrap_or("?"), tag.title().unwrap_or("?"));
/// }
/// ```
pub fn read_tag(data: &[u8]) -> Result<Option<Id3v2Tag>, Mp3Error> {
    if data.len() < 10 {
        return Err(Mp3Error::TooSmall { len: data.len() });
    }

    if &data[0..3] != b"ID3" {
        // Pas de tag ID3v2 : ce n'est pas une erreur.
        return Ok(None);
    }

    let version = Id3Version {
        major: data[3],
        minor: data[4],
    };
    let flags = data[5];

    // La taille du tag est toujours un synchsafe integer, y compris en
    // ID3v2.2 : seule la taille des *frames* varie selon la version.
    let size_bytes: [u8; 4] = data[6..10]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");
    let size = synchsafe_to_u32(size_bytes);

    let tag_end = 10usize
        .checked_add(size as usize)
        .filter(|&end| end <= data.len())
        .ok_or(Mp3Error::InvalidTagSize {
            declared: size,
            available: data.len(),
        })?;

    let raw_body = &data[10..tag_end];

    // L'unsynchronisation s'applique à tout le corps du tag : extended
    // header et frames compris. On la retire une bonne fois pour toutes
    // avant d'y chercher quoi que ce soit d'autre.
    let tag_already_unsynced = flags & UNSYNCHRONISATION_FLAG != 0;
    let body: Cow<[u8]> = if tag_already_unsynced {
        Cow::Owned(deunsynchronize(raw_body))
    } else {
        Cow::Borrowed(raw_body)
    };

    let frames_start = if flags & EXTENDED_HEADER_FLAG != 0 {
        extended_header_len(&body, version)?
    } else {
        0
    };

    let frames = read_frames(&body, frames_start, version, tag_already_unsynced)?;

    Ok(Some(Id3v2Tag {
        version,
        flags,
        size,
        frames,
    }))
}

/// Renvoie la longueur totale (en octets) de l'extended header présent en
/// tête de `body`, afin de savoir où commencent les frames.
///
/// Le format du champ de taille diffère entre versions :
///
/// - **ID3v2.3** : un entier big-endian brut sur 4 octets, qui ne compte
///   pas ces 4 octets eux-mêmes — la longueur totale est donc
///   `4 + valeur_lue`.
/// - **ID3v2.4** : un entier *synchsafe* sur 4 octets, qui compte
///   l'extended header dans son intégralité, lui-même compris — la
///   longueur totale est directement `valeur_lue`.
///
/// Le contenu de l'extended header (padding, CRC) n'est pas exposé par
/// [`Id3v2Tag`] : cette fonction sert uniquement à le franchir.
fn extended_header_len(body: &[u8], version: Id3Version) -> Result<usize, Mp3Error> {
    if body.len() < 4 {
        return Err(Mp3Error::ExtendedHeaderTooShort {
            available: body.len(),
        });
    }

    let size_bytes: [u8; 4] = body[0..4]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");

    let total = if version.major >= 4 {
        synchsafe_to_u32(size_bytes) as usize
    } else {
        4usize.saturating_add(u32::from_be_bytes(size_bytes) as usize)
    };

    if total > body.len() {
        return Err(Mp3Error::ExtendedHeaderTooShort {
            available: body.len(),
        });
    }

    Ok(total)
}

/// Lit séquentiellement toutes les frames du corps d'un tag ID3v2.
///
/// `body` est le corps du tag (après le retrait de l'en-tête principal, de
/// l'unsynchronisation et de l'extended header éventuels) ; `start` est le
/// décalage, dans `body`, auquel commence la première frame.
/// `tag_already_unsynced` indique si `body` a déjà été désunsynchronisé
/// dans son ensemble (voir [`deunsynchronize`]) : transmis tel quel à
/// [`read_frame`], qui l'utilise pour savoir si le bit d'unsynchronisation
/// propre à une frame individuelle (ID3v2.4) doit encore être consulté.
///
/// La taille de l'en-tête de frame — 6 octets en ID3v2.2, 10 octets
/// au-delà — détermine la borne d'arrêt de la boucle : en dessous de cette
/// taille il ne peut plus y avoir de frame complète, et la lecture s'arrête
/// sans erreur. Elle s'arrête aussi, normalement, dès que le padding de
/// fin de tag est atteint (voir [`read_frame`]).
fn read_frames(
    body: &[u8],
    start: usize,
    version: Id3Version,
    tag_already_unsynced: bool,
) -> Result<Vec<Frame>, Mp3Error> {
    let frame_header_len = if version.major == 2 { 6 } else { 10 };
    let mut offset = start;
    let mut frames = Vec::new();

    while offset + frame_header_len <= body.len() {
        match read_frame(body, offset, version, tag_already_unsynced)? {
            Some(frame) => {
                offset = frame.next_offset;
                frames.push(frame);
            }
            None => break,
        }
    }

    Ok(frames)
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests;
