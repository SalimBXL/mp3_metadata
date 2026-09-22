# mp3_metadata

Une bibliothèque Rust pour lire les métadonnées d'un fichier MP3 : tag ID3v2 (en-tête, frames individuelles — titre, artiste, album, pochette, commentaires... — et leur contenu décodé) ainsi que le tag ID3v1 / ID3v1.1 en fin de fichier.

## Fonctionnalités

- Vérification qu'un chemin pointe vers un fichier `.mp3` existant.
- Lecture d'un fichier MP3 et extraction de son tag ID3v2 complet, en-tête et frames déjà décodées (`read_mp3_file`).
- Lecture du tag ID3v1 / ID3v1.1 situé dans les 128 derniers octets du fichier, indépendamment du tag ID3v2.
- Décodage du contenu des frames texte (`TIT2`, `TPE1`, `TPE2`, `TALB`, `TRCK`, `TCON`), en respectant l'octet d'encoding ID3v2 (ISO-8859-1, UTF-16 avec BOM, UTF-16BE, UTF-8). Gère aussi bien l'unsynchronisation globale du tag (toutes versions) que celle propre à une frame individuelle (ID3v2.4 uniquement).
- Gestion d'erreurs typée via l'enum `Mp3Error`, plutôt que des chaînes de caractères génériques.
- Affichage lisible (`Display`) pour `Mp3File`, `Id3v2Tag`, `Frame` et `Id3v1Tag` — le CLI affiche ce dernier à droite du tag ID3v2 lorsque les deux sont présents.
- Vérification en ligne (feature `verify`, activée par défaut) des métadonnées locales auprès de MusicBrainz (`--verify`), en combinant une recherche par morceau et, si le tag local a un album, une recherche ciblée sur cet album — pensée pour les titres très repris en concert ou très réédités, où la première seule ne suffit pas (voir `src/verify/`). Une erreur réseau sur la recherche par album n'empêche jamais la vérification d'aboutir ; en mode `--verbose`, elle est quand même signalée sur `stderr` à titre diagnostique plutôt que de disparaître sans trace.
- Durée audio exacte quand la première frame porte un en-tête Xing/Info ou VBRI (calculée à partir du nombre de frames déclaré, plutôt qu'estimée à débit constant), avec débit moyen réel quand la taille du flux est connue — un `~` précède la durée dans l'affichage quand elle n'est qu'estimée (voir `src/mpeg/vbr.rs`).

## Installation

Ajoute la dépendance dans ton `Cargo.toml` :

```toml
[dependencies]
mp3_metadata = { git = "https://github.com/SalimBXL/mp3_metadata.git" }
```

## Utilisation

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mp3 = mp3_metadata::read_mp3_file("musique/chanson.mp3")?;

    println!("{mp3}");

    if let Some(tag) = &mp3.id3v2 {
        println!("{} — {}", tag.artist().unwrap_or("?"), tag.title().unwrap_or("?"));
        for frame in &tag.frames {
            println!("{frame}");
        }
    }

    Ok(())
}
```

Exemple de sortie (`cargo run -- musique/chanson.mp3`) :

```text
MP3
────────────────────────────────────
File       : musique/chanson.mp3
Size       : 8.70 MiB

Audio
────────────────────────────────────
MPEG       : MPEG-1 Layer III
Bitrate    : 320 kbps
Sample rate: 44.1 kHz
Channels   : Joint Stereo
Duration   : 3:44

ID3v2 (2.4.0, 28 frames)                ID3v1
────────────────────────────────────    ────────────────────────────────────
Title      : No Limit                   Title      : No Limit
Artist     : 2 Unlimited                Artist     : 2 Unlimited
Album      : The Very Best              Album      : The Very Best
Year       : 1994                       Year       : 1994
Comment    : dancevinilocd              Comment    : ?
Track      : 2/14                       Track      : 2
Genre      : Techno                     Genre      : Techno
Album Artist: 2 Unlimited
Cover      : image/jpeg
```

Version et nombre de frames figurent dans le titre de section ID3v2 ; les
champs communs aux deux formats (`Title` à `Genre`) sont dans le même
ordre des deux côtés pour s'aligner ligne à ligne une fois affichés côte à
côte ; les champs propres à ID3v2 (`Album Artist`, `Cover`) suivent sans
équivalent ID3v1 en face.

## Structure du projet

```text
src/
├── lib.rs                  # API publique : Mp3File, read_mp3_file, read_mp3_file_with_audio
├── tests.rs                # Tests de lib.rs
├── error.rs                # Enum Mp3Error et ses implémentations Display / Error
├── id3.rs                  # Utilitaires ID3v2 bas niveau (synchsafe, unsynchronisation) + sous-modules
├── id3/
│   ├── header.rs           # Id3v2Tag, Id3Version, read_tag
│   ├── header/tests/       # Tests de header.rs (scindés : parsing, accesseurs/Display)
│   └── frame/
│       ├── mod.rs          # Frame, read_frame (lecture bas niveau des frames)
│       ├── tests.rs        # Tests de frame/mod.rs
│       ├── decode.rs       # FrameContent, decode_frame (décodage du contenu)
│       └── decode/tests.rs # Tests de frame/decode.rs
├── id3v1.rs                # Id3v1Tag, read_id3v1_tag (tag ID3v1/ID3v1.1, fin de fichier)
├── id3v1/tests.rs          # Tests de id3v1.rs
├── mpeg/
│   ├── mod.rs               # En-tête de frame audio MPEG : MpegFrameHeader, find_frame_header
│   ├── vbr.rs                # AudioFormat, durée/débit exacts via Xing/Info/VBRI
│   └── vbr/tests.rs          # Tests de mpeg/vbr.rs
├── verify/
│   ├── mod.rs               # Feature "verify" : VerifyError, VerifyOutcome, verify_tag, recherche par morceau
│   ├── album.rs              # Recherche ciblée par album, complément de verify_tag
│   ├── table.rs              # VerificationReport, VerificationTable (affichage)
│   └── tests.rs              # Tests de verify/mod.rs
└── main.rs                  # CLI (--verbose, --verify, --load-audio, --limit)
```

Chaque module de plus de quelques centaines de lignes a ses tests unitaires dans un fichier `tests.rs` (ou `tests/`) sœur plutôt qu'un bloc `#[cfg(test)] mod tests { ... }` interne — même contenu, juste sorti pour garder les fichiers sous ~500 lignes.

