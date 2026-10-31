// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Application-owned layout and input routing. No SDL, GPU, projection or text painter.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn contains(self, x: u32, y: u32) -> bool {
        x.checked_sub(self.x).is_some_and(|x| x < self.width)
            && y.checked_sub(self.y).is_some_and(|y| y < self.height)
    }
    pub fn local(self, x: u32, y: u32) -> Option<(u32, u32)> {
        self.contains(x, y).then(|| (x - self.x, y - self.y))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Control {
    Select,
    Move,
    Terrain,
    Clear,
    Undo,
    Redo,
    Accept,
    Cancel,
    Coordinates,
    Nudge([i64; 2]),
    Field(String),
    Palette { grid: String, prototype: String },
    ApplyDraft,
    CancelDraft,
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub bounds: Rect,
    pub text: String,
    pub control: Option<Control>,
    pub enabled: bool,
    pub active: bool,
}

/// All coordinates are physical pixels, including the exact world viewport.
/// Logical sizes are scaled here once; pointer conversion happens at SDL's boundary.
pub struct WorkspaceUi {
    pub size: (u32, u32),
    pub viewport: Rect,
    pub sidebar: Rect,
    pub review: Rect,
    pub widgets: Vec<Widget>,
    logical: (u32, u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Hit {
    Control(Control),
    /// Every panel pixel consumes input, including blank and disabled rows.
    Panel,
    World(u32, u32),
    Outside,
}

impl WorkspaceUi {
    pub fn new(logical: (u32, u32), size: (u32, u32)) -> Self {
        let top = 48.min(logical.1);
        let side = 320.min(logical.0 / 2);
        let review = 180.min(logical.1.saturating_sub(top) / 2);
        let mut ui = Self {
            size,
            viewport: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            sidebar: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            review: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            widgets: Vec::new(),
            logical,
        };
        ui.viewport = ui.scale(Rect {
            x: 0,
            y: top,
            width: logical.0 - side,
            height: logical.1 - top - review,
        });
        ui.sidebar = ui.scale(Rect {
            x: logical.0 - side,
            y: top,
            width: side,
            height: logical.1 - top,
        });
        ui.review = ui.scale(Rect {
            x: 0,
            y: logical.1 - review,
            width: logical.0 - side,
            height: review,
        });
        ui
    }

    fn scale(&self, rect: Rect) -> Rect {
        let scale = |value: u32, axis: usize| -> u32 {
            let (from, to) = if axis == 0 {
                (self.logical.0, self.size.0)
            } else {
                (self.logical.1, self.size.1)
            };
            if from == 0 {
                return 0;
            }
            u32::try_from(u64::from(value) * u64::from(to) / u64::from(from)).unwrap_or(to)
        };
        let x = scale(rect.x, 0);
        let y = scale(rect.y, 1);
        Rect {
            x,
            y,
            width: scale(rect.x.saturating_add(rect.width), 0).saturating_sub(x),
            height: scale(rect.y.saturating_add(rect.height), 1).saturating_sub(y),
        }
    }

    pub fn widget(
        &mut self,
        bounds: Rect,
        text: impl Into<String>,
        control: Option<Control>,
        enabled: bool,
        active: bool,
    ) {
        let bounds = self.scale(bounds);
        if bounds.width > 0
            && bounds.height > 0
            && bounds.x.saturating_add(bounds.width) <= self.size.0
            && bounds.y.saturating_add(bounds.height) <= self.size.1
        {
            self.widgets.push(Widget {
                bounds,
                text: text.into(),
                control,
                enabled,
                active,
            });
        }
    }

    pub fn sidebar_x(&self) -> u32 {
        self.logical.0 - 320.min(self.logical.0 / 2) + 10
    }
    pub fn sidebar_width(&self) -> u32 {
        320.min(self.logical.0 / 2).saturating_sub(20)
    }
    pub fn review_y(&self) -> u32 {
        self.logical.1 - 180.min(self.logical.1.saturating_sub(48) / 2)
    }
    pub fn logical_size(&self) -> (u32, u32) {
        self.logical
    }
    pub fn logical_height(&self) -> u32 {
        self.logical.1
    }

    pub fn hit(&self, x: u32, y: u32) -> Hit {
        if x >= self.size.0 || y >= self.size.1 {
            return Hit::Outside;
        }
        for widget in self.widgets.iter().rev() {
            if widget.bounds.contains(x, y) {
                return match &widget.control {
                    Some(control) if widget.enabled => Hit::Control(control.clone()),
                    _ => Hit::Panel,
                };
            }
        }
        self.viewport
            .local(x, y)
            .map_or(Hit::Panel, |(x, y)| Hit::World(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_and_viewport_offset_convert_once() {
        let ui = WorkspaceUi::new((960, 640), (1920, 1280));
        assert_eq!(
            ui.viewport,
            Rect {
                x: 0,
                y: 96,
                width: 1280,
                height: 824
            }
        );
        assert_eq!(ui.hit(18, 110), Hit::World(18, 14));
        assert_eq!(ui.hit(1280, 110), Hit::Panel);
        assert_eq!(ui.hit(18, 920), Hit::Panel);
        assert_eq!(ui.hit(1920, 110), Hit::Outside);
    }

    #[test]
    fn blank_and_disabled_panel_pixels_never_reach_world() {
        let mut ui = WorkspaceUi::new((960, 640), (960, 640));
        ui.widget(
            Rect {
                x: 650,
                y: 90,
                width: 100,
                height: 30,
            },
            "disabled",
            Some(Control::Accept),
            false,
            false,
        );
        assert_eq!(ui.hit(660, 100), Hit::Panel);
        assert_eq!(ui.hit(800, 500), Hit::Panel);
        ui.widget(
            Rect {
                x: 10,
                y: 8,
                width: 80,
                height: 30,
            },
            "Select",
            Some(Control::Select),
            true,
            true,
        );
        assert_eq!(ui.hit(20, 20), Hit::Control(Control::Select));
    }

    #[test]
    fn resized_nonuniform_dpi_keeps_local_mask_pixels() {
        let ui = WorkspaceUi::new((800, 600), (1200, 1200));
        assert_eq!(
            ui.hit(ui.viewport.x + 5, ui.viewport.y + 9),
            Hit::World(5, 9)
        );
        assert_eq!(ui.hit(ui.viewport.width, ui.viewport.y), Hit::Panel);
        let empty = WorkspaceUi::new((0, 0), (0, 0));
        assert_eq!(empty.hit(0, 0), Hit::Outside);
    }
}
