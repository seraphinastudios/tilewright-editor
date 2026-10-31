// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Scene assembly only. Typography, clipping and quad generation belong to the
//! shared renderer. The World scene and its exact picking masks remain untouched.
use crate::workspace_ui::{Rect, WorkspaceUi};
use std::{fmt, sync::Arc};
use tilewright_render_resources::{
    Error as ResourceError, ResourceBlob, ResourceProvider, ResourceRequest,
};
use tilewright_render_ui::{
    BitmapFontSet, UiDisplayList, UiLimits, UiPaint, UiPrimitive, UiRect, UiTexture, UiViewport,
    builtin, compile_ui,
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
    Scene(tilewright_scene::Error),
}
impl fmt::Display for CompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => error.fmt(formatter),
            Self::Scene(error) => error.fmt(formatter),
            other => write!(formatter, "UI composition refused: {other:?}"),
        }
    }
}
impl std::error::Error for CompositionError {}

pub struct Presentation {
    pub scene: SceneBundle,
    pub resources: Arc<dyn ResourceProvider>,
    pub unsupported_runs: usize,
}

struct Resources {
    world: Arc<dyn ResourceProvider>,
    atlas: UiTexture,
    solid: UiTexture,
}
impl ResourceProvider for Resources {
    fn load(&self, request: ResourceRequest) -> std::result::Result<ResourceBlob, ResourceError> {
        request.validate()?;
        let expected = if request.id == self.atlas.id {
            Some((self.atlas, builtin::ATLAS_BYTES))
        } else if request.id == self.solid.id {
            Some((self.solid, builtin::SOLID_BYTES))
        } else {
            None
        };
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

fn font_texture(id: u64) -> UiTexture {
    UiTexture {
        id: ResourceId(id),
        revision: 1,
        digest: Digest256(builtin::ATLAS_SHA256),
    }
}

/// Test the complete original input; display truncation never authorizes a
/// visually lossy edit. The font's shared classifier owns shaping limitations.
pub fn supports_text(text: &str) -> bool {
    builtin::builtin_font(font_texture(1)).is_ok_and(|font| font.supports_text(text))
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

fn display_list(ui: &WorkspaceUi, mut identity: u32) -> Result<Vec<UiPrimitive<'_>>> {
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
        primitives.push(UiPrimitive::Text {
            origin: [bounds.x + 4.0, bounds.y + 1.0],
            clip: bounds,
            paint: paint(&mut identity, order + 1, color)?,
            font: builtin::FONT_ID,
            size: if widget.layout.height <= 22 {
                14.0
            } else {
                18.0
            },
            text: &widget.text,
        });
    }
    Ok(primitives)
}

pub fn compose(
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
    let atlas = font_texture(
        resource
            .checked_add(1)
            .ok_or(CompositionError::ResourceIdentityExhausted)?,
    );
    let solid = UiTexture {
        id: ResourceId(
            resource
                .checked_add(2)
                .ok_or(CompositionError::ResourceIdentityExhausted)?,
        ),
        revision: 1,
        digest: Digest256(builtin::SOLID_SHA256),
    };
    let font = builtin::builtin_font(atlas).map_err(CompositionError::Ui)?;
    let font_array = [font];
    let fonts = BitmapFontSet::new(&font_array).map_err(CompositionError::Ui)?;
    let identity = world
        .quads()
        .iter()
        .map(|quad| quad.semantic_id.0)
        .max()
        .unwrap_or(0);
    let primitives = display_list(ui, identity)?;
    let logical = ui.logical_size();
    let compiled = compile_ui(
        UiViewport {
            logical_size: [logical.0, logical.1],
            physical_size: [ui.size.0, ui.size.1],
        },
        UiDisplayList {
            primitives: &primitives,
        },
        &fonts,
        solid,
        UiLimits::default(),
    )
    .map_err(CompositionError::Ui)?;
    tilewright_render_ui::validate_ui_composition(world.quads(), &compiled)
        .map_err(CompositionError::Ui)?;
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
            atlas,
            solid,
        }),
        unsupported_runs: compiled.unsupported.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Missing;
    impl ResourceProvider for Missing {
        fn load(&self, _: ResourceRequest) -> std::result::Result<ResourceBlob, ResourceError> {
            Err(ResourceError::NotFound)
        }
    }
    #[test]
    fn immutable_ui_provider_checks_complete_identity_and_request_budget() {
        let resources = Resources {
            world: Arc::new(Missing),
            atlas: font_texture(100),
            solid: UiTexture {
                id: ResourceId(101),
                revision: 1,
                digest: Digest256(builtin::SOLID_SHA256),
            },
        };
        let request = ResourceRequest {
            id: resources.atlas.id,
            revision: 1,
            digest: resources.atlas.digest,
            maximum_bytes: tilewright_render_resources::DEFAULT_MAXIMUM_RESOURCE_BYTES,
        };
        assert_eq!(
            resources.load(request).unwrap().bytes(),
            builtin::ATLAS_BYTES
        );
        assert_eq!(
            resources.load(ResourceRequest {
                revision: 2,
                ..request
            }),
            Err(ResourceError::DigestMismatch)
        );
        assert_eq!(
            resources.load(ResourceRequest {
                digest: resources.solid.digest,
                ..request
            }),
            Err(ResourceError::DigestMismatch)
        );
        assert_eq!(
            resources.load(ResourceRequest {
                maximum_bytes: 1,
                ..request
            }),
            Err(ResourceError::LimitExceeded)
        );
        assert_eq!(
            resources.load(ResourceRequest {
                id: ResourceId(102),
                ..request
            }),
            Err(ResourceError::NotFound)
        );
    }
}
