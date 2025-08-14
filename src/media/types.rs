#[derive(Debug)]
#[allow(dead_code)]
pub struct MediaMetadata {
    pub title: String,
    pub id: String,
    pub thumbnail: Option<String>,
    pub duration: Option<u64>,
    pub author: Option<String>,
    pub likes: Option<u64>,
    pub format_ext: String,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct MediaFile {
    pub filename: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct MediaInfo {
    pub url: String,
    pub files: Vec<MediaFile>,
    pub metadata: MediaMetadata,
}
