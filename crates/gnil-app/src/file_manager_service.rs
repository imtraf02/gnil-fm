use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use gpui::{Application, QuitPolicy};
use url::Url;
use zbus::{Connection, fdo, interface};

use crate::{
    assets::Assets,
    file_manager::{FileManagerOpenRequest, open_main_window},
};

const BUS_NAME: &str = "org.freedesktop.FileManager1";
const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestKind {
    Folders,
    Items,
    Properties,
}

#[derive(Clone)]
struct FileManagerService {
    ui: async_channel::Sender<Vec<FileManagerOpenRequest>>,
}

#[interface(name = "org.freedesktop.FileManager1")]
impl FileManagerService {
    #[zbus(name = "ShowFolders")]
    async fn show_folders(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        drop(startup_id);
        self.send_requests(RequestKind::Folders, uris).await
    }

    #[zbus(name = "ShowItems")]
    async fn show_items(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        drop(startup_id);
        self.send_requests(RequestKind::Items, uris).await
    }

    #[zbus(name = "ShowItemProperties")]
    async fn show_item_properties(&self, uris: Vec<String>, startup_id: String) -> fdo::Result<()> {
        drop(startup_id);
        self.send_requests(RequestKind::Properties, uris).await
    }
}

impl FileManagerService {
    async fn send_requests(&self, kind: RequestKind, uris: Vec<String>) -> fdo::Result<()> {
        let requests = open_requests(kind, &uris)?;
        self.ui
            .send(requests)
            .await
            .map_err(|_| fdo::Error::Failed("gnil-fm UI loop is unavailable".into()))
    }
}

fn open_requests(kind: RequestKind, uris: &[String]) -> fdo::Result<Vec<FileManagerOpenRequest>> {
    if uris.is_empty() {
        return Err(fdo::Error::InvalidArgs(
            "at least one local file URI is required".into(),
        ));
    }

    let paths = uris
        .iter()
        .map(|uri| local_path(uri))
        .collect::<fdo::Result<Vec<_>>>()?;

    match kind {
        RequestKind::Folders => folder_requests(paths),
        RequestKind::Items => item_requests(paths, false),
        RequestKind::Properties => item_requests(paths, true),
    }
}

fn local_path(uri: &str) -> fdo::Result<PathBuf> {
    let url = Url::parse(uri)
        .map_err(|error| fdo::Error::InvalidArgs(format!("invalid URI {uri:?}: {error}")))?;
    if url.scheme() != "file" {
        return Err(fdo::Error::InvalidArgs(format!(
            "only local file:// URIs are supported: {uri}"
        )));
    }
    let path = url.to_file_path().map_err(|()| {
        fdo::Error::InvalidArgs(format!("URI does not identify a local path: {uri}"))
    })?;
    if !path.is_absolute() {
        return Err(fdo::Error::InvalidArgs(format!(
            "URI must identify an absolute path: {uri}"
        )));
    }
    Ok(path)
}

fn folder_requests(paths: Vec<PathBuf>) -> fdo::Result<Vec<FileManagerOpenRequest>> {
    let mut folders = BTreeSet::new();
    for path in paths {
        if !path.is_dir() {
            return Err(fdo::Error::InvalidArgs(format!(
                "ShowFolders requires an existing directory: {}",
                path.display()
            )));
        }
        folders.insert(path);
    }
    Ok(folders
        .into_iter()
        .map(FileManagerOpenRequest::browse)
        .collect())
}

