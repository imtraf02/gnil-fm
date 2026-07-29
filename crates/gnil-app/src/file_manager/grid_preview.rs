impl FileManager {
    fn ensure_grid_preview(
        &mut self,
        key: PathBuf,
        source: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.grid_previews.contains_key(&key) {
            return;
        }
        self.grid_previews
            .insert(key.clone(), GridPreviewState::Loading);
        self.grid_preview_queue
            .push_back(GridPreviewRequest { key, source });
        self.pump_grid_preview_queue(cx);
    }

    fn pump_grid_preview_queue(&mut self, cx: &mut Context<Self>) {
        while self.grid_preview_inflight.len() < 2 {
            let Some(request) = self.grid_preview_queue.pop_front() else {
                break;
            };
            if !self.grid_preview_inflight.insert(request.key.clone()) {
                continue;
            }
            let generation = self.grid_preview_generation;
            let cancel = Arc::clone(&self.grid_preview_cancel);
            let preview_service = Arc::clone(&self.preview_service);
            let thumbnail_service = Arc::clone(&self.thumbnail_service);
            let source = request.source.clone();
            let task = cx.background_executor().spawn(async move {
                if cancel.load(Ordering::Relaxed) {
                    return GridPreviewState::Unavailable;
                }
                match preview_service.preview_cancellable(
                    &PreviewRequest::initial(source.clone()),
                    &cancel,
                ) {
                    Ok(PreviewResult::Text(text)) => {
                        let lines = text
                            .lines
                            .into_iter()
                            .take(6)
                            .map(|line| {
                                let mut line = line
                                    .segments
                                    .into_iter()
                                    .map(|segment| segment.text)
                                    .collect::<String>();
                                line.truncate(120);
                                line
                            })
                            .collect();
                        GridPreviewState::Text(lines)
                    }
                    Ok(
                        PreviewResult::Image(_)
                        | PreviewResult::Metadata(_)
                        | PreviewResult::Directory(_),
                    )
                    | Err(_) => thumbnail_service
                        .thumbnail_cancellable(&ThumbnailRequest::grid(source), &cancel)
                        .map_or(GridPreviewState::Unavailable, |thumbnail| {
                            GridPreviewState::Thumbnail(thumbnail.path)
                        }),
                }
            });
            let key = request.key;
            cx.spawn(async move |this, cx| {
                let preview = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.grid_preview_inflight.remove(&key);
                    if this.grid_preview_generation == generation {
                        this.grid_previews.insert(key, preview);
                        this.invalidate_surfaces(SurfaceMask::FILE_LIST, cx);
                    }
                    this.pump_grid_preview_queue(cx);
                });
            })
            .detach();
        }
    }

    fn reset_grid_previews(&mut self) {
        self.grid_preview_cancel.store(true, Ordering::Relaxed);
        self.grid_preview_cancel = Arc::new(AtomicBool::new(false));
        self.grid_preview_generation = self.grid_preview_generation.wrapping_add(1);
        self.grid_previews.clear();
        self.grid_preview_queue.clear();
        self.grid_preview_inflight.clear();
    }
}
