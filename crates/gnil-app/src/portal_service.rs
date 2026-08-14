use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use async_io::Timer;
use futures_lite::future;
use gpui::Application;
use zbus::{
    Connection, ObjectServer, interface,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use crate::{
    picker::{self, PickerAssets, PickerUiCommand},
    portal_protocol::{
        OpenFileOptions, PickerOutcome, PickerRequest, PickerRequestKind, PortalOptions,
        PortalResponse, SaveFileOptions, SaveFilesOptions,
    },
};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.gnilfm";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_ACTIVE_REQUESTS: usize = 8;
const MAX_ACTIVE_REQUESTS_PER_APP: usize = 2;
const UI_CHANNEL_CAPACITY: usize = 16;

#[derive(Clone)]
struct FileChooserBackend {
    ui: async_channel::Sender<PickerUiCommand>,
    quota: Arc<Mutex<PortalQuota>>,
}

#[derive(Default)]
struct PortalQuota {
    total: usize,
    per_app: HashMap<String, usize>,
}

struct PortalPermit {
    quota: Arc<Mutex<PortalQuota>>,
    app_id: String,
}

impl Drop for PortalPermit {
    fn drop(&mut self) {
        let Ok(mut quota) = self.quota.lock() else {
            return;
        };
        quota.total = quota.total.saturating_sub(1);
        let remove_app = if let Some(count) = quota.per_app.get_mut(&self.app_id) {
            *count = count.saturating_sub(1);
            *count == 0
        } else {
            false
        };
        if remove_app {
            quota.per_app.remove(&self.app_id);
        }
    }
}

#[derive(Clone)]
struct PortalRequestObject {
    cancel: async_channel::Sender<()>,
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequestObject {
    #[zbus(name = "Close")]
    #[allow(clippy::unused_async)]
    async fn close(&self) {
        let _ = self.cancel.try_send(());
    }
}

enum StartupState {
    Started(Result<(), String>),
    Cancelled,
    TimedOut,
}

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooserBackend {
    #[zbus(property(emits_changed_signal = "const"), name = "version")]
    #[allow(clippy::unused_self)]
    fn version(&self) -> u32 {
        4
    }

    #[zbus(name = "OpenFile", out_args("response", "results"))]
    async fn open_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: PortalOptions,
        #[zbus(object_server)] object_server: &ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let Ok(options) = OpenFileOptions::parse(&options) else {
            return error_result();
        };
        self.run_request(
            PickerRequest {
                handle: handle.to_string(),
                app_id,
                parent_window,
                title,
                kind: PickerRequestKind::Open(options),
            },
            handle,
            object_server,
            true,
        )
        .await
    }

    #[zbus(name = "SaveFile", out_args("response", "results"))]
    async fn save_file(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: PortalOptions,
        #[zbus(object_server)] object_server: &ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let Ok(options) = SaveFileOptions::parse(&options) else {
            return error_result();
        };
        self.run_request(
            PickerRequest {
                handle: handle.to_string(),
                app_id,
                parent_window,
                title,
                kind: PickerRequestKind::Save(options),
            },
            handle,
            object_server,
            false,
        )
        .await
    }

    #[zbus(name = "SaveFiles", out_args("response", "results"))]
    async fn save_files(
        &self,
        handle: OwnedObjectPath,
        app_id: String,
        parent_window: String,
        title: String,
        options: PortalOptions,
        #[zbus(object_server)] object_server: &ObjectServer,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let Ok(options) = SaveFilesOptions::parse(&options) else {
            return error_result();
        };
        self.run_request(
            PickerRequest {
                handle: handle.to_string(),
                app_id,
                parent_window,
                title,
                kind: PickerRequestKind::SaveMany(options),
            },
            handle,
            object_server,
            false,
        )
        .await
    }
}

