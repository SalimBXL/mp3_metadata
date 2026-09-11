struct Id3Header {
    version: Id3Version,
    flags: u8,
    size: u32,
}

enum Id3Frame {
    Title(String),
    Artist(String),
    Album(String),
    Track(String),
    Genre(String),
    Comment(String),
    Picture {
        mime_type: String,
        picture_type: u8,
        description: String,
        data: Vec<u8>,
    },
    Unknown {
        id: String,
        data: Vec<u8>,
    },
}
