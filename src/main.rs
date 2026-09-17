use mp3_metadata::{FrameContent, read_mp3_file};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut path = None;
    let mut verbose = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            _ => path = Some(arg),
        }
    }

    let Some(path) = path else {
        eprintln!("Usage : mp3_metadata [--verbose] <fichier.mp3>");
        return ExitCode::FAILURE;
    };

    if let Err(err) = run(&path, verbose) {
        eprintln!("Erreur : {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run(path: &str, verbose: bool) -> Result<(), mp3_metadata::Mp3Error> {
    let mp3 = read_mp3_file(path)?;
    println!("{mp3}");

    let Some(tag) = &mp3.id3v2 else {
        println!("Pas de tag ID3v2");
        return Ok(());
    };

    println!("{tag}");

    // Les accesseurs donnent directement les métadonnées usuelles.
    println!("Titre   : {}", tag.title().unwrap_or("?"));
    println!("Artiste : {}", tag.artist().unwrap_or("?"));
    println!("Album   : {}", tag.album().unwrap_or("?"));
    println!("Année   : {}", tag.year().unwrap_or("?"));

    for frame in tag.pictures() {
        if let FrameContent::Picture {
            mime_type, data, ..
        } = &frame.content
        {
            println!("Pochette : {mime_type}, {} octets", data.len());
        }
    }

    if verbose {
        for frame in &tag.frames {
            println!("{frame}");
        }
    }

    Ok(())
}
