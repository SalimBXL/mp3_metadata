# mp3_metadata

Une bibliothèque Rust pour lire les métadonnées ID3v2 d'un fichier MP3 : en-tête du tag, frames individuelles (titre, artiste, album, pochette, commentaires...) et leur contenu décodé.

## Fonctionnalités

- Vérification qu'un chemin pointe vers un fichier `.mp3` existant.
- Lecture d'un fichier MP3 et extraction de son en-tête ID3v2 (`read_mp3_file`).
- Analyse séquentielle des frames contenues dans le tag (`read_frames`).
- Décodage du contenu des frames texte (`TIT2`, `TPE1`, `TPE2`, `TALB`, `TRCK`, `TCON`), en respectant l'octet d'encoding ID3v2 (ISO-8859-1, UTF-16 avec BOM, UTF-16BE, UTF-8).
- Gestion d'erreurs typée via l'enum `Mp3Error`, plutôt que des chaînes de caractères génériques.
- Affichage lisible (`Display`) pour `Mp3File`, `Header` et `Frame`.

## Installation

Ajoute la dépendance dans ton `Cargo.toml` :

```toml
[dependencies]
mp3_metadata = { git = "https://github.com/SalimBXL/mp3_metadata.git" }
```

## Utilisation

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mp3_file = mp3_metadata::read_mp3_file("musique/chanson.mp3")?;

    println!("{mp3_file}");
    println!("{}", mp3_file.header);

    let frames = mp3_metadata::read_frames(&mp3_file.header);
    for frame in &frames {
        println!("{frame}");
    }

    Ok(())
}
```

Exemple de sortie :

```
- MP3 -------------------------
Filename : musique/chanson.mp3
Size     : 5872341 octets (5.60 Mo)
-------------------------------
- HEADER ----------------------
ID3v2 détecté
Version : 3.0
Flags   : 00
Taille  : 4096 octets (4.00 Ko)
-------------------------------
- FRAME -----------------------
Id          : TIT2
Flags       : 00
Taille      : 17 octets
Offset      : 10
Next offset : 37
Data        : Bohemian Rhapsody
-------------------------------
```

## Structure du projet

```
src/
├── lib.rs          # API publique : Mp3File, read_mp3_file, read_frames
├── error.rs        # Enum Mp3Error et ses implémentations Display / Error
├── id3.rs          # Déclaration des sous-modules header et frame
└── id3/
    ├── header.rs   # Header, Id3Version, read_header
    └── frame.rs    # Frame, DecodedFrame, read_frame, decode_frame, decode_text_frame
```

## API principale

| Élément | Description |
|---|---|
| `read_mp3_file(path) -> Result<Mp3File, Box<dyn Error>>` | Lit un fichier `.mp3` sur le disque et en extrait l'en-tête ID3v2. |
| `read_frames(&header) -> Vec<Frame>` | Parcourt et décode toutes les frames contenues dans un tag ID3v2. |
| `Mp3File` | Fichier MP3 chargé : chemin, taille, en-tête. |
| `Header` | En-tête ID3v2 : version, flags, taille du tag, contenu brut du tag. |
| `Frame` | Une frame individuelle : identifiant, taille, flags, contenu brut, position dans le tag. |
| `Mp3Error` | Enum d'erreurs : `NotFound`, `ReadFailed`, `TooSmall`, `MissingId3Tag`, `InvalidTagSize`. |

## Frames prises en charge

| Identifiant | Type de contenu |
|---|---|
| `TIT2`, `TPE1`, `TPE2`, `TALB`, `TRCK`, `TCON` | Texte (titre, artiste, album, piste, genre) |
| `APIC` | Image jointe (pochette d'album) |
| `COMM` | Commentaire |
| autre | Contenu brut conservé tel quel |

## Tests

```sh
cargo test
```

Chaque fonction de parsing (`read_header`, `read_frame`, `decode_frame`, `decode_text_frame`) est couverte par des tests unitaires, y compris les cas limites (fichiers tronqués, tailles invalides, encodings inconnus) et les protections contre les dépassements arithmétiques sur des offsets ou tailles corrompus.

## Limitations connues

- Le type MIME des images `APIC` n'est pas encore extrait (`mime_type` vaut toujours `"inconnu"`).
- Les frames `COMM` sont décodées en UTF-8 brut, sans tenir compte de leur octet d'encoding ni de leurs champs langue/description.
- Seul ID3v2 est pris en charge (pas ID3v1, situé en fin de fichier).
- `Mp3File` ne conserve pas les données audio du fichier, seulement l'en-tête ID3v2.

## Licence

À définir.