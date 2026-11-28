// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Drawing the opt-in shared text field (`--shared-string-field`).
//!
//! Every text draft (the inspector's fields of every type, the zone name
//! drafts, the palette filter, the Move coordinates, a new anchor's id and
//! a portal's typed location) is edited by `tilewright-ui`'s field (see
//! `tilewright_editor_ui::field_input`); here its state is laid out in the
//! sidebar row (or, for the filter, the row at the top of the sidebar),
//! compiled by `tilewright-ui-render`'s `UiCompiler` with the editor's own
//! text system lent for each call, and composed into the
//! editor's frame under the editor's own scene limits. Its geometry then
//! serves pointer selection, caret steps and the input method area in place
//! of the editor's own. The field is laid out without the shared crate's
//! submit control: the editor's own Review control and Enter review. It is
//! styled with the editor's own field row's values, so it sits in the same
//! row, the same size, with its text at the same origin and size.
use crate::workspace_render::{self, EditableGeometry, Presentation};
use crate::workspace_ui::{Rect, WorkspaceUi};
use tilewright_editor_ui::field_input::{SharedInput, editor_snapshot};
use tilewright_render_ui::{TextSystem, UiRect};
use tilewright_scene::SceneLimits;
use tilewright_ui_render::{CompiledFrame, Error, Namespace, UiCompiler, UiLimits};

type Result<T> = std::result::Result<T, String>;

/// Host rule 1 of the shared compiler: its atlas (and its solid texture)
/// use IDs the editor can never compute for its own. The editor's own UI
/// textures are the largest World or thumbnail resource ID plus one and
/// two; World resource IDs are catalog positions plus one, bounded by the
/// catalog's size, and the cell reference art is `u64::MAX`. These sit far
/// above every derived ID and clear of that sentinel. A host scene that
/// uses one anyway is refused by `CompiledFrame::compose`.
pub const SHARED_SOLID_RESOURCE: u64 = 0xffff_ffff_ffff_ff00;
pub const SHARED_ATLAS_RESOURCE: u64 = 0xffff_ffff_ffff_ff01;
/// The lowest ID the editor's own textures must stay below.
pub const RESERVED_RESOURCES: u64 = SHARED_SOLID_RESOURCE;
/// The shared field's quads' semantic IDs start here, above the World's
/// and the editor interface's.
pub const SHARED_FIRST_SEMANTIC: u32 = 0xf000_0000;
const _: () =
    assert!(SHARED_ATLAS_RESOURCE != u64::MAX && SHARED_SOLID_RESOURCE != SHARED_ATLAS_RESOURCE);

/// The shared field's pointer geometry, beside the editor's own terms.
#[derive(Clone, Debug)]
pub struct SharedGeometry {
    pub text: tilewright_ui_render::TextGeometry,
}

/// The shared layout's options: the single field without its own submit
/// control, so the composer takes the row's whole width, styled as the
/// editor's own field row (`workspace_render::display_list`): flat, the
/// row's own height and active fill, the editor's text size, inset and
/// colours, and the same scroll. The editor keeps the caret's left edge
/// `FIELD_SCROLL_MARGIN` from the row's right edge, counted from the text
/// inset; the shared field keeps its right edge `padding_right` from it.
pub fn layout_options() -> tilewright_ui::LayoutOptions {
    use workspace_render::{
        ACTIVE_ROW_COLOR, CARET_WIDTH, FIELD_ACCENT, FIELD_ROW_HEIGHT, FIELD_SCROLL_MARGIN,
        TEXT_COLOR, TEXT_INSET,
    };
    let mut options = tilewright_ui::LayoutOptions::default();
    options.submit_control = false;
    let style = &mut options.field_style;
    style.text_size = workspace_render::text_size(FIELD_ROW_HEIGHT);
    #[allow(clippy::cast_precision_loss)]
    let height = FIELD_ROW_HEIGHT as f32;
    style.height = height;
    style.inset = 0.0;
    style.padding_left = TEXT_INSET[0];
    style.padding_top = TEXT_INSET[1];
    style.padding_right = FIELD_SCROLL_MARGIN - TEXT_INSET[0] - CARET_WIDTH;
    style.text_color = TEXT_COLOR;
    style.box_color = Some(ACTIVE_ROW_COLOR);
    style.selection_color = FIELD_ACCENT;
    style.caret_color = Some(FIELD_ACCENT);
    style.preedit_color = Some(FIELD_ACCENT);
    options
}

pub struct SharedField {
    compiler: UiCompiler,
}

impl SharedField {
    pub fn new() -> Result<Self> {
        Ok(Self {
            compiler: UiCompiler::new(
                Namespace {
                    first_semantic: SHARED_FIRST_SEMANTIC,
                    solid_resource: SHARED_SOLID_RESOURCE,
                    atlas_resource: SHARED_ATLAS_RESOURCE,
                },
                UiLimits::default(),
            )
            .map_err(refusal)?,
        })
    }

