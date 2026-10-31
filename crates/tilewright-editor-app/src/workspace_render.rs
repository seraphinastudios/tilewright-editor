// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Scene assembly only. Typography, clipping and quad generation belong to the
//! shared renderer. The World scene and its exact picking masks remain untouched.
use crate::workspace_ui::{Rect, WorkspaceUi};
use std::{fmt, sync::Arc};
use tilewright_editor_ui::text_input::{TextInputSnapshot, TextRevision};
use tilewright_render_resources::{
    Error as ResourceError, ResourceBlob, ResourceProvider, ResourceRequest,
};
use tilewright_render_ui::{
    UiDisplayList, UiLimits, UiPaint, UiPrimitive, UiRect, UiTexture, UiViewport, builtin,
    unicode::{TextAffinity, TextCursor, TextSystem, UnicodeLayout, UnicodeTexture},
};
use tilewright_scene::{Digest256, ResourceId, SceneBundle, SceneLimits, SemanticId};

type Result<T> = std::result::Result<T, CompositionError>;

#[derive(Debug)]
pub enum CompositionError {
    TargetMismatch,
    SemanticIdentityExhausted,
    ResourceIdentityExhausted,
    PainterOrderExhausted,
    Ui(tilewright_render_ui::UiError),
    Text(tilewright_render_ui::unicode::TextError),
    Scene(tilewright_scene::Error),
}
impl fmt::Display for CompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(error) => fmt::Display::fmt(error, formatter),
            Self::Ui(error) => fmt::Display::fmt(error, formatter),
            Self::Scene(error) => fmt::Display::fmt(error, formatter),
            other => write!(formatter, "UI composition refused: {other:?}"),
        }
    }
}
impl std::error::Error for CompositionError {}

#[derive(Clone, Debug)]
pub struct EditableGeometry {
    pub revision: TextRevision,
    pub layout: UnicodeLayout,
    pub origin: [f32; 2],
    pub clip: UiRect,
    pub snapshot: TextInputSnapshot,
}
pub struct Presentation {
    pub scene: SceneBundle,
    pub resources: Arc<dyn ResourceProvider>,
    pub unsupported_runs: usize,
    pub editable: Option<EditableGeometry>,
}
struct Resources {
    world: Arc<dyn ResourceProvider>,
    textures: Vec<UnicodeTexture>,
    solid: UiTexture,
}
impl ResourceProvider for Resources {
    fn load(&self, request: ResourceRequest) -> std::result::Result<ResourceBlob, ResourceError> {
        request.validate()?;
        let expected = self
            .textures
            .iter()
            .find(|texture| texture.identity.id == request.id)
            .map(|texture| (texture.identity, texture.bytes.as_ref()))
            .or_else(|| {
                (request.id == self.solid.id).then_some((self.solid, builtin::SOLID_BYTES))
            });
        if let Some((identity, bytes)) = expected {
            if request.revision != identity.revision || request.digest != identity.digest {
                return Err(ResourceError::DigestMismatch);
            }
            if bytes.len() > request.maximum_bytes {
                return Err(ResourceError::LimitExceeded);
            }
            return ResourceBlob::for_request(request, bytes.to_vec());
        }
        self.world.load(request)
    }
}
pub fn text_size(height: u32) -> f32 {
    if height <= 22 { 14.0 } else { 18.0 }
}

