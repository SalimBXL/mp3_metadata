use mp3_metadata::{FrameContent, Id3v2Tag, MpegAudio, read_mp3_file, read_mp3_file_with_audio};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut path = None;
    let mut verbose = false;
    let mut verify = false;
    let mut load_audio = false;
    #[cfg(feature = "verify")]
    let mut limit = mp3_metadata::verify::DEFAULT_SEARCH_LIMIT;
    #[cfg(not(feature = "verify"))]
    let mut limit: u32 = 20;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            "--verify" => verify = true,
            "--load-audio" => load_audio = true,
            "--limit" => {
                let Some(value) = args.next() else {
                    eprintln!("--limit attend une valeur (ex. --limit 20)");
                    return ExitCode::FAILURE;
                };
                match value.parse() {
                    Ok(n) => limit = n,
                    Err(_) => {
                        eprintln!("--limit attend un nombre entier positif, reçu : {value}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            _ => path = Some(arg),
        }
    }

    let Some(path) = path else {
        eprintln!(
            "Usage : mp3_metadata [--verbose] [--verify] [--load-audio] [--limit N] <fichier.mp3>"
        );
        return ExitCode::FAILURE;
    };

    if let Err(err) = run(&path, verbose, verify, load_audio, limit) {
        eprintln!("Erreur : {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Orchestre la commande : lit le fichier selon les options demandées,
/// puis délègue chaque bloc d'affichage à sa propre fonction.
fn run(
    path: &str,
    verbose: bool,
    verify: bool,
    load_audio: bool,
    limit: u32,
) -> Result<(), mp3_metadata::Mp3Error> {
    // Par défaut, seul le tag ID3v2 est lu (quelques dizaines de Ko au
    // plus) : les données audio, potentiellement énormes, ne sont chargées
    // que si --load-audio est passé.
    let mp3 = if load_audio {
        read_mp3_file_with_audio(path)?
    } else {
        read_mp3_file(path)?
    };
    println!("{mp3}");

    let Some(tag) = &mp3.id3v2 else {
        println!("Pas de tag ID3v2");
        return Ok(());
    };

    println!("{tag}");
    print_tag_summary(tag);

    if let Some(audio) = &mp3.audio {
        print_audio_info(audio);
    }

    if verbose {
        print_frames(tag);
    }

    if verify {
        print_verification(tag, limit);
    }

    Ok(())
}

/// Affiche le résumé des métadonnées usuelles d'un tag : titre, artiste,
/// album, année, et les images qu'il porte.
fn print_tag_summary(tag: &Id3v2Tag) {
    println!("Titre   : {}", tag.title().unwrap_or("?"));
    println!("Artiste : {}", tag.artist().unwrap_or("?"));
    println!("Album   : {}", tag.album().unwrap_or("?"));
    println!("Année   : {}", tag.year().unwrap_or("?"));

    for frame in tag.pictures() {
        if let FrameContent::Picture {
            mime_type, data, ..
        } = &frame.content
        {
            let taille_ko = data.len() as f64 / 1024.0;
            println!(
                "Pochette : {mime_type}, {} octets ({taille_ko:.2} Ko)",
                data.len()
            );
        }
    }
}

/// Affiche la taille des données audio chargées (mode --load-audio).
fn print_audio_info(audio: &MpegAudio) {
    let taille_mo = audio.data.len() as f64 / (1024.0 * 1024.0);
    println!(
        "Audio    : {} octets chargés ({taille_mo:.2} Mo)",
        audio.data.len()
    );
}

/// Affiche le détail de chaque frame du tag (mode --verbose).
fn print_frames(tag: &Id3v2Tag) {
    for frame in &tag.frames {
        println!("{frame}");
    }
}

/// Interroge MusicBrainz et affiche le résultat (mode --verify), jusqu'à
/// `limit` candidats (voir --limit).
///
/// Un échec de vérification (pas de réseau, aucun résultat...) n'invalide
/// pas le reste : le tag local a bien été lu, seul le contrôle en ligne
/// n'a pas abouti.
fn print_verification(tag: &Id3v2Tag, limit: u32) {
    #[cfg(feature = "verify")]
    match mp3_metadata::verify::verify_tag(tag, limit) {
        Ok(reports) => print!("{}", mp3_metadata::verify::VerificationTable(&reports)),
        Err(err) => eprintln!("Vérification MusicBrainz impossible : {err}"),
    }
    #[cfg(not(feature = "verify"))]
    {
        let _ = (tag, limit); // non utilisés sans la feature "verify"
        eprintln!("Compilé sans la fonctionnalité 'verify' (voir Cargo.toml)");
    }
}
