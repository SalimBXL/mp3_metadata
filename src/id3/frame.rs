struct Frame {
    id: String,
    size: u32,
    flags: u16,
    data: Vec<u8>,
}

enum DecodedFrame {
    Text(String),
    Comment(Comment),
    Picture(Picture),
    Unknown(Vec<u8>),
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

fn decode_frame(frame: &Frame) -> Option<DecodedFrame> {
    println!(
        ". . Décodage de la frame ID : {}",
        String::from_utf8_lossy(&frame.id)
    );
    if frame.data.is_empty() {
        return None;
    }

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

fn print_frame(frame: &Frame) {
    let text_encoding = frame.data[0];
    let text_data = &frame.data[1..];
    match text_encoding {
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
