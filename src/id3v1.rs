//! Décodage du tag ID3v1 (et de son extension ID3v1.1), situé dans les 128
//! derniers octets d'un fichier MP3.
//!
//! Format historique, bien plus simple qu'ID3v2 : champs de taille fixe,
//! sans en-tête de frame ni octet d'encoding — le texte y est toujours en
//! ISO-8859-1 (Latin-1), tronqué ou complété avec des octets nuls (parfois
//! des espaces, chez certains encodeurs plus anciens) jusqu'à la taille du
//! champ.
//!
//! # Portée volontairement limitée
//!
//! - Les genres au-delà de l'index 191 (ni les 80 d'origine, ni
//!   l'extension Winamp — voir [`Id3v1Tag::genre_name`]) n'ont pas de nom
//!   dans cette bibliothèque : [`Id3v1Tag::genre_name`] renvoie `None`, et
//!   l'affichage montre alors l'index brut suivi d'un `*` plutôt qu'un nom
//!   (voir [`format_genre`]).
//! - L'« ID3v1 Extended » (signature `TAG+` sur 227 octets, placée juste
//!   avant le tag ID3v1 classique par certains anciens encodeurs pour
//!   allonger titre/artiste/album) n'est pas reconnu : seul le tag ID3v1
//!   standard de 128 octets est lu.

use std::fmt;

/// Longueur totale, en octets, d'un tag ID3v1 : toujours les 128 derniers
/// octets du fichier lorsqu'il en porte un.
pub(crate) const ID3V1_LEN: usize = 128;

/// Tag ID3v1 (ou ID3v1.1) extrait de la fin d'un fichier MP3.
///
/// Une valeur `Id3v1Tag` est construite par [`read_id3v1_tag`]. Contrairement
/// à [`crate::Id3v2Tag`], ID3v1 n'a ni frames ni octet d'encoding : chaque
/// champ est une chaîne de taille fixe, décodée en ISO-8859-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id3v1Tag {
    /// Titre du morceau.
    pub title: String,
    /// Artiste principal.
    pub artist: String,
    /// Titre de l'album.
    pub album: String,
    /// Année, telle qu'écrite sur les 4 caractères qui lui sont réservés
    /// (peut être vide ou non numérique si le champ n'a jamais été rempli).
    pub year: String,
    /// Commentaire libre. Pour un tag ID3v1.1, les deux derniers octets de
    /// ce champ peuvent être réutilisés pour porter [`Id3v1Tag::track`] à
    /// la place de texte (voir [`read_id3v1_tag`]).
    pub comment: String,
    /// Numéro de piste, présent seulement si le tag suit l'extension
    /// ID3v1.1 (voir [`read_id3v1_tag`] pour la condition de détection).
    /// `None` pour un tag ID3v1 classique, où ces deux octets appartiennent
    /// au commentaire.
    pub track: Option<u8>,
    /// Index de genre brut (0-255), tel qu'écrit dans le dernier octet du
    /// tag. Voir [`Id3v1Tag::genre_name`] pour le nom correspondant.
    pub genre: u8,
}

impl Id3v1Tag {
    /// Nom du genre, d'après soit la table des 80 genres d'origine de la
    /// spécification ID3v1 (index 0 à 79), soit l'extension Winamp (index
    /// 80 à 191, non officielle mais largement répandue — voir la portée
    /// limitée en tête de module et [`format_genre`] pour comment
    /// `Display` distingue les deux). `None` seulement au-delà de l'index
    /// 191, où aucune des deux tables n'a de nom à proposer.
    pub fn genre_name(&self) -> Option<&'static str> {
        let index = self.genre as usize;
        GENRES
            .get(index)
            .or_else(|| {
                index
                    .checked_sub(GENRES.len())
                    .and_then(|i| WINAMP_EXTRA_GENRES.get(i))
            })
            .copied()
    }
}

