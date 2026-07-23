use std::{
    collections::HashMap,
    sync::mpsc,
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

#[derive(Clone)]
struct FileChooserBackend {
    ui: async_channel::Sender<PickerUiCommand>,
}

#[derive(Clone)]
struct PortalRequestObject {
    cancel: async_channel::Sender<()>,
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequestObject {
    #[zbus(name = "Close")]
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
    async fn run_request(
        &self,
        request: PickerRequest,
        handle: OwnedObjectPath,
        object_server: &ObjectServer,
        include_writable: bool,
    ) -> (u32, HashMap<String, OwnedValue>) {
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
                request: request.clone(),
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
                    Timer::after(STARTUP_TIMEOUT).await;
                    StartupState::TimedOut
                },
            ),
        )
        .await;

        let outcome = match startup {
            StartupState::Started(Ok(())) => {
                future::or(
                    async {
                        response_rx.recv().await.unwrap_or_else(|_| {
                            PickerOutcome::Failed("picker UI channel closed".into())
                        })
                    },
                    async {
                        let _ = cancel_rx.recv().await;
                        close_picker(&self.ui, request.handle.clone()).await;
                        PickerOutcome::Cancelled
                    },
                )
                .await
            }
            StartupState::Cancelled => {
                close_picker(&self.ui, request.handle.clone()).await;
                PickerOutcome::Cancelled
            }
            StartupState::Started(Err(error)) => PickerOutcome::Failed(error),
            StartupState::TimedOut => {
                close_picker(&self.ui, request.handle.clone()).await;
                PickerOutcome::Failed("picker window creation timed out".into())
            }
        };

        let _ = object_server
            .remove::<PortalRequestObject, _>(&handle)
            .await;
        response_result(PortalResponse::from_outcome(outcome), include_writable)
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
    let (ui_tx, ui_rx) = async_channel::unbounded();
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
        .quit_when_last_window_closed(false)
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
    let backend = FileChooserBackend { ui };
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
}