fn item_requests(
    paths: Vec<PathBuf>,
    show_properties: bool,
) -> fdo::Result<Vec<FileManagerOpenRequest>> {
    let mut grouped: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    let mut roots = BTreeSet::new();

    for path in paths {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            fdo::Error::InvalidArgs(format!("cannot access {}: {error}", path.display()))
        })?;
        if show_properties && metadata.file_type().is_symlink() {
            return Err(fdo::Error::NotSupported(format!(
                "properties for symbolic links are not supported: {}",
                path.display()
            )));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            grouped
                .entry(parent.to_path_buf())
                .or_default()
                .insert(path);
        } else if path == Path::new("/") {
            roots.insert(path);
        } else {
            return Err(fdo::Error::InvalidArgs(format!(
                "path has no containing directory: {}",
                path.display()
            )));
        }
    }

    let mut requests = Vec::with_capacity(grouped.len() + roots.len());
    for (directory, reveal) in grouped {
        requests.push(FileManagerOpenRequest::new(
            directory,
            reveal.into_iter().collect(),
            show_properties,
        ));
    }
    requests.extend(
        roots
            .into_iter()
            .map(|directory| FileManagerOpenRequest::new(directory, Vec::new(), show_properties)),
    );
    Ok(requests)
}

pub fn run() -> anyhow::Result<()> {
    let (ui_tx, ui_rx) = async_channel::unbounded();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("gnil-filemanager1-dbus".into())
        .spawn(move || {
            let result = zbus::block_on(run_dbus(ui_tx, ready_tx.clone()));
            if let Err(error) = result {
                let _ = ready_tx.send(Err(error.to_string()));
            }
        })?;

    match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => anyhow::bail!(error),
        Err(error) => anyhow::bail!("FileManager1 D-Bus service did not start: {error}"),
    }

    Application::new()
        .with_assets(Assets)
        .with_quit_policy(QuitPolicy::Explicit)
        .run(move |cx| {
            cx.spawn(async move |cx| {
                while let Ok(requests) = ui_rx.recv().await {
                    let _ = cx.update(|cx| {
                        for request in requests {
                            if let Err(error) = open_main_window(request, cx) {
                                eprintln!("Could not open gnil-fm window: {error}");
                            }
                        }
                    });
                }
            })
            .detach();
        });
    Ok(())
}

async fn run_dbus(
    ui: async_channel::Sender<Vec<FileManagerOpenRequest>>,
    ready: mpsc::SyncSender<Result<(), String>>,
) -> anyhow::Result<()> {
    let service = FileManagerService { ui };
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, service)?
        .build()
        .await?;

    let _connection: Connection = connection;
    let _ = ready.send(Ok(()));
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(path: &Path) -> String {
        Url::from_file_path(path).unwrap().into()
    }

    #[test]
    fn show_items_groups_and_deduplicates_by_parent() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second file.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        let requests = open_requests(
            RequestKind::Items,
            &[uri(&second), uri(&first), uri(&first)],
        )
        .unwrap();

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].directory, temp.path());
        assert_eq!(requests[0].reveal, vec![first, second]);
        assert!(!requests[0].show_properties);
    }

    #[test]
    fn show_folders_rejects_regular_files_without_partial_results() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("folder");
        let file = temp.path().join("file.txt");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(&file, b"file").unwrap();

        let error = open_requests(RequestKind::Folders, &[uri(&folder), uri(&file)]).unwrap_err();

        assert!(matches!(error, fdo::Error::InvalidArgs(_)));
    }

    #[test]
    fn remote_and_missing_uris_are_rejected() {
        let remote = open_requests(RequestKind::Items, &["smb://server/share/file".into()]);
        assert!(matches!(remote, Err(fdo::Error::InvalidArgs(_))));

        let temp = tempfile::tempdir().unwrap();
        let missing = open_requests(RequestKind::Items, &[uri(&temp.path().join("missing"))]);
        assert!(matches!(missing, Err(fdo::Error::InvalidArgs(_))));
    }

    #[cfg(unix)]
    #[test]
    fn properties_reject_symbolic_links() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::write(&target, b"target").unwrap();
        symlink(&target, &link).unwrap();

        let result = open_requests(RequestKind::Properties, &[uri(&link)]);

        assert!(matches!(result, Err(fdo::Error::NotSupported(_))));
    }
}