impl FileChooserBackend {
    fn try_acquire(&self, app_id: &str) -> Option<PortalPermit> {
        let mut quota = self.quota.lock().ok()?;
        let app_count = quota.per_app.get(app_id).copied().unwrap_or(0);
        if quota.total >= MAX_ACTIVE_REQUESTS || app_count >= MAX_ACTIVE_REQUESTS_PER_APP {
            return None;
        }
        quota.total += 1;
        *quota.per_app.entry(app_id.to_owned()).or_default() += 1;
        Some(PortalPermit {
            quota: Arc::clone(&self.quota),
            app_id: app_id.to_owned(),
        })
    }

    async fn run_request(
        &self,
        request: PickerRequest,
        handle: OwnedObjectPath,
        object_server: &ObjectServer,
        include_writable: bool,
    ) -> (u32, HashMap<String, OwnedValue>) {
        if request.validate().is_err() {
            return error_result();
        }
        let Some(_permit) = self.try_acquire(&request.app_id) else {
            return error_result();
        };
        let (cancel_tx, cancel_rx) = async_channel::bounded(1);
        let request_object = PortalRequestObject { cancel: cancel_tx };
        match object_server.at(handle.clone(), request_object).await {
            Ok(true) => {}
            Ok(false) | Err(_) => return error_result(),
        }

        let (response_tx, response_rx) = async_channel::bounded(1);
        let (started_tx, started_rx) = async_channel::bounded(1);
        if self
            .ui
            .send(PickerUiCommand::Open {
                request: Box::new(request.clone()),
                response: response_tx,
                started: started_tx,
            })
            .await
            .is_err()
        {
            let _ = object_server
                .remove::<PortalRequestObject, _>(&handle)
                .await;
            return error_result();
        }

        let outcome = await_picker(
            &self.ui,
            &request.handle,
            cancel_rx,
            response_rx,
            started_rx,
            STARTUP_TIMEOUT,
            REQUEST_TIMEOUT,
        )
        .await;

        let _ = object_server
            .remove::<PortalRequestObject, _>(&handle)
            .await;
        response_result(PortalResponse::from_outcome(outcome), include_writable)
    }
}

async fn await_picker(
    ui: &async_channel::Sender<PickerUiCommand>,
    handle: &str,
    cancel_rx: async_channel::Receiver<()>,
    response_rx: async_channel::Receiver<PickerOutcome>,
    started_rx: async_channel::Receiver<Result<(), String>>,
    startup_timeout: Duration,
    request_timeout: Duration,
) -> PickerOutcome {
    let startup = future::or(
        async {
            StartupState::Started(
                started_rx
                    .recv()
                    .await
                    .unwrap_or_else(|_| Err("picker UI channel closed".into())),
            )
        },
        future::or(
            async {
                let _ = cancel_rx.recv().await;
                StartupState::Cancelled
            },
            async {
                Timer::after(startup_timeout).await;
                StartupState::TimedOut
            },
        ),
    )
    .await;

    match startup {
        StartupState::Started(Ok(())) => {
            future::or(
                async {
                    response_rx.recv().await.unwrap_or_else(|_| {
                        PickerOutcome::Failed("picker UI channel closed".into())
                    })
                },
                future::or(
                    async {
                        let _ = cancel_rx.recv().await;
                        close_picker(ui, handle.to_owned()).await;
                        PickerOutcome::Cancelled
                    },
                    async {
                        Timer::after(request_timeout).await;
                        close_picker(ui, handle.to_owned()).await;
                        PickerOutcome::Failed("picker request timed out".into())
                    },
                ),
            )
            .await
        }
        StartupState::Cancelled => {
            close_picker(ui, handle.to_owned()).await;
            PickerOutcome::Cancelled
        }
        StartupState::Started(Err(error)) => {
            close_picker(ui, handle.to_owned()).await;
            PickerOutcome::Failed(error)
        }
        StartupState::TimedOut => {
            close_picker(ui, handle.to_owned()).await;
            PickerOutcome::Failed("picker window creation timed out".into())
        }
    }
}

async fn close_picker(ui: &async_channel::Sender<PickerUiCommand>, handle: String) {
    let _ = ui.send(PickerUiCommand::Close { handle }).await;
}

fn error_result() -> (u32, HashMap<String, OwnedValue>) {
    response_result(PortalResponse::error(), false)
}

