use std::{
    collections::{HashMap, VecDeque},
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const MIME_CACHE_CAPACITY: usize = 4096;
const OCTET_STREAM: &str = "application/octet-stream";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl FileStamp {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;

        Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedMime {
    stamp: FileStamp,
    mime: String,
}

#[derive(Default)]
struct MimeCache {
    entries: HashMap<PathBuf, CachedMime>,
    recency: VecDeque<PathBuf>,
}

impl MimeCache {
    fn get(&mut self, path: &Path, stamp: FileStamp) -> Option<String> {
        let cached = self
            .entries
            .get(path)
            .filter(|cached| cached.stamp == stamp)?
            .mime
            .clone();
        self.touch(path);
        Some(cached)
    }

    fn insert(&mut self, path: &Path, stamp: FileStamp, mime: String) {
        let path = path.to_path_buf();
        self.entries
            .insert(path.clone(), CachedMime { stamp, mime });
        self.touch(&path);
        while self.entries.len() > MIME_CACHE_CAPACITY {
            if let Some(oldest) = self.recency.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    fn touch(&mut self, path: &Path) {
        if let Some(index) = self.recency.iter().position(|candidate| candidate == path) {
            self.recency.remove(index);
        }
        self.recency.push_back(path.to_path_buf());
    }
}

static MIME_CACHE: OnceLock<Mutex<MimeCache>> = OnceLock::new();

/// Detects a file's MIME type using its extension first and shared-mime-info
/// magic only when the extension is missing or unknown.
pub fn detect_mime(path: &Path) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    Ok(detect_mime_with_metadata(path, &metadata))
}

pub(crate) fn detect_mime_with_metadata(path: &Path, metadata: &fs::Metadata) -> String {
    let stamp = FileStamp::from_metadata(metadata);
    let cache = MIME_CACHE.get_or_init(|| Mutex::new(MimeCache::default()));
    if let Some(mime) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path, stamp)
    {
        return mime;
    }

    let guessed = mime_guess::from_path(path).first_raw().map(normalize_guess);
    let (mime, detected_stamp) = match guessed {
        Some(mime) if mime != OCTET_STREAM && !guess_needs_content_check(path, mime) => {
            (mime.to_owned(), stamp)
        }
        _ if metadata.len() == 0 => ("application/x-zerosize".to_owned(), stamp),
        _ => match fs::File::open(path) {
            Ok(file) => {
                let detected_stamp = file
                    .metadata()
                    .ok()
                    .map_or(stamp, |metadata| FileStamp::from_metadata(&metadata));
                let detected = tree_magic_mini::from_file(&file).unwrap_or(OCTET_STREAM);
                let mime = if detected.trim().is_empty() || detected == OCTET_STREAM {
                    sniff_known_header(&file).unwrap_or(OCTET_STREAM).to_owned()
                } else {
                    detected.to_owned()
                };
                (mime, detected_stamp)
            }
            Err(_) => (OCTET_STREAM.to_owned(), stamp),
        },
    };
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(path, detected_stamp, mime.clone());
    mime
}

#[cfg(unix)]
fn sniff_known_header(file: &fs::File) -> Option<&'static str> {
    use std::os::unix::fs::FileExt as _;

    let mut bytes = [0_u8; 16];
    let read = file.read_at(&mut bytes, 0).ok()?;
    let bytes = &bytes[..read];
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        Some("application/zip")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(not(unix))]
fn sniff_known_header(_file: &fs::File) -> Option<&'static str> {
    None
}

fn normalize_guess(mime: &str) -> &str {
    match mime {
        // mime_guess still uses legacy spellings that differ from current
        // shared-mime-info.
        "text/x-rust" => "text/rust",
        "text/x-toml" => "application/toml",
        "text/x-yaml" => "application/yaml",
        mime => mime,
    }
}

fn guess_needs_content_check(path: &Path, mime: &str) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    matches!(
        (extension, mime),
        (Some("ts"), "video/vnd.dlna.mpeg-tts" | "video/mp2t")
            | (Some("tsx"), "application/x-tiled-tsx")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extension_uses_fast_guess() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("settings.toml");
        fs::write(&path, "theme = \"dark\"").expect("write TOML fixture");

        assert_eq!(detect_mime(&path).unwrap(), "application/toml");
    }

    #[test]
    fn legacy_code_mimes_are_normalized_to_shared_mime_info_names() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let rust = temporary.path().join("main.rs");
        let yaml = temporary.path().join("settings.yaml");
        fs::write(&rust, "fn main() {}").expect("write Rust fixture");
        fs::write(&yaml, "theme: dark").expect("write YAML fixture");

        assert_eq!(detect_mime(&rust).unwrap(), "text/rust");
        assert_eq!(detect_mime(&yaml).unwrap(), "application/yaml");
    }

    #[test]
    fn ambiguous_typescript_extension_checks_content() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("main.ts");
        fs::write(&path, "export const answer: number = 42;").expect("write TypeScript fixture");

        assert_eq!(detect_mime(&path).unwrap(), "text/plain");
    }

    #[test]
    fn extensionless_file_uses_content_magic() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("image");
        fs::write(&path, b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff")
            .expect("write GIF fixture");

        assert_eq!(detect_mime(&path).unwrap(), "image/gif");
    }

    #[test]
    fn cache_stamp_changes_when_a_same_size_file_is_replaced() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("unknown");
        let replacement = temporary.path().join("replacement");
        let gif = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff";
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.resize(gif.len(), 0);
        fs::write(&path, gif).expect("write GIF fixture");
        assert_eq!(detect_mime(&path).unwrap(), "image/gif");

        fs::write(&replacement, png).expect("write PNG fixture");
        fs::rename(replacement, &path).expect("replace fixture at the same path");
        assert_eq!(detect_mime(&path).unwrap(), "image/png");
    }
}
