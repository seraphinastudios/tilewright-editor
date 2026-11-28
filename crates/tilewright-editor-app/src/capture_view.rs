// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Copy area in Place mode (docs/OFFLINE-PROTOTYPE.md#copying-an-area):
//! C arms it, a drag over the map spans a rectangle of view tiles, and the
//! release copies the objects in it into the session's clipboard
//! ([`tilewright_editor_ui::capture`]). The copy is then the chosen stamp:
//! R and M turn and mirror it, the hover previews it and a click prepares
//! one `place_objects` change, all through the catalog stamps' own code.
//! Nothing here writes the World or records anything: a copy lives only in
//! the session's tool state.
use super::{Application, ROW_GAP, Result, STAMP_BLOCKED, TEXT_ROW_HEIGHT, objects_words};
use crate::workspace_ui::{Control, Rect, WorkspaceUi};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::SessionPhase;
use tilewright_editor_ui::capture::{CaptureGesture, Contents};
use tilewright_editor_ui::tools::ToolMode;

/// The status line while Copy area is armed.
pub(super) const CAPTURE_ARMED: &str =
    "Copy area: press on a tile and drag across the objects to copy; Esc stops.";
/// The refusal of a press that would start a rectangle while a change is
/// pending.
pub(super) const CAPTURE_PENDING: &str =
    "Accept or cancel the pending change before copying an area.";
/// The refusal of a press off the view's tiles.
pub(super) const CAPTURE_OFF_VIEW: &str =
    "Copy area starts on a tile of the view: no tile under the pointer.";
/// The pinned row's words for a copy, after its count.
pub(super) const COPIED: &str = "Copied";

impl Application {
    /// Place mode's Copy area and Drop copy buttons, under the stamp's turn
    /// row. Laid out only while Copy area is armed or a copy is held, so a
    /// session that never copies looks exactly as before. Returns the next
    /// free row.
    pub(super) fn capture_row(&self, ui: &mut WorkspaceUi, y: u32) -> u32 {
        let armed = self.tools.capture_gesture().is_some();
        let held = self.tools.copy_choice().is_some();
        if !armed && !held {
            return y;
        }
        let blocked = self.session.status().phase == SessionPhase::Blocked;
        let x = ui.sidebar_x();
        let half = ui.sidebar_width().saturating_sub(ROW_GAP) / 2;
        ui.widget(
            Rect {
                x,
                y,
                width: half,
                height: TEXT_ROW_HEIGHT,
            },
            "Copy area  C",
            Some(Control::CopyArea),
            !blocked,
            armed,
        );
        ui.widget(
            Rect {
                x: x + half + ROW_GAP,
                y,
                width: half,
                height: TEXT_ROW_HEIGHT,
            },
            "Drop copy  Esc",
            Some(Control::DropCopy),
            held,
            false,
        );
        y + TEXT_ROW_HEIGHT + ROW_GAP
    }

    /// The pinned row's text for a held copy: "Copied 4 objects".
    pub(super) fn copy_label(&self) -> Option<String> {
        let copy = self.tools.copy_choice()?;
        Some(format!("{COPIED} {}", objects_words(copy.objects().len())))
    }

    /// Copy area and Drop copy, by key or button. Copy area is inert in a
    /// blocked session, like the stamp's turn and mirror.
    pub(super) fn capture_control(&mut self, control: &Control) -> Result<()> {
        match control {
            Control::CopyArea => {
                if self.session.status().phase == SessionPhase::Blocked {
                    return Err(STAMP_BLOCKED.into());
                }
                self.tools
                    .toggle_capture()
                    .map_err(|error| error.to_string())?;
            }
            _ => {
                self.tools.drop_copy();
            }
        }
        self.forget_hover();
        Ok(())
    }

    /// Esc ends Copy area, armed or dragged, copying nothing: after an
    /// open portal step and before the palette filter. Whether it did.
    pub(super) fn escape_capture(&mut self) -> bool {
        if !self.tools.stop_capture() {
            return false;
        }
        self.forget_hover();
        self.presented_generation = None;
        true
    }

    /// Whether Esc drops the held copy: only with nothing pending. A
    /// pending change is cancelled first, so the copy stays for another
    /// stamp.
    pub(super) fn copy_droppable(&self) -> bool {
        self.session.history().pending().is_none() && self.tools.copy_choice().is_some()
    }

    /// Whether a press on the map starts or ends Copy area's rectangle.
    pub(super) fn capturing(&self) -> bool {
        self.tools.mode() == ToolMode::Place && self.tools.capture_gesture().is_some()
    }

    /// Whether Copy area's rectangle is being dragged.
    pub(super) fn capture_dragging(&self) -> bool {
        matches!(
            self.tools.capture_gesture(),
            Some(CaptureGesture::Dragging { .. })
        )
    }