    /// Lay out and compile the field's current state in `bounds`. Host rule
    /// 2: the one text system the editor draws its own text with is lent
    /// here on every call.
    fn compile(
        &mut self,
        text: &mut TextSystem,
        shared: &SharedInput,
        ui: &WorkspaceUi,
        bounds: Rect,
    ) -> Result<CompiledFrame> {
        let logical = ui.logical_size();
        #[allow(clippy::cast_precision_loss)]
        let viewport = tilewright_ui::Viewport::new(
            logical.0 as f32,
            logical.1 as f32,
            ui.size.0 as f32 / logical.0.max(1) as f32,
        )
        .map_err(|error| format!("shared field: {error}"))?;
        let snapshot = shared.snapshot();
        let layout = tilewright_ui::layout_in_with(
            &snapshot,
            viewport,
            neutral(bounds),
            neutral(ui.logical_sidebar),
            &layout_options(),
        )
        .map_err(|error| format!("shared field: {error}"))?;
        self.compiler
            .compile(text, &snapshot, &layout, [ui.size.0, ui.size.1])
            .map_err(refusal)
    }

    /// Compile the field in the row the sidebar reserved, before the
    /// editor's frame is fitted, so the fit can leave room for it
    /// ([`PreparedField::quads`], [`PreparedField::raster_samples`]).
    /// Without a reserved row (the field is scrolled away) there is nothing
    /// to draw.
    pub fn prepare(
        &mut self,
        text: &mut TextSystem,
        shared: &SharedInput,
        draft: u64,
        ui: &WorkspaceUi,
    ) -> Result<Option<PreparedField>> {
        let Some(bounds) = ui.shared_field else {
            return Ok(None);
        };
        let frame = self.compile(text, shared, ui, bounds)?;
        Ok(Some(PreparedField { frame, draft }))
    }

    /// The current state's geometry, for a caret step: its shared
    /// `TextGeometry` steps with `step_caret`.
    pub fn current(
        &mut self,
        text: &mut TextSystem,
        shared: &SharedInput,
        ui: &WorkspaceUi,
        draft: u64,
    ) -> Result<EditableGeometry> {
        let frame = self.compile(text, shared, ui, bounds(ui))?;
        frame
            .editable()
            .and_then(|geometry| geometry_for(geometry, draft))
            .ok_or_else(|| "shared field: no editable text".into())
    }
}

/// The field compiled for this frame, not yet drawn.
pub struct PreparedField {
    frame: CompiledFrame,
    draft: u64,
}

impl PreparedField {
    /// How many quads drawing the field adds to the frame.
    pub fn quads(&self) -> usize {
        self.frame.quad_count()
    }

    /// How many raster samples drawing the field adds to the frame.
    pub fn raster_samples(&self) -> u64 {
        self.frame.raster_sample_count()
    }

    /// Draw the field over the editor's composed frame and hand its
    /// geometry to the input paths. The composed frame is checked against
    /// the editor's own `limits` as a backstop: the editor's frame was
    /// fitted to leave [`PreparedField::quads`] and
    /// [`PreparedField::raster_samples`] free, so a frame refused here is
    /// one the field alone takes past them. It is not drawn, and editing
    /// pauses with the draft kept.
    pub fn attach(self, presentation: &mut Presentation, limits: SceneLimits) -> Result<()> {
        let composed = self
            .frame
            .compose_with_limits(&presentation.scene, presentation.resources.clone(), limits)
            .map_err(refusal)?;
        presentation.scene = composed.scene;
        presentation.resources = composed.resources;
        presentation.editable = self
            .frame
            .editable()
            .and_then(|geometry| geometry_for(geometry, self.draft));
        Ok(())
    }
}

/// The reserved row, or one of its size at the top left while the row is
/// not laid out: a caret step does not depend on where it is.
fn bounds(ui: &WorkspaceUi) -> Rect {
    let logical = ui.logical_size();
    ui.shared_field.unwrap_or(Rect {
        x: 0,
        y: 0,
        width: 300.min(logical.0),
        height: workspace_render::FIELD_ROW_HEIGHT.min(logical.1),
    })
}

/// The shared geometry in the editor's terms; `None` for text that is
/// not the editable field.
fn geometry_for(
    geometry: &tilewright_ui_render::TextGeometry,
    draft: u64,
) -> Option<EditableGeometry> {
    let snapshot = editor_snapshot(geometry.snapshot.as_ref()?, draft);
    Some(EditableGeometry {
        revision: snapshot.revision,
        layout: geometry.layout.clone(),
        origin: geometry.origin,
        clip: UiRect {
            x: geometry.clip.x,
            y: geometry.clip.y,
            width: geometry.clip.width,
            height: geometry.clip.height,
        },
        snapshot,
        shared: Some(SharedGeometry {
            text: geometry.clone(),
        }),
    })
}

#[allow(clippy::cast_precision_loss)]
fn neutral(rect: Rect) -> tilewright_ui::Rect {
    tilewright_ui::Rect {
        x: rect.x as f32,
        y: rect.y as f32,
        width: rect.width as f32,
        height: rect.height as f32,
    }
}

/// A clean refusal for each compiler error; the frame is not drawn and the
/// editor pauses through its usual presentation refusal.
fn refusal(error: Error) -> String {
    match error {
        Error::TextSystemChanged => {
            "shared field: the lent text system changed; the field is not drawn".into()
        }
        Error::IdentityCollision => {
            "shared field: the frame uses an identity reserved for the shared field".into()
        }
        other => format!("shared field: {other}"),
    }
}
