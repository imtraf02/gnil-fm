use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufReader, BufWriter},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use image::{ImageReader, imageops::FilterType};
use md5::{Digest as _, Md5};
use thiserror::Error;
use url::Url;

use crate::MAX_IMAGE_PIXELS;

const THUMBNAIL_SIZE: u32 = 256;
const HELPER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailRequest {
    pub path: PathBuf,
    pub size: u32,
}

impl ThumbnailRequest {
    #[must_use]
    pub fn grid(path: PathBuf) -> Self {
        Self {
            path,
            size: THUMBNAIL_SIZE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThumbnailSource {
    Cache,
    Image,
    SystemThumbnailer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThumbnailResult {
    pub path: PathBuf,
    pub source: ThumbnailSource,
}

#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("thumbnail generation is unsupported for {0}")]
    Unsupported(PathBuf),
    #[error("thumbnail generation cancelled")]
    Cancelled,
    #[error("thumbnail helper timed out")]
    TimedOut,
    #[error("invalid thumbnailer entry: {0}")]
    InvalidEntry(String),
    #[error("image error: {0}")]
    Image(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("PNG error: {0}")]
    Png(String),
}

#[derive(Default)]
pub struct ThumbnailService;

impl ThumbnailService {
    pub fn thumbnail(&self, request: &ThumbnailRequest) -> Result<ThumbnailResult, ThumbnailError> {
        self.thumbnail_cancellable(request, &AtomicBool::new(false))
    }

    pub fn thumbnail_cancellable(
        &self,
        request: &ThumbnailRequest,
        cancelled: &AtomicBool,
    ) -> Result<ThumbnailResult, ThumbnailError> {
        ensure_not_cancelled(cancelled)?;
        let metadata = fs::symlink_metadata(&request.path)?;
        if !metadata.is_file() {
            return Err(ThumbnailError::Unsupported(request.path.clone()));
        }
        let uri = file_uri(&request.path)?;
        let cache_path = cache_path(&uri)?;
        let modified = modified_seconds(&metadata)?;
        if valid_cached_thumbnail(&cache_path, &uri, modified, metadata.len()) {
            return Ok(ThumbnailResult {
                path: cache_path,
                source: ThumbnailSource::Cache,
            });
        }
        let mime = mime_guess::from_path(&request.path)
            .first_raw()
            .unwrap_or("application/octet-stream");
        let image_error = if mime.starts_with("image/") {
            match create_image_thumbnail(
                &request.path,
                &cache_path,
                &uri,
                modified,
                metadata.len(),
                request.size,
                cancelled,
            ) {
                Ok(()) => {
                    return Ok(ThumbnailResult {
                        path: cache_path,
                        source: ThumbnailSource::Image,
                    });
                }
                Err(ThumbnailError::Cancelled) => return Err(ThumbnailError::Cancelled),
                Err(error) => Some(error),
            }
        } else {
            None
        };

        let Some(entry) = discover_thumbnailer(mime) else {
            return Err(
                image_error.unwrap_or_else(|| ThumbnailError::Unsupported(request.path.clone()))
            );
        };
        run_thumbnailer(
            &entry,
            request,
            &cache_path,
            &uri,
            modified,
            metadata.len(),
            cancelled,
        )?;
        Ok(ThumbnailResult {
            path: cache_path,
            source: ThumbnailSource::SystemThumbnailer,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ThumbnailerEntry {
    exec: String,
    try_exec: Option<String>,
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), ThumbnailError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ThumbnailError::Cancelled)
    } else {
        Ok(())
    }
}

fn file_uri(path: &Path) -> Result<String, ThumbnailError> {
    let absolute = path.canonicalize()?;
    Url::from_file_path(&absolute)
        .map(|uri| uri.to_string())
        .map_err(|()| ThumbnailError::Unsupported(path.to_path_buf()))
}

fn cache_path(uri: &str) -> Result<PathBuf, ThumbnailError> {
    let root = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("thumbnails")
        .join("large");
    fs::create_dir_all(&root)?;
    let digest = Md5::digest(uri.as_bytes());
    Ok(root.join(format!("{digest:x}.png")))
}

fn modified_seconds(metadata: &fs::Metadata) -> Result<u64, ThumbnailError> {
    metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| ThumbnailError::Io(io::Error::other(error)))
}

fn valid_cached_thumbnail(path: &Path, uri: &str, modified: u64, size: u64) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let decoder = png::Decoder::new(BufReader::new(file));
    let Ok(reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    let value = |key: &str| {
        info.uncompressed_latin1_text
            .iter()
            .find(|chunk| chunk.keyword == key)
            .map(|chunk| chunk.text.as_str())
    };
    let modified = modified.to_string();
    let size = size.to_string();
    value("Thumb::URI") == Some(uri)
        && value("Thumb::MTime") == Some(modified.as_str())
        && value("Thumb::Size") == Some(size.as_str())
}

#[allow(clippy::too_many_arguments)]
fn create_image_thumbnail(
    source: &Path,
    destination: &Path,
    uri: &str,
    modified: u64,
    source_size: u64,
    requested_size: u32,
    cancelled: &AtomicBool,
) -> Result<(), ThumbnailError> {
    ensure_not_cancelled(cancelled)?;
    let reader = ImageReader::open(source)?
        .with_guessed_format()
        .map_err(|error| ThumbnailError::Image(error.to_string()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| ThumbnailError::Image(error.to_string()))?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(ThumbnailError::Image(
            "image exceeds the safe decode limit".into(),
        ));
    }
    ensure_not_cancelled(cancelled)?;
    let image = image::open(source).map_err(|error| ThumbnailError::Image(error.to_string()))?;
    let thumbnail = image
        .resize(requested_size, requested_size, FilterType::Triangle)
        .to_rgba8();
    write_thumbnail(
        destination,
        &thumbnail,
        uri,
        modified,
        source_size,
        cancelled,
    )
}

fn thumbnailer_search_dirs() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(data_home));
    } else if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local/share"));
    }
    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsStr::new("/usr/local/share:/usr/share").to_os_string());
    roots.extend(env::split_paths(&data_dirs));
    roots
        .into_iter()
        .map(|root| root.join("thumbnailers"))
        .collect()
}