    /// A press on physical world pixel `x, y` while Copy area is armed: the
    /// rectangle starts on the view tile there, picked through the exact
    /// cell mask as a placement is. Nothing can be copied while a change is
    /// pending or editing is paused, so the copy is always of the committed
    /// World as shown.
    pub(super) fn begin_capture(&mut self, x: u32, y: u32) -> Result<()> {
        self.refuse_while_paused()?;
        if !self.session.can_prepare() {
            return Err(CAPTURE_PENDING.into());
        }
        let cell = self
            .prepared
            .pick(self.view.frame_revision, x, y, true)
            .map_err(|error| error.to_string())?
            .ok_or(CAPTURE_OFF_VIEW)?;
        let tile = self.view_tile(&cell).ok_or(CAPTURE_OFF_VIEW)?;
        self.tools
            .begin_capture(tile)
            .map_err(|error| error.to_string())?;
        // The rectangle is drawn on the next frame, and its status line
        // replaces an earlier refusal's notice.
        self.presented_generation = None;
        self.notice = None;
        Ok(())
    }

    /// A drag sample at physical world pixel `x, y`: the rectangle's far
    /// corner follows the pointer over view tiles; off the map it stays on
    /// the last one, so the rectangle is always inside the view.
    pub(super) fn capture_sample(&mut self, x: u32, y: u32) {
        let before = self.tools.capture_gesture();
        if let Some(tile) = self
            .hover_mask_pick(x, y)
            .and_then(|cell| self.view_tile(&cell))
        {
            self.tools.extend_capture(tile);
        }
        // A moved corner is drawn on the next frame.
        if self.tools.capture_gesture() != before {
            self.presented_generation = None;
        }
    }

    /// Release: copy the rectangle's objects, or say why nothing was
    /// copied (the earlier choice stays then, and Copy area stays armed).
    pub(super) fn finish_capture(&mut self) -> Result<()> {
        let world = self.session.history().document().world();
        let outcome = self.tools.finish_capture(world, self.session.catalog());
        self.forget_hover();
        self.presented_generation = None;
        match outcome {
            Some(Err(refusal)) => Err(refusal.to_string()),
            None | Some(Ok(_)) => Ok(()),
        }
    }

    /// The plane tile of `cell` when it is a tile of the active view.
    fn view_tile(&self, cell: &SelectionKey) -> Option<[i64; 2]> {
        let SelectionKey::Cell {
            plane, layer, x, y, ..
        } = cell
        else {
            return None;
        };
        self.session
            .catalog()
            .tile_in_active_view(plane, *layer, *x, *y)
            .then_some([*x, *y])
    }

    /// What the status line says while Copy area is armed or dragged: the
    /// rectangle, how many objects it holds and whether release copies
    /// them, so a refusal is said before the release.
    pub(super) fn capture_status(&self) -> Option<String> {
        if !self.capturing() {
            return None;
        }
        let catalog = self.session.catalog();
        let Some(area) = self.tools.capture_area(catalog) else {
            return Some(CAPTURE_ARMED.into());
        };
        let contents = Contents::of(self.session.history().document().world(), catalog, area);
        let what = match contents.refusal() {
            None => format!(
                "{}; release to copy.",
                objects_words(contents.objects.len())
            ),
            Some(refusal) => format!("{}; release copies nothing.", refusal.short()),
        };
        let [width, height] = area.size();
        Some(format!(
            "Copy ({}, {}) to ({}, {}), {width} x {height} tiles: {what}{}",
            area.from[0],
            area.from[1],
            area.to[0],
            area.to[1],
            if self.preview_omitted {
                " Too many cells to preview."
            } else if self.prepared.cell_slots().is_none() {
                " Preview unavailable for this view."
            } else {
                ""
            }
        ))
    }

    /// The tiles Copy area's rectangle covers while it is dragged.
    pub(super) fn capture_tiles(&self) -> Vec<[i64; 2]> {
        if !self.capturing() || self.prepared.frame_revision != self.view.frame_revision {
            return Vec::new();
        }
        self.tools
            .capture_area(self.session.catalog())
            .map(|area| area.tiles().collect())
            .unwrap_or_default()
    }
}

/// The rectangle being dragged, marked like a stamp's preview: every tile
/// of it in the brush preview's white, through the same cell copies.
pub(super) fn capture_overlay(app: &Application) -> Vec<tilewright_scene::Quad> {
    let Some(area) = app
        .capturing()
        .then(|| app.tools.capture_area(app.session.catalog()))
        .flatten()
    else {
        return Vec::new();
    };
    if app.prepared.frame_revision != app.view.frame_revision {
        return Vec::new();
    }
    crate::workspace_render::tile_overlay(&app.prepared, |at| {
        area.contains(at)
            .then_some(crate::workspace_render::STROKE_PREVIEW_TINT)
    })
}
