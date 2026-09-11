use std::fs;

struct Frame {
    id: [u8; 4],
    size: u32,
    flags: u16,
    data: Vec<u8>,
    offset: usize,
    next_offset: usize,
}
struct Header {
    major: u8,
    minor: u8,
    flags: u8,
    size: u32,
    data: Vec<u8>,
}

fn mp3_file_exists(mp3_file: &str) -> bool {
    match fs::exists(mp3_file) {
        Ok(true) => true,
        Ok(false) => false,
        Err(e) => {
            println!("Erreur lors de la vérification de l'existence du fichier : {e}");
            false
        }
    }
}

pub fn read_mp3_file(mp3_file: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !mp3_file_exists(mp3_file) {
        return Err(format!("Le fichier '{}' n'existe pas", mp3_file).into());
    }
    println!("Lecture du fichier MP3 '{}'", mp3_file);
    let data = match fs::read(mp3_file) {
        Ok(data) => data,
        Err(e) => {
            return Err(format!(
                "Erreur lors de la lecture du fichier '{}' : {}",
                mp3_file, e
            )
            .into());
        }
    };
    Ok(data)
}

pub fn read_id3_tags(data: &[u8]) {
    println!("Lecture des tags ID3...");

    let header = match read_header(data) {
        Ok(header) => header,
        Err(e) => {
            println!("Erreur lors de la lecture de l'en-tête ID3 : {}", e);
            return;
        }
    };

    println!("-------------------------------");
    println!("ID3v2 détecté");
    println!("Version : {}.{}", header.major, header.minor);
    println!("Flags   : {:02X}", header.flags);
    println!("Taille  : {} octets", header.size);
    println!("-------------------------------");

    read_frames(&header.data, 0, header.data.len());
}

fn read_header(data: &[u8]) -> Result<Header, Box<dyn std::error::Error>> {
    println!("Lecture de l'en-tête ID3v...");
    if data.len() < 10 {
        println!("Fichier trop petit");
        return Err("Fichier trop petit".into());
    }

    if &data[0..3] != b"ID3" {
        println!("Pas de tag ID3v2 au début du fichier");
        return Err("Pas de tag ID3v2 au début du fichier".into());
    }

    let major = data[3];
    let minor = data[4];
    let flags = data[5];

    let size: u32 = ((data[6] as u32) << 21)
        | ((data[7] as u32) << 14)
        | ((data[8] as u32) << 7)
        | data[9] as u32;

    let tag_end = 10 + size as usize;

    Ok(Header {
        major,
        minor,
        flags,
        size,
        data: data[0..tag_end].to_vec(),
    })
}

fn read_frames(data: &[u8], start: usize, end: usize) {
    println!("Lecture des frames ID3... {} à {}", start, end);
    let mut offset = start;
    while offset + 10 <= end {
        let Some(frame) = id3::frame::read_frame(data, offset) else {
            break;
        };
        let Some(decoded_content) = mp3_metadata::id3::frame::decode_frame(&frame) else {
            println!(
                ". . Décodage de la frame ID : {} (Erreur de décodage)",
                String::from_utf8_lossy(&frame.id)
            );
            offset = frame.next_offset;
            continue;
        };
        println!(
            ". . Décodage de la frame ID : {} (Contenu : {})",
            String::from_utf8_lossy(&frame.id),
            decoded_content
        );
        mp3_metadata::id3::frame::print_frame(&frame);
        offset = frame.next_offset;
    }
}
