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
mod tests {
    use super::*;

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
        assert!(text.contains("Album Artist: Queen")); // 12 caractères : dépasse la largeur de colonne (11), sans espace avant ":"
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
}