## API principale

| Élément | Description |
| --- | --- |
| `read_mp3_file(path) -> Result<Mp3File, Mp3Error>` | Lit un fichier `.mp3` sur le disque et en extrait le tag ID3v2 complet et le tag ID3v1. |
| `read_mp3_file_with_audio(path) -> Result<Mp3File, Mp3Error>` | Comme `read_mp3_file`, en chargeant aussi les données audio. |
| `Mp3File` | Fichier MP3 chargé : chemin, taille, tag ID3v2 (`id3v2`), tag ID3v1 (`id3v1`), format audio. |
| `Id3v2Tag` | Tag ID3v2 : version, flags, taille, frames décodées, accesseurs (`title()`, `artist()`, `pictures()`...). |
| `Frame` | Une frame ID3v2 individuelle : identifiant, taille, flags, contenu décodé, position dans le tag. |
| `Id3v1Tag` | Tag ID3v1 / ID3v1.1 : titre, artiste, album, année, commentaire, piste (`Option<u8>`), genre. |
| `Mp3Error` | Enum d'erreurs : `NotFound`, `ReadFailed`, `TooSmall`, `InvalidTagSize`, `UnsupportedVersion`, ... |

## Frames prises en charge

| Identifiant | Type de contenu |
| --- | --- |
| `TIT2`, `TPE1`, `TPE2`, `TALB`, `TRCK`, `TCON` | Texte (titre, artiste, album, piste, genre) |
| `APIC` | Image jointe (pochette d'album) |
| `COMM` | Commentaire |
| autre | Contenu brut conservé tel quel |

## Tests

```sh
cargo test
```

Chaque fonction de parsing (`read_tag`, `read_frame`, `decode_frame`, `decode_string`, `read_id3v1_tag`) ainsi que `Mp3Error` (`Display`, `source()`) sont couverts par des tests unitaires, y compris les cas limites (fichiers tronqués, tailles invalides, encodings inconnus, remplissage par espaces ou par octets nuls) et les protections contre les dépassements arithmétiques sur des offsets ou tailles corrompus.

## Limitations connues

- En ID3v2.2, la frame `PIC` (équivalent d'`APIC`) code le format d'image sur 3 lettres (`JPG`, `PNG`, ...) plutôt qu'une chaîne MIME terminée par un nul : sur un tag v2.2, `mime_type` et `description` seront mal découpés. Ce cas n'est pas géré (voir `FrameContent::Picture`).
- `Id3v1Tag::genre_name()` couvre les 80 genres d'origine de la spécification ID3v1 (index 0 à 79) et l'extension Winamp (index 80 à 191) ; au-delà, `None`. L'affichage marque d'un `*` tout nom (ou index brut, si vraiment inconnu) hors des 80 d'origine. L'« ID3v1 Extended » (`TAG+`, 227 octets) n'est pas reconnu.
- `Mp3File` ne charge les données audio du fichier que sur demande explicite (`read_mp3_file_with_audio`).
- La durée exacte (Xing/Info/VBRI) ne retranche pas le délai et le padding que certains encodeurs (LAME) ajoutent pour un décodage "gapless" (quelques dizaines de millisecondes). Sans aucun des deux en-têtes, la durée retombe sur une estimation à débit constant, potentiellement fausse pour un fichier VBR (l'affichage le signale par un `~`).

## Changelog

Les modifications notables sont documentées dans [`CHANGELOG.md`](CHANGELOG.md).

## Licence

À définir.
