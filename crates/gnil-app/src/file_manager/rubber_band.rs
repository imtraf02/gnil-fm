impl FileManager {
    fn begin_rubber_band(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.snapshot.entries.is_empty() {
            if merge_from_modifiers(event.modifiers) == gnil_core::SelectionMerge::Replace
                && self.selection.clear()
            {
                self.selection_changed(cx);
            }
            return;
        }
        let (viewport, offset_y) = self.file_list_metrics();
        self.pointer_serial = self.pointer_serial.wrapping_add(1);
        let content_y = f32::from(event.position.y - viewport.top()) - offset_y;
        self.pointer_interaction = PointerInteraction::RubberBand(RubberBandState {
            serial: self.pointer_serial,
            origin: event.position,
            current: event.position,
            frame: RubberFrameGate::default(),
            origin_content_y: content_y,
            current_content_y: content_y,
            baseline: self.selection.clone(),
            merge: merge_from_modifiers(event.modifiers),
            crossed_threshold: false,
            hit_span: None,
            autoscroll_scheduled: false,
        });
        cx.notify();
    }

    fn file_list_metrics(&self) -> (Bounds<Pixels>, f32) {
        let state = self.file_list_scroll.0.borrow();
        (
            state.base_handle.bounds(),
            f32::from(state.base_handle.offset().y),
        )
    }

    fn active_row_height(&self) -> f32 {
        if self.tab.root == TabRoot::Trash {
            TRASH_ROW_HEIGHT
        } else {
            FILE_ROW_HEIGHT
        }
    }

    fn update_rubber_band(
        &mut self,
        position: gpui::Point<Pixels>,
        schedule_autoscroll: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let (viewport, offset_y) = self.file_list_metrics();
        let row_height = self.active_row_height();
        let item_count = self.snapshot.entries.len();
        let rendered_range = self.rendered_file_range.clone();
        let viewport_left = f32::from(viewport.left());
        let viewport_right = f32::from(viewport.right());
        let mut should_schedule = false;
        let mut changed = false;
        if let PointerInteraction::RubberBand(state) = &mut self.pointer_interaction {
            let current_content_y = f32::from(position.y - viewport.top()) - offset_y;
            let crossed_threshold =
                state.crossed_threshold || movement_crossed_threshold(state.origin, position);
            let left = f32::from(state.origin.x).min(f32::from(position.x));
            let right = f32::from(state.origin.x).max(f32::from(position.x));
            let candidate_span = band_span(
                state.origin_content_y,
                current_content_y,
                row_height,
                item_count,
                right >= viewport_left && left <= viewport_right,
            );
            let hit_span = candidate_span.filter(|span| {
                !rendered_range.is_empty()
                    && *span.end() >= rendered_range.start
                    && *span.start() < rendered_range.end
            });
            changed = state.current != position
                || state.current_content_y.to_bits() != current_content_y.to_bits()
                || state.crossed_threshold != crossed_threshold
                || state.hit_span != hit_span;
            state.current = position;
            state.current_content_y = current_content_y;
            state.crossed_threshold = crossed_threshold;
            state.hit_span = hit_span;
            should_schedule = schedule_autoscroll
                && state.crossed_threshold
                && !state.autoscroll_scheduled
                && Self::rubber_scroll_velocity(viewport, position) != 0.0;
        }
        if should_schedule {
            self.schedule_rubber_autoscroll(cx);
        }
        if changed {
            self.surfaces.invalidate_file_list(cx);
        }
        changed
    }

    fn queue_rubber_band_update(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let serial = match &mut self.pointer_interaction {
            PointerInteraction::RubberBand(state) => {
                if !state.frame.queue(position) {
                    return;
                }
                self.perf_trace.record_pointer_queued();
                state.serial
            }
            _ => return,
        };
        cx.on_next_frame(window, move |this, _, cx| {
            let position = match &mut this.pointer_interaction {
                PointerInteraction::RubberBand(state) if state.serial == serial => {
                    state.frame.take()
                }
                _ => None,
            };
            if let Some(position) = position {
                this.perf_trace.record_pointer_applied();
                this.update_rubber_band(position, true, cx);
            }
        });
    }

    fn flush_rubber_band_update(&mut self, cx: &mut Context<Self>) {
        let position = match &mut self.pointer_interaction {
            PointerInteraction::RubberBand(state) => state.frame.take(),
            _ => None,
        };
        if let Some(position) = position {
            self.perf_trace.record_pointer_applied();
            self.update_rubber_band(position, false, cx);
        }
    }

    fn rubber_scroll_velocity(viewport: Bounds<Pixels>, position: gpui::Point<Pixels>) -> f32 {
        let top = f32::from(viewport.top());
        let bottom = f32::from(viewport.bottom());
        let y = f32::from(position.y);
        let penetration = if y < top + RUBBER_EDGE_ZONE {
            -((top + RUBBER_EDGE_ZONE - y) / RUBBER_EDGE_ZONE).clamp(0.0, 1.0)
        } else if y > bottom - RUBBER_EDGE_ZONE {
            ((y - (bottom - RUBBER_EDGE_ZONE)) / RUBBER_EDGE_ZONE).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if penetration == 0.0 {
            0.0
        } else {
            penetration.signum() * (3.0 + 11.0 * penetration.abs().powi(2))
        }
    }

    fn schedule_rubber_autoscroll(&mut self, cx: &mut Context<Self>) {
        let serial = match &mut self.pointer_interaction {
            PointerInteraction::RubberBand(state) if !state.autoscroll_scheduled => {
                state.autoscroll_scheduled = true;
                state.serial
            }
            _ => return,
        };
        let timer = cx.background_executor().timer(Duration::from_millis(16));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                let (position, frame_scheduled) = match &mut this.pointer_interaction {
                    PointerInteraction::RubberBand(state) if state.serial == serial => {
                        state.autoscroll_scheduled = false;
                        (
                            state.frame.pending_or(state.current),
                            state.frame.is_scheduled(),
                        )
                    }
                    _ => return,
                };
                let (viewport, _) = this.file_list_metrics();
                let velocity = Self::rubber_scroll_velocity(viewport, position);
                if velocity == 0.0 {
                    return;
                }
                let base_handle = this.file_list_scroll.0.borrow().base_handle.clone();
                let offset = base_handle.offset();
                let max_offset = f32::from(base_handle.max_offset().height);
                let next_y = (f32::from(offset.y) - velocity).clamp(-max_offset, 0.0);
                base_handle.set_offset(point(offset.x, px(next_y)));
                if !frame_scheduled {
                    this.update_rubber_band(position, true, cx);
                }
            });
        })
        .detach();
    }

    fn handle_pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(&self.pointer_interaction, PointerInteraction::RubberBand(_))
            && event.dragging()
        {
            self.perf_trace.record_pointer_received();
            self.queue_rubber_band_update(event.position, window, cx);
        }
    }
}