/// Elision uses shared shaped measurements and original grapheme boundaries.
#[allow(clippy::cast_precision_loss)]
pub fn fit_text(system: &mut TextSystem, text: &str, width: u32, height: u32) -> String {
    let available = width.saturating_sub(8) as f32;
    let size = text_size(height);
    let Ok(layout) = system.layout(text, size, None) else {
        return "Text unavailable".into();
    };
    if layout.size()[0] <= available {
        return text.into();
    }
    for &end in layout.grapheme_boundaries().iter().rev() {
        let candidate = format!("{}...", &text[..end]);
        if system
            .layout(&candidate, size, None)
            .is_ok_and(|l| l.size()[0] <= available)
        {
            return candidate;
        }
    }
    String::new()
}
#[allow(clippy::cast_precision_loss)]
pub fn wrap_text(system: &mut TextSystem, text: &str, width: u32, height: u32) -> Vec<String> {
    let available = width.saturating_sub(8) as f32;
    let mut remaining = text;
    let mut lines = Vec::new();
    while !remaining.is_empty() && lines.len() < 256 {
        let Ok(layout) = system.layout(remaining, text_size(height), None) else {
            break;
        };
        let end = layout
            .grapheme_boundaries()
            .iter()
            .copied()
            .rev()
            .find(|&end| {
                end > 0
                    && system
                        .layout(&remaining[..end], text_size(height), None)
                        .is_ok_and(|l| l.size()[0] <= available)
            });
        let Some(end) = end else {
            break;
        };
        lines.push(remaining[..end].into());
        remaining = &remaining[end..];
    }
    lines
}
pub fn renderer_cursor(cursor: tilewright_editor_ui::text_input::TextCursor) -> TextCursor {
    TextCursor {
        byte: cursor.byte,
        affinity: match cursor.affinity {
            tilewright_editor_ui::text_input::TextAffinity::Before => TextAffinity::Before,
            tilewright_editor_ui::text_input::TextAffinity::After => TextAffinity::After,
        },
    }
}
pub fn model_cursor(cursor: TextCursor) -> tilewright_editor_ui::text_input::TextCursor {
    tilewright_editor_ui::text_input::TextCursor {
        byte: cursor.byte,
        affinity: match cursor.affinity {
            TextAffinity::Before => tilewright_editor_ui::text_input::TextAffinity::Before,
            TextAffinity::After => tilewright_editor_ui::text_input::TextAffinity::After,
        },
    }
}

#[allow(clippy::cast_precision_loss)]
fn rect(rect: Rect) -> UiRect {
    // SDL dimensions are constrained by SceneLimits before scene construction.
    UiRect {
        x: rect.x as f32,
        y: rect.y as f32,
        width: rect.width as f32,
        height: rect.height as f32,
    }
}

fn paint(next: &mut u32, order: u32, color: [f32; 4]) -> Result<UiPaint> {
    let id = next
        .checked_add(1)
        .ok_or(CompositionError::SemanticIdentityExhausted)?;
    *next = id;
    Ok(UiPaint {
        semantic_id: SemanticId(id),
        painter_order: order,
        color,
    })
}