fn discover_thumbnailer(mime: &str) -> Option<ThumbnailerEntry> {
    for directory in thumbnailer_search_dirs() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            if entry.path().extension() != Some(OsStr::new("thumbnailer")) {
                continue;
            }
            let Ok(source) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok((thumbnailer, mime_types)) = parse_thumbnailer_entry(&source) else {
                continue;
            };
            if mime_types.iter().any(|candidate| candidate == mime)
                && thumbnailer.try_exec.as_deref().is_none_or(command_exists)
            {
                return Some(thumbnailer);
            }
        }
    }
    None
}

fn parse_thumbnailer_entry(
    source: &str,
) -> Result<(ThumbnailerEntry, Vec<String>), ThumbnailError> {
    let mut in_group = false;
    let mut exec = None;
    let mut try_exec = None;
    let mut mime_types = Vec::new();
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line == "[Thumbnailer Entry]";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Exec" => exec = Some(value.trim().to_owned()),
            "TryExec" => try_exec = Some(value.trim().to_owned()),
            "MimeType" => {
                mime_types = value
                    .split(';')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
    let exec = exec.ok_or_else(|| ThumbnailError::InvalidEntry("missing Exec".into()))?;
    if mime_types.is_empty() {
        return Err(ThumbnailError::InvalidEntry("missing MimeType".into()));
    }
    Ok((ThumbnailerEntry { exec, try_exec }, mime_types))
}

fn command_exists(program: &str) -> bool {
    let program = Path::new(program);
    if program.components().count() > 1 {
        return program.is_file();
    }
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(program))
            .any(|candidate| candidate.is_file())
    })
}

#[allow(clippy::too_many_arguments)]
fn run_thumbnailer(
    entry: &ThumbnailerEntry,
    request: &ThumbnailRequest,
    destination: &Path,
    uri: &str,
    modified: u64,
    source_size: u64,
    cancelled: &AtomicBool,
) -> Result<(), ThumbnailError> {
    ensure_not_cancelled(cancelled)?;
    let output = temporary_path(destination, "helper");
    let arguments = shlex::split(&entry.exec)
        .ok_or_else(|| ThumbnailError::InvalidEntry("Exec has invalid quoting".into()))?;
    let Some((program, arguments)) = arguments.split_first() else {
        return Err(ThumbnailError::InvalidEntry("Exec is empty".into()));
    };
    let input = request.path.to_string_lossy();
    let size = request.size.to_string();
    let output_text = output.to_string_lossy();
    let expanded = arguments
        .iter()
        .map(|argument| expand_exec_argument(argument, &input, uri, &output_text, &size))
        .collect::<Result<Vec<_>, _>>()?;
    let mut child = Command::new(program).args(expanded).spawn()?;
    if let Err(error) = wait_for_helper(&mut child, cancelled) {
        let _ = fs::remove_file(&output);
        return Err(error);
    }
    ensure_not_cancelled(cancelled)?;
    let rendered =
        image::open(&output).map_err(|error| ThumbnailError::Image(error.to_string()))?;
    let rendered = rendered
        .resize(request.size, request.size, FilterType::Triangle)
        .to_rgba8();
    let result = write_thumbnail(
        destination,
        &rendered,
        uri,
        modified,
        source_size,
        cancelled,
    );
    let _ = fs::remove_file(output);
    result
}