fn response_result(
    response: PortalResponse,
    include_writable: bool,
) -> (u32, HashMap<String, OwnedValue>) {
    if response.code != 0 {
        return (response.code, HashMap::new());
    }
    let mut results = HashMap::new();
    results.insert("uris".into(), owned_value(response.uris));
    results.insert("choices".into(), owned_value(response.choices));
    if let Some(filter) = response.current_filter {
        results.insert("current_filter".into(), owned_value(filter));
    }
    if include_writable {
        results.insert("writable".into(), OwnedValue::from(false));
    }
    (response.code, results)
}

fn owned_value<T>(value: T) -> OwnedValue
where
    T: Into<Value<'static>>,
{
    let value: Value<'static> = value.into();
    OwnedValue::try_from(value).expect("portal results use valid D-Bus value types")
}

pub fn run() -> anyhow::Result<()> {
    let (ui_tx, ui_rx) = async_channel::bounded(UI_CHANNEL_CAPACITY);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("gnil-filechooser-dbus".into())
        .spawn(move || {
            let result = zbus::block_on(run_dbus(ui_tx, ready_tx.clone()));
            if let Err(error) = result {
                let _ = ready_tx.send(Err(error.to_string()));
            }
        })?;

    match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => anyhow::bail!(error),
        Err(error) => anyhow::bail!("portal D-Bus service did not start: {error}"),
    }

    Application::new()
        .with_assets(PickerAssets)
        .with_quit_policy(gpui::QuitPolicy::Explicit)
        .run(move |cx| {
            picker::bind_keys(cx);
            picker::run_command_loop(ui_rx, cx);
        });
    Ok(())
}

