//! File clipboard MIME codec shared by the Wayland transport and UI.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

pub const URI_LIST_MIME: &str = "text/uri-list";
pub const GNOME_FILES_MIME: &str = "x-special/gnome-copied-files";
pub const KDE_CUT_MIME: &str = "application/x-kde-cutselection";
pub const TEXT_MIME: &str = "text/plain;charset=utf-8";
pub const GNIL_FILES_MIME: &str = "application/x-gnil-fm-file-operation+json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileClipboardMode {
    Copy,
    Cut,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileClipboard {
    pub mode: FileClipboardMode,
    pub paths: Vec<PathBuf>,
    pub token: String,
}

impl FileClipboard {
    #[must_use]
    pub fn new(mode: FileClipboardMode, paths: Vec<PathBuf>) -> Self {
        Self {
            mode,
            paths,
            token: Uuid::new_v4().to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MimePayload {
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum ClipboardCodecError {
    #[error("file clipboard contains no paths")]
    Empty,
    #[error("path cannot be represented as an absolute local file URI: {0}")]
    InvalidPath(PathBuf),
    #[error("clipboard path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[error("clipboard contains a malformed URI: {0}")]
    MalformedUri(String),
    #[error("remote clipboard URI is unsupported: {0}")]
    RemoteUri(String),
    #[error("invalid gnil-fm clipboard payload: {0}")]
    InvalidInternalPayload(String),
}

pub fn encode_file_clipboard(
    clipboard: &FileClipboard,
) -> Result<Vec<MimePayload>, ClipboardCodecError> {
    if clipboard.paths.is_empty() {
        return Err(ClipboardCodecError::Empty);
    }

    let paths: Vec<_> = clipboard
        .paths
        .iter()
        .map(|path| {
            std::path::absolute(path).map_err(|_| ClipboardCodecError::InvalidPath(path.clone()))
        })
        .collect::<Result<_, _>>()?;
    let uris: Vec<_> = paths
        .iter()
        .map(|path| {
            Url::from_file_path(path)
                .map(Into::into)
                .map_err(|()| ClipboardCodecError::InvalidPath(path.clone()))
        })
        .collect::<Result<Vec<String>, _>>()?;
    let uri_list = uris.join("\r\n") + "\r\n";
    let operation = match clipboard.mode {
        FileClipboardMode::Copy => "copy",
        FileClipboardMode::Cut => "cut",
    };
    let gnome = format!("{operation}\n{}", uris.join("\n"));
    let text = paths
        .iter()
        .map(|path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| ClipboardCodecError::NonUtf8Path(path.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    let internal = serde_json::to_vec(clipboard)
        .map_err(|error| ClipboardCodecError::InvalidInternalPayload(error.to_string()))?;

    Ok(vec![
        MimePayload {
            mime_type: URI_LIST_MIME,
            bytes: uri_list.into_bytes(),
        },
        MimePayload {
            mime_type: GNOME_FILES_MIME,
            bytes: gnome.into_bytes(),
        },
        MimePayload {
            mime_type: KDE_CUT_MIME,
            bytes: match clipboard.mode {
                FileClipboardMode::Copy => b"0".to_vec(),
                FileClipboardMode::Cut => b"1".to_vec(),
            },
        },
        MimePayload {
            mime_type: TEXT_MIME,
            bytes: text.into_bytes(),
        },
        MimePayload {
            mime_type: GNIL_FILES_MIME,
            bytes: internal,
        },
    ])
}

pub fn decode_file_clipboard(
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<FileClipboard>, ClipboardCodecError> {
    if let Some(bytes) = payloads.get(GNIL_FILES_MIME) {
        let value = serde_json::from_slice(bytes)
            .map_err(|error| ClipboardCodecError::InvalidInternalPayload(error.to_string()))?;
        return Ok(Some(value));
    }
    if let Some(bytes) = payloads.get(GNOME_FILES_MIME) {
        return decode_gnome(bytes).map(Some);
    }
    let Some(bytes) = payloads.get(URI_LIST_MIME) else {
        return Ok(None);
    };
    let mode = if payloads
        .get(KDE_CUT_MIME)
        .is_some_and(|marker| marker.first() == Some(&b'1'))
    {
        FileClipboardMode::Cut
    } else {
        FileClipboardMode::Copy
    };
    let paths = decode_uri_lines(bytes)?;
    Ok(Some(FileClipboard {
        mode,
        paths,
        token: String::new(),
    }))
}

fn decode_gnome(bytes: &[u8]) -> Result<FileClipboard, ClipboardCodecError> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mode = match lines.next() {
        Some("copy") => FileClipboardMode::Copy,
        Some("cut") => FileClipboardMode::Cut,
        Some(other) => return Err(ClipboardCodecError::MalformedUri(other.into())),
        None => return Err(ClipboardCodecError::Empty),
    };
    let paths = decode_uris(lines)?;
    Ok(FileClipboard {
        mode,
        paths,
        token: String::new(),
    })
}

fn decode_uri_lines(bytes: &[u8]) -> Result<Vec<PathBuf>, ClipboardCodecError> {
    decode_uris(String::from_utf8_lossy(bytes).lines())
}

fn decode_uris<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<Vec<PathBuf>, ClipboardCodecError> {
    let mut paths = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let url =
            Url::parse(line).map_err(|_| ClipboardCodecError::MalformedUri(line.to_owned()))?;
        if url.scheme() != "file"
            || url
                .host_str()
                .is_some_and(|host| !host.is_empty() && host != "localhost")
        {
            return Err(ClipboardCodecError::RemoteUri(line.to_owned()));
        }
        let path = url
            .to_file_path()
            .map_err(|()| ClipboardCodecError::MalformedUri(line.to_owned()))?;
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(ClipboardCodecError::Empty);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_internal_and_encodes_desktop_formats() {
        let clipboard = FileClipboard {
            mode: FileClipboardMode::Cut,
            paths: vec![
                PathBuf::from("/tmp/a file.txt"),
                PathBuf::from("/tmp/β.txt"),
            ],
            token: "token".into(),
        };
        let encoded = encode_file_clipboard(&clipboard).unwrap();
        let payloads = encoded
            .into_iter()
            .map(|payload| (payload.mime_type.to_owned(), payload.bytes))
            .collect();
        assert_eq!(decode_file_clipboard(&payloads).unwrap(), Some(clipboard));
        assert_eq!(payloads[KDE_CUT_MIME], b"1");
        assert!(String::from_utf8_lossy(&payloads[URI_LIST_MIME]).contains("a%20file.txt"));
    }

    #[test]
    fn decodes_gnome_copy_and_kde_cut() {
        let gnome = BTreeMap::from([(
            GNOME_FILES_MIME.to_owned(),
            b"copy\nfile:///tmp/one\nfile:///tmp/two".to_vec(),
        )]);
        let decoded = decode_file_clipboard(&gnome).unwrap().unwrap();
        assert_eq!(decoded.mode, FileClipboardMode::Copy);
        assert_eq!(
            decoded.paths,
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );

        let kde = BTreeMap::from([
            (URI_LIST_MIME.to_owned(), b"file:///tmp/one\r\n".to_vec()),
            (KDE_CUT_MIME.to_owned(), b"1".to_vec()),
        ]);
        assert_eq!(
            decode_file_clipboard(&kde).unwrap().unwrap().mode,
            FileClipboardMode::Cut
        );
    }

    #[test]
    fn rejects_remote_uris() {
        let payloads = BTreeMap::from([(
            URI_LIST_MIME.to_owned(),
            b"smb://server/share/file".to_vec(),
        )]);
        assert!(matches!(
            decode_file_clipboard(&payloads),
            Err(ClipboardCodecError::RemoteUri(_))
        ));
    }
}