fn display_list<'a>(
    ui: &'a WorkspaceUi,
    mut identity: u32,
    system: &mut TextSystem,
) -> Result<(Vec<UiPrimitive<'a>>, Option<EditableGeometry>)> {
    let mut editable = None;
    let mut primitives = Vec::new();
    for bounds in [
        Rect {
            x: 0,
            y: 0,
            width: ui.logical_size().0,
            height: ui.logical_viewport.y,
        },
        ui.logical_sidebar,
        ui.logical_review,
    ] {
        let bounds = rect(bounds);
        primitives.push(UiPrimitive::Solid {
            bounds,
            clip: bounds,
            paint: paint(&mut identity, 0, [0.065, 0.08, 0.11, 1.0])?,
            pickable: false,
        });
    }
    for (index, widget) in ui.widgets.iter().enumerate() {
        let bounds = rect(widget.layout);
        let order = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(2))
            .and_then(|index| index.checked_add(1))
            .ok_or(CompositionError::PainterOrderExhausted)?;
        if widget.control.is_some() || widget.active {
            let color = if widget.active {
                [0.12, 0.32, 0.47, 1.0]
            } else if widget.enabled {
                [0.14, 0.18, 0.24, 1.0]
            } else {
                [0.09, 0.11, 0.14, 1.0]
            };
            primitives.push(UiPrimitive::Solid {
                bounds,
                clip: bounds,
                paint: paint(&mut identity, order, color)?,
                pickable: false,
            });
        }
        let color = if widget.enabled {
            [0.91, 0.94, 0.98, 1.0]
        } else {
            [0.50, 0.56, 0.63, 1.0]
        };
        let mut origin = [bounds.x + 4.0, bounds.y + 1.0];
        if let Some(snapshot) = &widget.editable {
            let layout = system
                .layout(&widget.text, text_size(widget.layout.height), None)
                .map_err(CompositionError::Text)?;
            let cursor = renderer_cursor(snapshot.display_selection.focus);
            if let Some(caret) = layout.caret(cursor) {
                origin[0] -= (caret.x - (bounds.width - 12.0).max(0.0)).max(0.0);
            }
            let mut selection =
                layout.selection_rects(renderer_cursor(snapshot.display_selection.anchor), cursor);
            if let Some(composition) = &snapshot.composition {
                for mut underline in layout.selection_rects(
                    TextCursor {
                        byte: composition.display_range.start,
                        affinity: TextAffinity::After,
                    },
                    TextCursor {
                        byte: composition.display_range.end,
                        affinity: TextAffinity::Before,
                    },
                ) {
                    underline.y += underline.height - 2.0;
                    underline.height = 1.0;
                    selection.push(underline);
                }
            }
            if let Some(caret) = layout.caret(cursor) {
                selection.push(caret);
            }
            for mut selected in selection {
                selected.x += origin[0];
                selected.y += origin[1];
                primitives.push(UiPrimitive::Solid {
                    bounds: selected,
                    clip: bounds,
                    paint: paint(&mut identity, order, [0.28, 0.48, 0.66, 1.0])?,
                    pickable: false,
                });
            }
            editable = Some(EditableGeometry {
                revision: snapshot.revision,
                layout,
                origin,
                clip: bounds,
                snapshot: snapshot.clone(),
            });
        }
        primitives.push(UiPrimitive::Text {
            origin,
            clip: bounds,
            paint: paint(&mut identity, order + 1, color)?,
            font: builtin::FONT_ID,
            size: text_size(widget.layout.height),
            text: &widget.text,
        });
    }
    Ok((primitives, editable))
}

pub fn compose(
    system: &mut TextSystem,
    ui: &WorkspaceUi,
    world: &SceneBundle,
    resources: Arc<dyn ResourceProvider>,
) -> Result<Presentation> {
    if (world.viewport().width, world.viewport().height) != ui.size {
        return Err(CompositionError::TargetMismatch);
    }
    let resource = world
        .quads()
        .iter()
        .map(|quad| quad.resource_id.0)
        .max()
        .unwrap_or(0);
    let solid = UiTexture {
        id: ResourceId(
            resource
                .checked_add(1)
                .ok_or(CompositionError::ResourceIdentityExhausted)?,
        ),
        revision: 1,
        digest: Digest256(builtin::SOLID_SHA256),
    };
    let first_resource = resource
        .checked_add(2)
        .ok_or(CompositionError::ResourceIdentityExhausted)?;
    let identity = world
        .quads()
        .iter()
        .map(|quad| quad.semantic_id.0)
        .max()
        .unwrap_or(0);
    let (primitives, editable) = display_list(ui, identity, system)?;
    let logical = ui.logical_size();
    let compiled = system
        .compile(
            UiViewport {
                logical_size: [logical.0, logical.1],
                physical_size: [ui.size.0, ui.size.1],
            },
            UiDisplayList {
                primitives: &primitives,
            },
            solid,
            first_resource,
            UiLimits::default(),
        )
        .map_err(CompositionError::Text)?;
    let scene = SceneBundle::from_quads(
        world.revision(),
        world.clock_millis(),
        world.viewport(),
        world.clear_color(),
        world.quads().iter().copied().chain(compiled.quads),
        SceneLimits::default(),
    )
    .map_err(CompositionError::Scene)?;
    Ok(Presentation {
        scene,
        resources: Arc::new(Resources {
            world: resources,
            textures: compiled.resources,
            solid,
        }),
        unsupported_runs: compiled.unsupported.len(),
        editable,
    })
}