/// Affiche un résumé lisible du tag ID3v1 : mêmes libellés et même ordre
/// de champs que [`crate::Id3v2Tag`] (qui reprend cet ordre pour ses
/// propres champs communs) pour rester facile à comparer une fois les
/// deux affichés côte à côte, `?` pour un champ vide — y compris `Track`
/// pour un tag ID3v1 classique, plutôt que d'omettre la ligne : ID3v2
/// affiche toujours ce champ (voir [`crate::Id3v2Tag`]), l'omettre ici
/// décalerait `Genre` d'une ligne par rapport à son vis-à-vis dès qu'un
/// tag n'a pas de numéro de piste. Le genre est un cas particulier — voir
/// [`format_genre`].
impl fmt::Display for Id3v1Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ID3v1")?;
        writeln!(f, "{}", crate::SECTION_SEPARATOR)?;
        writeln!(f, "{:<11}: {}", "Title", non_empty(&self.title))?;
        writeln!(f, "{:<11}: {}", "Artist", non_empty(&self.artist))?;
        writeln!(f, "{:<11}: {}", "Album", non_empty(&self.album))?;
        writeln!(f, "{:<11}: {}", "Year", non_empty(&self.year))?;
        writeln!(f, "{:<11}: {}", "Comment", non_empty(&self.comment))?;
        match self.track {
            Some(track) => writeln!(f, "{:<11}: {track}", "Track")?,
            None => writeln!(f, "{:<11}: ?", "Track")?,
        }
        write!(
            f,
            "{:<11}: {}",
            "Genre",
            format_genre(self.genre, self.genre_name())
        )
    }
}

fn non_empty(s: &str) -> &str {
    if s.is_empty() { "?" } else { s }
}

/// Formate le genre pour l'affichage : son nom s'il en a un — table des
/// 80 genres d'origine ou extension Winamp, voir [`Id3v1Tag::genre_name`]
/// — suivi d'un `*` si ce nom vient de l'extension Winamp plutôt que des
/// 80 d'origine ; sinon (aucun nom disponible, au-delà de l'index 191)
/// l'index brut suivi d'un `*`. Dans les deux cas, le `*` signale donc la
/// même chose : « hors de la spécification ID3v1 officielle », qu'un nom
/// ait pu être trouvé ou non — pas la peine de le distinguer davantage
/// pour l'affichage.
fn format_genre(genre: u8, name: Option<&str>) -> String {
    match (name, is_official_genre(genre)) {
        (Some(name), true) => name.to_string(),
        (Some(name), false) => format!("{name}*"),
        (None, _) => format!("{genre}*"),
    }
}

/// `true` si `genre` fait partie des 80 genres d'origine de la
/// spécification ID3v1 (index 0 à 79) — par opposition à l'extension
/// Winamp (80 à 191) ou à un index non reconnu du tout (au-delà de 191).
/// Voir [`format_genre`].
fn is_official_genre(genre: u8) -> bool {
    (genre as usize) < GENRES.len()
}

/// Analyse un tag ID3v1 à partir des 128 derniers octets d'un fichier MP3.
///
/// # Retour
///
/// `None` si `tail` ne commence pas par la signature `TAG` : le fichier ne
/// porte simplement pas de tag ID3v1, ce n'est pas une erreur — au même
/// titre que l'absence de tag ID3v2 dans [`crate::id3::header::read_tag`].
pub(crate) fn read_id3v1_tag(tail: &[u8; ID3V1_LEN]) -> Option<Id3v1Tag> {
    if &tail[0..3] != b"TAG" {
        return None;
    }

    let title = decode_field(&tail[3..33]);
    let artist = decode_field(&tail[33..63]);
    let album = decode_field(&tail[63..93]);
    let year = decode_field(&tail[93..97]);

    let comment_field = &tail[97..127];
    // ID3v1.1 (De Facto Standard) : le 29ᵉ octet du champ commentaire
    // (index 28) sert de marqueur — nul — et le 30ᵉ (index 29) porte le
    // numéro de piste, à condition que l'octet 28 soit bien nul et le 29ᵉ
    // non nul (sinon ce sont deux octets de commentaire ordinaires, comme
    // en ID3v1 classique).
    let (comment, track) = if comment_field[28] == 0 && comment_field[29] != 0 {
        (decode_field(&comment_field[..28]), Some(comment_field[29]))
    } else {
        (decode_field(comment_field), None)
    };

    let genre = tail[127];

    Some(Id3v1Tag {
        title,
        artist,
        album,
        year,
        comment,
        track,
        genre,
    })
}

/// Décode un champ ID3v1 de taille fixe : ISO-8859-1 (Latin-1) octet à
/// octet, tronqué au premier octet nul, puis débarrassé des espaces de fin
/// — les deux modes de remplissage rencontrés en pratique.
fn decode_field(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end]
        .iter()
        .map(|&b| b as char)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Table des 80 genres d'origine de la spécification ID3v1 (index = octet