async fn run_dbus(
    ui: async_channel::Sender<PickerUiCommand>,
    ready: mpsc::SyncSender<Result<(), String>>,
) -> anyhow::Result<()> {
    let backend = FileChooserBackend {
        ui,
        quota: Arc::new(Mutex::new(PortalQuota::default())),
    };
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(DESKTOP_PATH, backend)?
        .build()
        .await?;

    // The connection owns the object server and keeps processing requests on zbus' executor.
    // Keep it alive for the remainder of the desktop session.
    let _connection: Connection = connection;
    let _ = ready.send(Ok(()));
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_results_have_portal_shapes() {
        let response = PortalResponse {
            code: 0,
            uris: vec!["file:///tmp/example".into()],
            choices: vec![("encoding".into(), "utf8".into())],
            current_filter: Some(("Text".into(), vec![(0, "*.txt".into())])),
        };
        let (code, results) = response_result(response, true);
        assert_eq!(code, 0);
        assert!(results.contains_key("uris"));
        assert!(results.contains_key("choices"));
        assert!(results.contains_key("current_filter"));
        assert!(!bool::try_from(results["writable"].clone()).unwrap());
    }

    #[test]
    fn cancellation_has_no_result_payload() {
        let (code, results) = response_result(PortalResponse::cancelled(), false);
        assert_eq!(code, 1);
        assert!(results.is_empty());
    }

    #[test]
    fn errors_have_no_result_payload() {
        let (code, results) = response_result(PortalResponse::error(), true);
        assert_eq!(code, 2);
        assert!(results.is_empty());
    }

    #[test]
    fn request_close_is_idempotent() {
        zbus::block_on(async {
            let (cancel, cancelled) = async_channel::bounded(1);
            let request = PortalRequestObject { cancel };

            request.close().await;
            request.close().await;

            assert!(cancelled.recv().await.is_ok());
            assert!(cancelled.try_recv().is_err());
        });
    }

    #[test]
    fn quota_limits_total_and_per_application_requests() {
        let (ui, _commands) = async_channel::bounded(1);
        let backend = FileChooserBackend {
            ui,
            quota: Arc::new(Mutex::new(PortalQuota::default())),
        };
        let first = backend.try_acquire("app.one").unwrap();
        let second = backend.try_acquire("app.one").unwrap();
        assert!(backend.try_acquire("app.one").is_none());

        let mut others = Vec::new();
        for index in 0..MAX_ACTIVE_REQUESTS - 2 {
            others.push(backend.try_acquire(&format!("app.{index}")).unwrap());
        }
        assert!(backend.try_acquire("app.extra").is_none());
        drop(first);
        assert!(backend.try_acquire("app.extra").is_some());
        drop(second);
        drop(others);
    }

    #[test]
    fn active_picker_is_closed_after_request_timeout() {
        zbus::block_on(async {
            let (ui, commands) = async_channel::bounded(1);
            let (_cancel, cancel_rx) = async_channel::bounded(1);
            let (_response, response_rx) = async_channel::bounded(1);
            let (started, started_rx) = async_channel::bounded(1);
            started.send(Ok(())).await.unwrap();

            let outcome = await_picker(
                &ui,
                "/request/timeout",
                cancel_rx,
                response_rx,
                started_rx,
                Duration::from_secs(1),
                Duration::from_millis(1),
            )
            .await;

            assert!(matches!(outcome, PickerOutcome::Failed(error) if error.contains("timed out")));
            assert!(matches!(
                commands.recv().await,
                Ok(PickerUiCommand::Close { handle }) if handle == "/request/timeout"
            ));
        });
    }

    #[test]
    fn startup_failure_closes_only_its_picker() {
        zbus::block_on(async {
            let (ui, commands) = async_channel::unbounded();
            let (_cancel, cancel_rx) = async_channel::bounded(1);
            let (_response, response_rx) = async_channel::bounded(1);
            let (started, started_rx) = async_channel::bounded(1);
            started.send(Err("window failed".into())).await.unwrap();

            let outcome = await_picker(
                &ui,
                "/request/failure",
                cancel_rx,
                response_rx,
                started_rx,
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .await;

            assert_eq!(outcome, PickerOutcome::Failed("window failed".into()));
            assert!(matches!(
                commands.recv().await,
                Ok(PickerUiCommand::Close { handle }) if handle == "/request/failure"
            ));
        });
    }

    #[test]
    fn concurrent_picker_results_do_not_share_state() {
        zbus::block_on(async {
            let (ui, _commands) = async_channel::unbounded();
            let (_cancel_a, cancel_rx_a) = async_channel::bounded(1);
            let (_cancel_b, cancel_rx_b) = async_channel::bounded(1);
            let (response_a, response_rx_a) = async_channel::bounded(1);
            let (response_b, response_rx_b) = async_channel::bounded(1);
            let (started_a, started_rx_a) = async_channel::bounded(1);
            let (started_b, started_rx_b) = async_channel::bounded(1);
            started_a.send(Ok(())).await.unwrap();
            started_b.send(Ok(())).await.unwrap();
            response_a
                .send(PickerOutcome::Accepted {
                    paths: vec!["/tmp/a".into()],
                    choices: vec![("encoding".into(), "utf8".into())],
                    current_filter: None,
                })
                .await
                .unwrap();
            response_b
                .send(PickerOutcome::Accepted {
                    paths: vec!["/tmp/b".into()],
                    choices: vec![("encoding".into(), "latin1".into())],
                    current_filter: None,
                })
                .await
                .unwrap();

            let first = await_picker(
                &ui,
                "/request/a",
                cancel_rx_a,
                response_rx_a,
                started_rx_a,
                Duration::from_secs(1),
                Duration::from_secs(1),
            );
            let second = await_picker(
                &ui,
                "/request/b",
                cancel_rx_b,
                response_rx_b,
                started_rx_b,
                Duration::from_secs(1),
                Duration::from_secs(1),
            );
            let (first, second) = future::zip(first, second).await;

            assert!(matches!(
                first,
                PickerOutcome::Accepted { paths, choices, .. }
                    if paths == [std::path::PathBuf::from("/tmp/a")]
                        && choices == [("encoding".into(), "utf8".into())]
            ));
            assert!(matches!(
                second,
                PickerOutcome::Accepted { paths, choices, .. }
                    if paths == [std::path::PathBuf::from("/tmp/b")]
                        && choices == [("encoding".into(), "latin1".into())]
            ));
        });
    }
}
