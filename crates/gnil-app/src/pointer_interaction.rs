use std::{
    ops::RangeInclusive,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gnil_core::{FileEntry, SelectionMerge, SelectionState};
use gpui::{Modifiers, Pixels, Point};

pub(crate) const POINTER_DRAG_THRESHOLD: f32 = 5.0;
pub(crate) const RUBBER_EDGE_ZONE: f32 = 28.0;

#[derive(Debug, Default)]
pub(crate) struct DragVisualState {
    lifted: AtomicBool,
    copy: AtomicBool,
    valid_target: AtomicBool,
}

impl DragVisualState {
    pub(crate) fn set_lifted(&self, lifted: bool) {
        self.lifted.store(lifted, Ordering::Relaxed);
    }

    pub(crate) fn set_copy(&self, copy: bool) {
        self.copy.store(copy, Ordering::Relaxed);
    }

    pub(crate) fn set_valid_target(&self, valid: bool) {
        self.valid_target.store(valid, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn lifted(&self) -> bool {
        self.lifted.load(Ordering::Relaxed)
    }

    #[must_use]
    pub(crate) fn copy(&self) -> bool {
        self.copy.load(Ordering::Relaxed)
    }

    #[must_use]
    pub(crate) fn valid_target(&self) -> bool {
        self.valid_target.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FileDragPayload {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) first_name: String,
    pub(crate) first_icon: &'static str,
    pub(crate) visual: Arc<DragVisualState>,
}

impl FileDragPayload {
    pub(crate) fn new(
        paths: Vec<PathBuf>,
        first_name: String,
        first_icon: &'static str,
    ) -> Self {
        Self {
            paths,
            first_name,
            first_icon,
            visual: Arc::new(DragVisualState::default()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RowPressState {
    pub(crate) origin: Point<Pixels>,
    pub(crate) index: usize,
    pub(crate) baseline: SelectionState,
    pub(crate) modifiers: Modifiers,
    pub(crate) payload: FileDragPayload,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveFileDrag {
    pub(crate) press: RowPressState,
    pub(crate) copy: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct RubberBandState {
    pub(crate) serial: u64,
    pub(crate) origin: Point<Pixels>,
    pub(crate) current: Point<Pixels>,
    pub(crate) origin_content_y: f32,
    pub(crate) current_content_y: f32,
    pub(crate) baseline: SelectionState,
    pub(crate) merge: SelectionMerge,
    pub(crate) crossed_threshold: bool,
    pub(crate) hit_span: Option<RangeInclusive<usize>>,
    pub(crate) autoscroll_scheduled: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum PointerInteraction {
    #[default]
    Idle,
    RowArmed(RowPressState),
    FileDrag(ActiveFileDrag),
    RubberBand(RubberBandState),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DropTarget {
    Directory(PathBuf),
    Trash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropIntent {
    Move,
    Copy,
    Trash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DropInvalidReason {
    Empty,
    Busy,
    SelfTarget,
    SameParent,
    ExternalTrash,
}

#[must_use]
pub(crate) fn merge_from_modifiers(modifiers: Modifiers) -> SelectionMerge {
    if modifiers.shift {
        SelectionMerge::Union
    } else if modifiers.control || modifiers.platform {
        SelectionMerge::Toggle
    } else {
        SelectionMerge::Replace
    }
}

#[must_use]
pub(crate) fn movement_crossed_threshold(origin: Point<Pixels>, current: Point<Pixels>) -> bool {
    (current - origin).magnitude() >= f64::from(POINTER_DRAG_THRESHOLD)
}

#[must_use]
pub(crate) fn band_span(
    origin_content_y: f32,
    current_content_y: f32,
    row_height: f32,
    item_count: usize,
    overlaps_horizontally: bool,
) -> Option<RangeInclusive<usize>> {
    if !overlaps_horizontally || item_count == 0 || row_height <= 0.0 {
        return None;
    }
    let low = origin_content_y.min(current_content_y);
    let high = origin_content_y.max(current_content_y);
    let content_height = row_height * item_count as f32;
    if high < 0.0 || low >= content_height {
        return None;
    }
    let first = (low.max(0.0) / row_height).floor() as usize;
    let last = ((high.min(content_height) - f32::EPSILON).max(0.0) / row_height).floor() as usize;
    Some(first.min(item_count - 1)..=last.min(item_count - 1))
}

#[must_use]
pub(crate) fn endpoint_index(
    span: Option<&RangeInclusive<usize>>,
    origin_content_y: f32,
    current_content_y: f32,
) -> Option<usize> {
    span.map(|span| {
        if current_content_y >= origin_content_y {
            *span.end()
        } else {
            *span.start()
        }
    })
}

#[must_use]
pub(crate) fn rubber_highlighted(
    state: &RubberBandState,
    index: usize,
    entry: &FileEntry,
) -> bool {
    if !state.crossed_threshold {
        return state.baseline.is_highlighted(index, entry);
    }
    let hit = state
        .hit_span
        .as_ref()
        .is_some_and(|span| span.contains(&index));
    let baseline = state.baseline.is_highlighted(index, entry);
    match state.merge {
        SelectionMerge::Replace => hit,
        SelectionMerge::Union => baseline || hit,
        SelectionMerge::Toggle => baseline ^ hit,
    }
}

pub(crate) fn internal_drop_intent(
    payload: &FileDragPayload,
    target: &DropTarget,
    copy: bool,
    operation_running: bool,
) -> Result<DropIntent, DropInvalidReason> {
    if payload.paths.is_empty() {
        return Err(DropInvalidReason::Empty);
    }
    if operation_running {
        return Err(DropInvalidReason::Busy);
    }
    match target {
        DropTarget::Trash => Ok(DropIntent::Trash),
        DropTarget::Directory(destination) => {
            if payload.paths.iter().any(|path| path == destination) {
                return Err(DropInvalidReason::SelfTarget);
            }
            if payload
                .paths
                .iter()
                .any(|path| path.parent() == Some(destination.as_path()))
            {
                return Err(DropInvalidReason::SameParent);
            }
            Ok(if copy {
                DropIntent::Copy
            } else {
                DropIntent::Move
            })
        }
    }
}

pub(crate) fn external_drop_intent(
    paths: &[PathBuf],
    target: &DropTarget,
    operation_running: bool,
) -> Result<DropIntent, DropInvalidReason> {
    if paths.is_empty() {
        return Err(DropInvalidReason::Empty);
    }
    if operation_running {
        return Err(DropInvalidReason::Busy);
    }
    if target == &DropTarget::Trash {
        return Err(DropInvalidReason::ExternalTrash);
    }
    Ok(DropIntent::Copy)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{Modifiers, point, px};

    use super::*;

    fn payload(paths: &[&str]) -> FileDragPayload {
        FileDragPayload::new(
            paths.iter().map(PathBuf::from).collect(),
            "notes.txt".into(),
            "icons/file-generic.svg",
        )
    }

    #[test]
    fn threshold_is_five_pixels() {
        assert!(!movement_crossed_threshold(
            point(px(0.0), px(0.0)),
            point(px(3.0), px(3.0))
        ));
        assert!(movement_crossed_threshold(
            point(px(0.0), px(0.0)),
            point(px(3.0), px(4.0))
        ));
    }

    #[test]
    fn band_span_is_clamped_without_scanning_the_list() {
        assert_eq!(band_span(365.0, 75.0, 36.0, 100_000, true), Some(2..=10));
        assert_eq!(band_span(-40.0, 40.0, 36.0, 5, true), Some(0..=1));
        assert_eq!(band_span(10.0, 40.0, 36.0, 5, false), None);
    }

    #[test]
    fn shift_wins_over_control_for_rubber_band() {
        assert_eq!(
            merge_from_modifiers(Modifiers::default()),
            SelectionMerge::Replace
        );
        assert_eq!(
            merge_from_modifiers(Modifiers {
                control: true,
                ..Modifiers::default()
            }),
            SelectionMerge::Toggle
        );
        assert_eq!(
            merge_from_modifiers(Modifiers {
                control: true,
                shift: true,
                ..Modifiers::default()
            }),
            SelectionMerge::Union
        );
    }

    #[test]
    fn drop_policy_blocks_parent_noop_and_external_trash() {
        let payload = payload(&["/work/notes.txt", "/other/image.png"]);
        assert_eq!(
            internal_drop_intent(
                &payload,
                &DropTarget::Directory(PathBuf::from("/work")),
                false,
                false
            ),
            Err(DropInvalidReason::SameParent)
        );
        assert_eq!(
            internal_drop_intent(
                &payload,
                &DropTarget::Directory(PathBuf::from("/work/notes.txt")),
                false,
                false
            ),
            Err(DropInvalidReason::SelfTarget)
        );
        assert_eq!(
            internal_drop_intent(
                &payload,
                &DropTarget::Directory(PathBuf::from("/archive")),
                true,
                false
            ),
            Ok(DropIntent::Copy)
        );
        assert_eq!(
            external_drop_intent(&payload.paths, &DropTarget::Trash, false),
            Err(DropInvalidReason::ExternalTrash)
        );
    }

    #[test]
    fn idle_is_the_default_interaction() {
        assert!(matches!(PointerInteraction::default(), PointerInteraction::Idle));
    }
}