/// de genre). Les extensions ultérieures (au-delà de l'index 79) ne sont
/// pas incluses ici — voir la portée limitée en tête de module.
const GENRES: [&str; 80] = [
    "Blues",
    "Classic Rock",
    "Country",
    "Dance",
    "Disco",
    "Funk",
    "Grunge",
    "Hip-Hop",
    "Jazz",
    "Metal",
    "New Age",
    "Oldies",
    "Other",
    "Pop",
    "R&B",
    "Rap",
    "Reggae",
    "Rock",
    "Techno",
    "Industrial",
    "Alternative",
    "Ska",
    "Death Metal",
    "Pranks",
    "Soundtrack",
    "Euro-Techno",
    "Ambient",
    "Trip-Hop",
    "Vocal",
    "Jazz+Funk",
    "Fusion",
    "Trance",
    "Classical",
    "Instrumental",
    "Acid",
    "House",
    "Game",
    "Sound Clip",
    "Gospel",
    "Noise",
    "AlternRock",
    "Bass",
    "Soul",
    "Punk",
    "Space",
    "Meditative",
    "Instrumental Pop",
    "Instrumental Rock",
    "Ethnic",
    "Gothic",
    "Darkwave",
    "Techno-Industrial",
    "Electronic",
    "Pop-Folk",
    "Eurodance",
    "Dream",
    "Southern Rock",
    "Comedy",
    "Cult",
    "Gangsta",
    "Top 40",
    "Christian Rap",
    "Pop/Funk",
    "Jungle",
    "Native American",
    "Cabaret",
    "New Wave",
    "Psychedelic",
    "Rave",
    "Showtunes",
    "Trailer",
    "Lo-Fi",
    "Tribal",
    "Acid Punk",
    "Acid Jazz",
    "Polka",
    "Retro",
    "Musical",
    "Rock & Roll",
    "Hard Rock",
];

/// Extension Winamp de la table des genres ID3v1, indices 80 à 191 —
/// non officielle (Winamp n'engageant que lui-même), mais largement
/// reprise par d'autres lecteurs et bibliothèques au point d'être
/// devenue un standard de fait. Index 0 de ce tableau = genre `80`.
///
/// Source : la documentation de `mutagen`
/// (<https://mutagen-specs.readthedocs.io/en/latest/id3/id3v1-genres.html>),
/// recoupée avec la page Wikipédia « List of ID3v1 genres ». D'autres
/// listes en circulation diffèrent légèrement sur quelques entrées tardives
/// (ex. l'index 133 est `"Afro-Punk"` ici, `"Negerpunk"` ailleurs) : Winamp
/// a fait évoluer sa propre liste au fil de ses versions, sans qu'une
/// source unique ne fasse autorité au-delà de la version 1.91 (indices 80
/// à 147).
const WINAMP_EXTRA_GENRES: [&str; 112] = [
    "Folk",
    "Folk-Rock",
    "National Folk",
    "Swing",
    "Fast-Fusion",
    "Bebop",
    "Latin",
    "Revival",
    "Celtic",
    "Bluegrass",
    "Avantgarde",
    "Gothic Rock",
    "Progressive Rock",
    "Psychedelic Rock",
    "Symphonic Rock",
    "Slow Rock",
    "Big Band",
    "Chorus",
    "Easy Listening",
    "Acoustic",
    "Humour",
    "Speech",
    "Chanson",
    "Opera",
    "Chamber Music",
    "Sonata",
    "Symphony",
    "Booty Bass",
    "Primus",
    "Porn Groove",
    "Satire",
    "Slow Jam",
    "Club",
    "Tango",
    "Samba",
    "Folklore",
    "Ballad",
    "Power Ballad",
    "Rhythmic Soul",
    "Freestyle",
    "Duet",
    "Punk Rock",
    "Drum Solo",
    "A Cappella",
    "Euro-House",
    "Dance Hall",
    "Goa",
    "Drum & Bass",
    "Club-House",
    "Hardcore",
    "Terror",
    "Indie",
    "BritPop",
    "Afro-Punk",
    "Polsk Punk",
    "Beat",
    "Christian Gangsta Rap",
    "Heavy Metal",
    "Black Metal",
    "Crossover",
    "Contemporary Christian",
    "Christian Rock",
    "Merengue",
    "Salsa",
    "Thrash Metal",
    "Anime",
    "JPop",
    "Synthpop",
    "Abstract",
    "Art Rock",
    "Baroque",
    "Bhangra",
    "Big Beat",
    "Breakbeat",
    "Chillout",
    "Downtempo",
    "Dub",
    "EBM",
    "Eclectic",
    "Electro",
    "Electroclash",
    "Emo",
    "Experimental",
    "Garage",
    "Global",
    "IDM",
    "Illbient",
    "Industro-Goth",
    "Jam Band",
    "Krautrock",
    "Leftfield",
    "Lounge",
    "Math Rock",
    "New Romantic",
    "Nu-Breakz",
    "Post-Punk",
    "Post-Rock",
    "Psytrance",
    "Shoegaze",
    "Space Rock",
    "Trop Rock",
    "World Music",
    "Neoclassical",
    "Audiobook",
    "Audio Theatre",
    "Neue Deutsche Welle",
    "Podcast",
    "Indie Rock",
    "G-Funk",
    "Dubstep",
    "Garage Rock",
    "Psybient",
];

