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
        let Some(frame) = read_frame(data, offset) else {
            break;
        };
        let Some(decoded_content) = decode_frame(&frame) else {
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
        print_frame(&frame);
        offset = frame.next_offset;
    }
}

fn read_frame(data: &[u8], offset: usize) -> Option<Frame> {
    println!(". Lecture de la frame à l'offset {}", offset);
    if data.len() < offset + 10 {
        return None;
    }

    let frame_id = &data[offset..offset + 4];
    let frame_size = &data[offset + 4..offset + 8];
    let frame_flags = &data[offset + 8..offset + 10];
    let size = u32::from_be_bytes([frame_size[0], frame_size[1], frame_size[2], frame_size[3]]);
    let flags = u16::from_be_bytes([frame_flags[0], frame_flags[1]]);
    let next_offset = offset + 10 + size as usize;

    if next_offset > data.len() {
        return None;
    }

    let frame_data = data[offset + 10..offset + 10 + size as usize].to_vec();
    let next_offset = offset + 10 + size as usize;

    Some(Frame {
        id: frame_id.try_into().unwrap_or([0; 4]),
        size,
        flags,
        data: frame_data,
        offset,
        next_offset,
    })
}

fn decode_frame(frame: &Frame) -> Option<String> {
    println!(
        ". . Décodage de la frame ID : {}",
        String::from_utf8_lossy(&frame.id)
    );
    if frame.data.is_empty() {
        return None;
    }

    let encoding = frame.data[0];
    let text_data = &frame.data[1..];
    match encoding {
        // ISO-8859-1
        0 => Some(
            text_data
                .iter()
                .map(|&byte| char::from_u32(byte as u32).unwrap())
                .collect(),
        ),

        // UTF-16 avec BOM
        1 => {
            if text_data.len() < 2 {
                return None;
            }
            let bom = &text_data[..2];
            let text_data = &text_data[2..];
            if !text_data.len().is_multiple_of(2) {
                return None;
            }
            let units: Vec<u16> = match bom {
                [0xFF, 0xFE] => text_data
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                    .collect(),
                [0xFE, 0xFF] => text_data
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                    .collect(),
                _ => return None,
            };
            String::from_utf16(&units).ok()
        }

        // UTF-16BE
        2 => {
            let units: Vec<u16> = text_data
                .chunks_exact(2)
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16(&units).ok()
        }

        // UTF-8
        3 => String::from_utf8(text_data.to_vec()).ok(),
        _ => None,
    }
}

fn print_frame(frame: &Frame) {
    match &frame.id {
        b"TIT2" | b"TPE1" | b"TPE2" | b"TALB" | b"TRCK" | b"TCON" => {
            // texte
            println!(
                ". . . Contenu : {} ({} octets)",
                String::from_utf8_lossy(&frame.id),
                frame.data.len()
            );
        }

        b"APIC" => {
            // image
            println!(". . . Contenu : Image ({} octets)", frame.data.len());
        }

        b"COMM" => {
            // commentaire
            println!(". . . Contenu : Commentaire ({} octets)", frame.data.len());
        }

        _ => {
            // inconnu
            println!(". . . Contenu : Inconnu ({} octets)", frame.data.len());
        }
    }
}