fn expand_exec_argument(
    argument: &str,
    input: &str,
    uri: &str,
    output: &str,
    size: &str,
) -> Result<String, ThumbnailError> {
    let mut expanded = String::new();
    let mut chars = argument.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            expanded.push(character);
            continue;
        }
        let Some(code) = chars.next() else {
            return Err(ThumbnailError::InvalidEntry(
                "Exec ends with a percent sign".into(),
            ));
        };
        match code {
            'i' => expanded.push_str(input),
            'u' => expanded.push_str(uri),
            'o' => expanded.push_str(output),
            's' => expanded.push_str(size),
            '%' => expanded.push('%'),
            _ => {
                return Err(ThumbnailError::InvalidEntry(format!(
                    "unsupported Exec field %{code}"
                )));
            }
        }
    }
    Ok(expanded)
}

fn wait_for_helper(child: &mut Child, cancelled: &AtomicBool) -> Result<(), ThumbnailError> {
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ThumbnailError::Cancelled);
        }
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(ThumbnailError::Image(format!(
                    "thumbnail helper exited with {status}"
                )))
            };
        }
        if started.elapsed() >= HELPER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ThumbnailError::TimedOut);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn temporary_path(destination: &Path, suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    destination.with_extension(format!("png.{suffix}.{}.{nonce}", std::process::id()))
}

fn write_thumbnail(
    destination: &Path,
    image: &image::RgbaImage,
    uri: &str,
    modified: u64,
    source_size: u64,
    cancelled: &AtomicBool,
) -> Result<(), ThumbnailError> {
    ensure_not_cancelled(cancelled)?;
    let temporary = temporary_path(destination, "tmp");
    let file = File::create(&temporary)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .add_text_chunk("Thumb::URI".into(), uri.into())
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    encoder
        .add_text_chunk("Thumb::MTime".into(), modified.to_string())
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    encoder
        .add_text_chunk("Thumb::Size".into(), source_size.to_string())
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    encoder
        .add_text_chunk("Software".into(), "gnil-fm".into())
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    let mut writer = encoder
        .write_header()
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    writer
        .write_image_data(image.as_raw())
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    writer
        .finish()
        .map_err(|error| ThumbnailError::Png(error.to_string()))?;
    ensure_not_cancelled(cancelled)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_thumbnailer_entries_and_expands_exec_fields() {
        let source = r"
            [Thumbnailer Entry]
            TryExec=video-thumbnailer
            Exec=video-thumbnailer --size %s --input %i --uri %u --output %o
            MimeType=video/mp4;video/webm;
        ";
        let (entry, mime_types) = parse_thumbnailer_entry(source).unwrap();
        assert_eq!(entry.try_exec.as_deref(), Some("video-thumbnailer"));
        assert_eq!(mime_types, ["video/mp4", "video/webm"]);
        assert_eq!(
            expand_exec_argument(
                "%i:%s:%%",
                "/tmp/a.mp4",
                "file:///tmp/a.mp4",
                "/tmp/o",
                "256"
            )
            .unwrap(),
            "/tmp/a.mp4:256:%"
        );
    }

    #[test]
    fn unsupported_exec_fields_are_rejected() {
        assert!(expand_exec_argument("%x", "in", "uri", "out", "256").is_err());
    }

    #[test]
    fn image_thumbnail_is_cached_and_invalidated_by_metadata() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.png");
        image::RgbaImage::from_pixel(32, 16, image::Rgba([10, 20, 30, 255]))
            .save(&source)
            .unwrap();
        let metadata = fs::metadata(&source).unwrap();
        let uri = file_uri(&source).unwrap();
        let destination = root.path().join("thumb.png");
        create_image_thumbnail(
            &source,
            &destination,
            &uri,
            modified_seconds(&metadata).unwrap(),
            metadata.len(),
            24,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(valid_cached_thumbnail(
            &destination,
            &uri,
            modified_seconds(&metadata).unwrap(),
            metadata.len()
        ));
        assert!(!valid_cached_thumbnail(
            &destination,
            &uri,
            modified_seconds(&metadata).unwrap(),
            metadata.len() + 1
        ));
    }
}