//
// ---------- TESTS ----------
//
#[cfg(test)]
mod tests {
    use super::*;

    /// Construit les 128 octets d'un tag ID3v1, champs alignés sur leurs
    /// offsets réels ; ce qui n'est pas fourni reste à zéro.
    fn build_tag(
        title: &[u8],
        artist: &[u8],
        album: &[u8],
        year: &[u8],
        comment: &[u8],
        genre: u8,
    ) -> [u8; ID3V1_LEN] {
        let mut data = [0u8; ID3V1_LEN];
        data[0..3].copy_from_slice(b"TAG");
        data[3..3 + title.len()].copy_from_slice(title);
        data[33..33 + artist.len()].copy_from_slice(artist);
        data[63..63 + album.len()].copy_from_slice(album);
        data[93..93 + year.len()].copy_from_slice(year);
        data[97..97 + comment.len()].copy_from_slice(comment);
        data[127] = genre;
        data
    }

    #[test]
    fn test_read_id3v1_tag_without_signature_returns_none() {
        let data = [0u8; ID3V1_LEN];
        assert!(read_id3v1_tag(&data).is_none());
    }

    #[test]
    fn test_read_id3v1_tag_classic() {
        let data = build_tag(
            b"A Kind of Magic",
            b"Queen",
            b"Greatest Hits",
            b"1991",
            b"Super chanson",
            17, // Rock
        );

        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.title, "A Kind of Magic");
        assert_eq!(tag.artist, "Queen");
        assert_eq!(tag.album, "Greatest Hits");
        assert_eq!(tag.year, "1991");
        assert_eq!(tag.comment, "Super chanson");
        assert_eq!(tag.track, None);
        assert_eq!(tag.genre, 17);
        assert_eq!(tag.genre_name(), Some("Rock"));
    }

    #[test]
    fn test_read_id3v1_tag_v1_1_track_number() {
        let mut data = build_tag(b"Titre", b"", b"", b"", b"Commentaire", 0);
        // Marqueur ID3v1.1 : octet 28 du commentaire nul, octet 29 = piste.
        data[97 + 28] = 0;
        data[97 + 29] = 7;

        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.comment, "Commentaire");
        assert_eq!(tag.track, Some(7));
    }

    #[test]
    fn test_read_id3v1_tag_without_track_number_keeps_full_comment() {
        // 30 octets de commentaire, aucun octet nul : pas de marqueur
        // ID3v1.1, les deux derniers octets font partie du texte.
        let comment = [b'X'; 30];
        let data = build_tag(b"", b"", b"", b"", &comment, 0);

        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.comment, "X".repeat(30));
        assert_eq!(tag.track, None);
    }

    #[test]
    fn test_read_id3v1_tag_pads_fields_with_spaces() {
        // Remplissage par espaces plutôt que par octets nuls, rencontré
        // chez certains encodeurs plus anciens.
        let mut title = [b' '; 30];
        title[..5].copy_from_slice(b"Space");
        let data = build_tag(&title, b"", b"", b"", b"", 0);

        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.title, "Space");
    }

    #[test]
    fn test_read_id3v1_tag_unknown_genre_index_has_no_name() {
        let data = build_tag(b"", b"", b"", b"", b"", 255); // au-delà de 191 : aucune table
        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.genre_name(), None);
    }

    #[test]
    fn test_read_id3v1_tag_last_standard_genre_index_has_a_name() {
        let data = build_tag(b"", b"", b"", b"", b"", 79); // Hard Rock
        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.genre_name(), Some("Hard Rock"));
    }

    #[test]
    fn test_read_id3v1_tag_winamp_extension_first_and_last_have_names() {
        let first = read_id3v1_tag(&build_tag(b"", b"", b"", b"", b"", 80)).unwrap();
        let last = read_id3v1_tag(&build_tag(b"", b"", b"", b"", b"", 191)).unwrap();

        assert_eq!(first.genre_name(), Some("Folk"));
        assert_eq!(last.genre_name(), Some("Psybient"));
    }

    #[test]
    fn test_read_id3v1_tag_just_past_winamp_extension_has_no_name() {
        let data = build_tag(b"", b"", b"", b"", b"", 192);
        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.genre_name(), None);
    }

    #[test]
    fn test_read_id3v1_tag_decodes_latin1_byte() {
        // 0xE9 = 'é' en Latin-1.
        let data = build_tag(&[b'H', b'i', 0xE9], b"", b"", b"", b"", 0);
        let tag = read_id3v1_tag(&data).unwrap();

        assert_eq!(tag.title, "Hié");
    }

    #[test]
    fn test_display_includes_all_fields_and_track() {
        let mut data = build_tag(b"Titre", b"Artiste", b"Album", b"1999", b"Com", 17);
        data[97 + 28] = 0;
        data[97 + 29] = 3;
        let tag = read_id3v1_tag(&data).unwrap();
        let text = tag.to_string();

        assert!(text.starts_with("ID3v1\n"));
        assert!(text.contains("Title      : Titre"));
        assert!(text.contains("Artist     : Artiste"));
        assert!(text.contains("Album      : Album"));
        assert!(text.contains("Year       : 1999"));
        assert!(text.contains("Comment    : Com"));
        assert!(text.contains("Track      : 3"));
        assert!(text.contains("Genre      : Rock"));
    }

    #[test]
    fn test_display_shows_placeholder_track_for_classic_tag() {
        let data = build_tag(b"", b"", b"", b"", b"", 0);
        let tag = read_id3v1_tag(&data).unwrap();

        // La ligne reste présente (alignement avec ID3v2), mais avec un
        // placeholder plutôt qu'un vrai numéro de piste.
        assert!(tag.to_string().contains("Track      : ?"));
    }

    // ----- format_genre / is_official_genre -----

    #[test]
    fn test_format_genre_official_shows_name_without_asterisk() {
        assert_eq!(format_genre(17, Some("Rock")), "Rock");
    }

    #[test]
    fn test_format_genre_winamp_extension_shows_name_with_asterisk() {
        assert_eq!(format_genre(145, Some("Anime")), "Anime*");
    }

    #[test]
    fn test_format_genre_unrecognized_shows_index_with_asterisk() {
        assert_eq!(format_genre(255, None), "255*");
    }

    #[test]
    fn test_is_official_genre() {
        assert!(is_official_genre(0));
        assert!(is_official_genre(79));
        assert!(!is_official_genre(80));
        assert!(!is_official_genre(191));
        assert!(!is_official_genre(255));
    }

    #[test]
    fn test_display_marks_unrecognized_genre_with_an_asterisk() {
        let data = build_tag(b"", b"", b"", b"", b"", 255); // au-delà même de l'extension Winamp
        let tag = read_id3v1_tag(&data).unwrap();

        assert!(tag.to_string().contains("Genre      : 255*"));
    }

    #[test]
    fn test_display_marks_winamp_extension_genre_with_an_asterisk() {
        let data = build_tag(b"", b"", b"", b"", b"", 145); // Anime, extension Winamp
        let tag = read_id3v1_tag(&data).unwrap();

        assert!(tag.to_string().contains("Genre      : Anime*"));
    }

    #[test]
    fn test_display_known_genre_has_no_asterisk() {
        let data = build_tag(b"", b"", b"", b"", b"", 79); // Hard Rock, dernier de la table
        let tag = read_id3v1_tag(&data).unwrap();
        let text = tag.to_string();

        assert!(text.contains("Genre      : Hard Rock"));
        assert!(!text.contains('*'));
    }
}
