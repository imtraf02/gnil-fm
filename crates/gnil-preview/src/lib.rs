//! Bounded, cancellation-friendly file preview generation.

use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use image::ImageReader;
use syntect::{
    easy::HighlightLines,
    highlighting::{Color, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use thiserror::Error;

pub const INITIAL_TEXT_LIMIT: u64 = 2 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub text_limit: u64,
}

impl PreviewRequest {
    #[must_use]
    pub fn initial(path: PathBuf) -> Self {
        Self {
            path,
            text_limit: INITIAL_TEXT_LIMIT,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewResult {
    Directory(DirectoryPreview),
    Text(TextPreview),
    Image(ImagePreview),
    Metadata(MetadataPreview),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryPreview {
    pub child_count: usize,
    pub unreadable_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPreview {
    pub lines: Vec<HighlightedLine>,
    pub truncated: bool,
    pub syntax: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightedLine {
    pub segments: Vec<HighlightedSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightedSegment {
    pub text: String,
    pub foreground: [u8; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImagePreview {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub decode_allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPreview {
    pub mime: String,
    pub len: u64,
    pub modified_unix_ms: Option<i64>,
    pub readonly: bool,
}

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("path cannot be previewed: {0}")]
    Unsupported(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("image metadata error: {0}")]
    Image(String),
    #[error("syntax highlighting error: {0}")]
    Highlight(String),
}

pub struct PreviewService {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

impl Default for PreviewService {
    fn default() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
        }
    }
}

impl PreviewService {
    pub fn preview(&self, request: &PreviewRequest) -> Result<PreviewResult, PreviewError> {
        let metadata = fs::symlink_metadata(&request.path)?;
        if metadata.is_dir() {
            return Self::preview_directory(&request.path);
        }
        if !metadata.is_file() {
            return Ok(PreviewResult::Metadata(metadata_preview(
                &request.path,
                &metadata,
            )));
        }
        if is_svg(&request.path) {
            return Ok(PreviewResult::Metadata(metadata_preview(
                &request.path,
                &metadata,
            )));
        }
        if let Ok(image) = image_dimensions(&request.path) {
            return Ok(PreviewResult::Image(image));
        }
        let mut file = File::open(&request.path)?;
        let read_limit = request.text_limit.min(metadata.len());
        let mut bytes =
            Vec::with_capacity(usize::try_from(read_limit.min(8 * 1024 * 1024)).unwrap_or(0));
        file.by_ref().take(read_limit).read_to_end(&mut bytes)?;
        if bytes.iter().take(8 * 1024).any(|byte| *byte == 0) {
            return Ok(PreviewResult::Metadata(metadata_preview(
                &request.path,
                &metadata,
            )));
        }
        let text = String::from_utf8_lossy(&bytes);
        Ok(PreviewResult::Text(self.highlight(
            &request.path,
            &text,
            metadata.len() > read_limit,
        )?))
    }

    fn preview_directory(path: &Path) -> Result<PreviewResult, PreviewError> {
        let mut child_count = 0;
        let mut unreadable_count = 0;
        for item in fs::read_dir(path)? {
            if item.is_ok() {
                child_count += 1;
            } else {
                unreadable_count += 1;
            }
        }
        Ok(PreviewResult::Directory(DirectoryPreview {
            child_count,
            unreadable_count,
        }))
    }

    fn highlight(
        &self,
        path: &Path,
        text: &str,
        truncated: bool,
    ) -> Result<TextPreview, PreviewError> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(path)
            .map_err(|error| PreviewError::Highlight(error.to_string()))?
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let theme = self
            .themes
            .themes
            .get("base16-ocean.dark")
            .or_else(|| self.themes.themes.values().next())
            .ok_or_else(|| PreviewError::Highlight("no syntax theme available".into()))?;
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(text).take(10_000) {
            let ranges = highlighter
                .highlight_line(line, &self.syntaxes)
                .map_err(|error| PreviewError::Highlight(error.to_string()))?;
            lines.push(HighlightedLine {
                segments: ranges
                    .into_iter()
                    .map(|(style, text)| HighlightedSegment {
                        text: text.to_owned(),
                        foreground: color(style.foreground),
                    })
                    .collect(),
            });
        }
        Ok(TextPreview {
            lines,
            truncated,
            syntax: syntax.name.clone(),
        })
    }
}

fn color(color: Color) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}

fn image_dimensions(path: &Path) -> Result<ImagePreview, PreviewError> {
    let reader = ImageReader::open(path)
        .map_err(PreviewError::Io)?
        .with_guessed_format()
        .map_err(PreviewError::Io)?;
    let format = reader
        .format()
        .ok_or_else(|| PreviewError::Unsupported(path.to_path_buf()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| PreviewError::Image(error.to_string()))?;
    Ok(ImagePreview {
        width,
        height,
        format: format
            .extensions_str()
            .first()
            .copied()
            .unwrap_or("image")
            .to_owned(),
        decode_allowed: u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS,
    })
}

fn is_svg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

fn metadata_preview(path: &Path, metadata: &fs::Metadata) -> MetadataPreview {
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    MetadataPreview {
        mime: mime_guess::from_path(path)
            .first_raw()
            .unwrap_or("application/octet-stream")
            .into(),
        len: metadata.len(),
        modified_unix_ms,
        readonly: metadata.permissions().readonly(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn text_preview_highlights_and_reports_truncation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("main.rs");
        fs::write(&path, "fn main() { println!(\"hello\"); }\n").unwrap();
        let preview = PreviewService::default()
            .preview(&PreviewRequest {
                path,
                text_limit: 8,
            })
            .unwrap();
        let PreviewResult::Text(preview) = preview else {
            panic!("expected text")
        };
        assert!(preview.truncated);
        assert_eq!(preview.syntax, "Rust");
        assert!(!preview.lines.is_empty());
    }

    #[test]
    fn binary_file_falls_back_to_metadata() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("blob.bin");
        fs::write(&path, [0, 1, 2, 3]).unwrap();
        let preview = PreviewService::default()
            .preview(&PreviewRequest::initial(path))
            .unwrap();
        assert!(matches!(preview, PreviewResult::Metadata(_)));
    }
}
