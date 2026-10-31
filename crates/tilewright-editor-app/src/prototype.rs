// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Application composition for the bounded provisioned Linux prototype.
use crate::workspace_ui::{Control, Hit, Rect, WorkspaceUi};
use sdl3::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tilewright_editor_commands::History;
use tilewright_editor_document::{WorldDocument, terminal_field, terminal_safe};
use tilewright_editor_preview::{PreparedPreview, PreviewLimits, SelectionKey, ViewSpec, loader};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_ui::inspector::{self, SelectionSummary};
use tilewright_editor_ui::tools::{self, ToolMode, ToolState};
use tilewright_editor_ui::{Action, Session};
use tilewright_render_api::{
    BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_render_wgpu::WgpuRenderer;
use tilewright_scene::SemanticId;
use tilewright_source::SourceRoot;

const HELP: &str = "Select / Move / Terrain | Enter accept | Esc cancel | Z undo | Y redo | WASD pan | +/- zoom | Q quit";

type Result<T> = std::result::Result<T, String>;
type UiRows = Vec<(String, Option<Control>, bool, bool)>;

struct Application {
    session: Session,
    recovery: DurableRecovery,
    evidence: Evidence,
    view: ViewSpec,
    prepared: PreparedPreview,
    tools: ToolState,
    ui: WorkspaceUi,
    notice: Option<String>,
    sidebar_scroll: usize,
    review_scroll: usize,
    blocked: Option<String>,
}

/// This command is intentionally a deployment seam, not arbitrary recovery-path
/// selection. The trusted provision helper passes fresh separate retained Linux
/// directory descriptors, whose identity/separation receipt it retains.
pub fn run(arguments: &[String]) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(
            "checked prototype editing requires Linux; other platforms remain read-only".into(),
        );
    }
    let world = descriptor(arguments, "--world-fd")?;
    let recovery = descriptor(arguments, "--recovery-fd")?;
    let evidence = descriptor(arguments, "--evidence-fd")?;
    if world == recovery || world == evidence || recovery == evidence {
        return Err("provisioned descriptor numbers must be distinct".into());
    }
    let reference = arguments.iter().any(|argument| argument == "--reference");
    let demo = arguments.iter().any(|argument| argument == "--demo-script");
    let document = WorldDocument::open_prototype(&world).map_err(|error| error.to_string())?;
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let view = catalog.default_view(document.world(), 960, 640, 1);
    let prepared = tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let session = Session::new(History::new(document), catalog, resources);
    let recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&recovery),
        RecoveryLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let mut app = Application {
        session,
        recovery,
        evidence: Evidence::new(SourceRoot::new(&evidence))?,
        view,
        prepared,
        tools: ToolState::default(),
        ui: WorkspaceUi::new((960, 640), (960, 640)),
        notice: None,
        sidebar_scroll: 0,
        review_scroll: 0,
        blocked: None,
    };
    println!(
        "{}\n{}",
        HELP,
        terminal_field(app.session.catalog().presentation())
    );
    app.report();
    if reference {
        if !demo {
            return Err(
                "reference mode requires --demo-script; it is evidence, not native acceptance"
                    .into(),
            );
        }
        app.demo_all()?;
        return Ok(());
    }
    native(app, demo)
}

fn descriptor(arguments: &[String], name: &str) -> Result<PathBuf> {
    let index = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing {name}; use tools/provision-prototype.py"))?;
    let value: u32 = arguments
        .get(index + 1)
        .ok_or_else(|| format!("missing {name} value"))?
        .parse()
        .map_err(|_| format!("invalid {name}"))?;
    if value < 3 {
        return Err("provisioned descriptors must not be standard streams".into());
    }
    Ok(PathBuf::from(format!("/proc/self/fd/{value}")))
}

impl Application {
    fn rebuild(&mut self) -> Result<()> {
        let result = self.rebuild_inner();
        if let Err(reason) = &result {
            self.blocked = Some(reason.clone());
        }
        result
    }

    fn rebuild_inner(&mut self) -> Result<()> {
        self.tools.cancel();
        self.session
            .invalidate_frame()
            .map_err(|error| error.to_string())?;
        self.view.frame_revision = self.session.generation();
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        self.prepared = tilewright_editor_preview::prepare(
            self.session.world(),
            self.session.catalog(),
            self.session.resources(),
            &self.view,
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn action(&mut self, action: Action) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        let result = self
            .session
            .dispatch(action, &mut self.recovery)
            .map_err(|error| error.to_string());
        if let Some(error) = self.recovery.last_error() {
            self.session.set_recovery_descriptor(error.to_string());
        } else if let Some(record) = self.recovery.records().last() {
            self.session.set_recovery_descriptor(format!(
                "{} sha256={}",
                record.name,
                tilewright_editor_document::revision_hex(&record.sha256)
            ));
        }
        // Failed commands may still invalidate pending state (for example an
        // empty Undo). Always rebuild from Session's actual current World.
        let refreshed = self.rebuild();
        self.report();
        match result {
            Err(original) => Err(original),
            Ok(()) => refreshed,
        }
    }

    fn report(&self) {
        for diagnostic in &self.prepared.diagnostics {
            println!(
                "diagnostic {}: {}",
                terminal_field(diagnostic.code),
                terminal_field(&diagnostic.message)
            );
        }
        println!(
            "state={} selection={:?} history={:?}",
            terminal_field(&self.session.status().to_string()),
            self.session.selection(),
            self.session.history().depths()
        );
        if let Some(pending) = self.session.history().pending() {
            let mut remaining = 128 << 10;
            for file in pending.changed() {
                let text = format!(
                    "--- {}\n{}\n+++ {}\n{}\n",
                    file.path,
                    file.before.as_deref().unwrap_or("<absent>"),
                    file.path,
                    file.after.as_deref().unwrap_or("<absent>")
                );
                if text.len() > remaining {
                    println!("diff display truncated; plan retained");
                    break;
                }
                print!("{}", terminal_safe(&text));
                remaining -= text.len();
            }
        }
    }

    fn click(&mut self, x: u32, y: u32) -> Result<()> {
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        let key = self
            .prepared
            .pick(
                self.view.frame_revision,
                x,
                y,
                self.tools.mode() == ToolMode::Terrain,
            )
            .map_err(|error| error.to_string())?
            .ok_or("no selectable semantic hit at this pixel")?;
        self.action(Action::Select {
            generation: self.view.frame_revision,
            key,
        })
    }

    fn nudge(&mut self, delta: [i64; 2]) -> Result<()> {
        if !self.session.can_prepare() {
            return Err("accept or cancel the pending edit first".into());
        }
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an editable object first".into());
        };
        let location = self
            .session
            .world()
            .object_location(&uid)
            .ok_or("selection no longer exists")?;
        let at = self
            .session
            .world()
            .object(&uid)
            .and_then(|object| object.at)
            .ok_or("selection is not placed")?;
        let x = location
            .chunk
            .1
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[0])))
            .and_then(|value| value.checked_add(delta[0]))
            .ok_or("coordinate overflow")?;
        let y = location
            .chunk
            .2
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[1])))
            .and_then(|value| value.checked_add(delta[1]))
            .ok_or("coordinate overflow")?;
        self.action(Action::Move {
            uid,
            x,
            y,
            expected: self.session.history().document().revision(),
        })
    }

    fn field(&mut self) -> Result<()> {
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an object with a field capability first".into());
        };
        let field = self
            .session
            .catalog()
            .editing()
            .fields
            .iter()
            .find(|(object, _)| object == &uid)
            .map(|(_, field)| field.clone())
            .ok_or("selected object has no field capability")?;
        self.control(Control::Field(field))?;
        while self.tools.draft_text().is_some_and(|text| !text.is_empty()) {
            self.tools.backspace();
        }
        self.tools
            .input("Edited offline")
            .map_err(|error| error.to_string())?;
        self.control(Control::ApplyDraft)
    }

    fn paint(&mut self) -> Result<()> {
        let key = self
            .session
            .selection()
            .ok_or("select a cell in terrain mode first")?;
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        let action = self
            .tools
            .paint_action(
                &inspected,
                key,
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    fn refresh_ui(&mut self) {
        let mut ui = WorkspaceUi::new(self.ui.logical_size(), self.ui.size);
        self.toolbar(&mut ui);
        self.sidebar(&mut ui);
        self.review(&mut ui);
        self.ui = ui;
    }

    fn toolbar(&self, ui: &mut WorkspaceUi) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        for (x, width, label, control, enabled, active) in [
            (
                8,
                72,
                "Select",
                Control::Select,
                true,
                mode == ToolMode::Select,
            ),
            (86, 72, "Move", Control::Move, true, mode == ToolMode::Move),
            (
                164,
                76,
                "Terrain",
                Control::Terrain,
                true,
                mode == ToolMode::Terrain,
            ),
            (
                250,
                68,
                "Clear",
                Control::Clear,
                self.session.selection().is_some(),
                false,
            ),
            (
                328,
                62,
                "Undo",
                Control::Undo,
                ready && self.session.history().depths().0 > 0,
                false,
            ),
            (
                396,
                62,
                "Redo",
                Control::Redo,
                ready && self.session.history().depths().1 > 0,
                false,
            ),
        ] {
            ui.widget(
                Rect {
                    x,
                    y: 8,
                    width,
                    height: 32,
                },
                label,
                Some(control),
                enabled,
                active,
            );
        }
    }

    fn inspector_rows(&self) -> UiRows {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        let mut rows = Vec::new();
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        rows.push(("INSPECTOR  /  scroll for more".into(), None, true, false));
        match inspected.selection {
            Some(SelectionSummary::Object {
                uid,
                prototype,
                plane,
                layer,
                at,
                movable,
            }) => {
                self.object_rows(
                    &mut rows,
                    &inspected,
                    SelectionSummary::Object {
                        uid,
                        prototype,
                        plane,
                        layer,
                        at,
                        movable,
                    },
                );
            }
            Some(SelectionSummary::Cell {
                plane,
                layer,
                grid,
                x,
                y,
                prototype,
            }) => {
                rows.push((format!("Cell {x},{y} / {grid}"), None, true, false));
                rows.push((format!("{plane} / layer {layer}"), None, true, false));
                rows.push((
                    format!("Value: {}", prototype.unwrap_or("<empty>")),
                    None,
                    true,
                    false,
                ));
            }
            None => rows.push((
                inspected
                    .reason
                    .unwrap_or("Select an object or terrain cell")
                    .into(),
                None,
                true,
                false,
            )),
        }
        if mode == ToolMode::Terrain {
            rows.push((
                "TERRAIN PALETTE  /  explicit values".into(),
                None,
                true,
                false,
            ));
            for choice in tools::terrain_palette(self.session.catalog()) {
                let active = self.tools.terrain_choice() == Some(&choice);
                rows.push((
                    format!("{} / {}", choice.grid, choice.prototype),
                    Some(Control::Palette {
                        grid: choice.grid,
                        prototype: choice.prototype,
                    }),
                    ready,
                    active,
                ));
            }
        }
        rows
    }

    fn object_rows(
        &self,
        rows: &mut UiRows,
        inspected: &inspector::Inspector<'_>,
        summary: SelectionSummary<'_>,
    ) {
        let SelectionSummary::Object {
            uid,
            prototype,
            plane,
            layer,
            at,
            movable,
        } = summary
        else {
            return;
        };
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        rows.push((format!("Object {uid}  [{prototype}]"), None, true, false));
        rows.push((
            format!("{plane} / layer {layer} / {at:?}"),
            None,
            true,
            false,
        ));
        rows.push((
            "Edit coordinates: x,y".into(),
            Some(Control::Coordinates),
            ready && movable,
            false,
        ));
        if mode == ToolMode::Move {
            for (label, delta) in [
                ("Move left", [-1, 0]),
                ("Move right", [1, 0]),
                ("Move up", [0, -1]),
                ("Move down", [0, 1]),
            ] {
                rows.push((
                    label.into(),
                    Some(Control::Nudge(delta)),
                    ready && movable,
                    false,
                ));
            }
        }
        for row in &inspected.rows {
            let value = row.current.map_or_else(
                || "<absent>".into(),
                |value| match value {
                    tilewright_source::FieldValue::String(text) => inspector::display_text(text),
                    _ => inspector::display_text(&inspector::scalar_text(value)),
                },
            );
            rows.push((
                format!("{} = {}", row.name, value),
                Some(Control::Field(row.name.into())),
                ready && row.editable,
                false,
            ));
            let detail = row
                .reason
                .map_or_else(|| format!("{:?}", row.spec.field_type), str::to_owned);
            rows.push((detail, None, row.editable, false));
            let choices = tools::field_choices(inspected, row.name);
            if !choices.is_empty() {
                rows.push((
                    format!("Choices: {}", choices.join(", ")),
                    None,
                    true,
                    false,
                ));
            }
        }
        if inspected.truncated {
            rows.push((
                "More schema rows omitted by input budget".into(),
                None,
                false,
                false,
            ));
        }
    }

    fn sidebar(&mut self, ui: &mut WorkspaceUi) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        let rows = self.inspector_rows();
        let mut y = 56;
        if let Some(text) = self.tools.draft_text() {
            ui.widget(
                Rect {
                    x,
                    y,
                    width,
                    height: 30,
                },
                format!("> {text}|  [Ctrl+A clears]"),
                None,
                true,
                true,
            );
            y += 34;
            ui.widget(
                Rect {
                    x,
                    y,
                    width: width / 2 - 3,
                    height: 28,
                },
                "Review edit",
                Some(Control::ApplyDraft),
                ready,
                false,
            );
            ui.widget(
                Rect {
                    x: x + width / 2 + 3,
                    y,
                    width: width / 2 - 3,
                    height: 28,
                },
                "Discard input",
                Some(Control::CancelDraft),
                true,
                false,
            );
            y += 36;
        }
        let visible = usize::try_from(ui.logical_height().saturating_sub(y) / 28).unwrap_or(0);
        self.sidebar_scroll = self.sidebar_scroll.min(rows.len().saturating_sub(visible));
        for (text, control, enabled, active) in
            rows.into_iter().skip(self.sidebar_scroll).take(visible)
        {
            ui.widget(
                Rect {
                    x,
                    y,
                    width,
                    height: 26,
                },
                text,
                control,
                enabled,
                active,
            );
            y += 28;
        }
    }

    fn review(&mut self, ui: &mut WorkspaceUi) {
        let logical = ui.logical_size();
        let pending = self.session.history().pending().is_some();
        let y = ui.review_y();
        ui.widget(
            Rect {
                x: 10,
                y: y + 6,
                width: 100,
                height: 28,
            },
            "Accept",
            Some(Control::Accept),
            pending && self.blocked.is_none(),
            false,
        );
        ui.widget(
            Rect {
                x: 118,
                y: y + 6,
                width: 100,
                height: 28,
            },
            "Cancel",
            Some(Control::Cancel),
            pending && self.blocked.is_none(),
            false,
        );
        let status = self
            .blocked
            .as_deref()
            .or(self.notice.as_deref())
            .map_or_else(|| self.session.status().to_string(), str::to_owned);
        ui.widget(
            Rect {
                x: 10,
                y: y + 38,
                width: logical.0.saturating_sub(320.min(logical.0 / 2) + 20),
                height: 22,
            },
            status,
            None,
            true,
            false,
        );
        let diff = self.diff_lines();
        let lines = usize::try_from(logical.1.saturating_sub(y + 64) / 20).unwrap_or(0);
        self.review_scroll = self.review_scroll.min(diff.len().saturating_sub(lines));
        for (index, line) in diff
            .into_iter()
            .skip(self.review_scroll)
            .take(lines)
            .enumerate()
        {
            ui.widget(
                Rect {
                    x: 10,
                    y: y + 64 + u32::try_from(index).unwrap_or(0) * 20,
                    width: logical.0.saturating_sub(320.min(logical.0 / 2) + 20),
                    height: 20,
                },
                line,
                None,
                true,
                false,
            );
        }
    }

    fn diff_lines(&self) -> Vec<String> {
        let mut diff = Vec::new();
        if let Some(plan) = self.session.history().pending() {
            for file in plan.changed().take(32) {
                let before = file.before.as_deref().unwrap_or("<absent>");
                let after = file.after.as_deref().unwrap_or("<absent>");
                let first_change = before
                    .lines()
                    .zip(after.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| before.lines().count().min(after.lines().count()));
                let start = first_change.saturating_sub(2);
                diff.push(format!(
                    "--- {} / source excerpt at line {}",
                    file.path,
                    start + 1
                ));
                diff.extend(
                    before
                        .lines()
                        .skip(start)
                        .take(8)
                        .map(|line| format!("- {}", inspector::display_text(line))),
                );
                diff.push(format!("+++ {}", file.path));
                diff.extend(
                    after
                        .lines()
                        .skip(start)
                        .take(8)
                        .map(|line| format!("+ {}", inspector::display_text(line))),
                );
                diff.push("Full reviewed before/after images are printed in the terminal.".into());
            }
        } else {
            diff.push("No pending source diff. Edits are reviewed before acceptance.".into());
        }
        diff
    }

    fn control(&mut self, control: Control) -> Result<()> {
        self.notice = None;
        match control {
            Control::Select | Control::Move | Control::Terrain => {
                self.tools.set_mode(match control {
                    Control::Move => ToolMode::Move,
                    Control::Terrain => ToolMode::Terrain,
                    _ => ToolMode::Select,
                });
                self.sidebar_scroll = 0;
                Ok(())
            }
            Control::Clear => self.action(Action::ClearSelection),
            Control::Accept => self.action(Action::Accept),
            Control::Cancel => self.action(Action::Cancel),
            Control::Undo => self.action(Action::Undo),
            Control::Redo => self.action(Action::Redo),
            Control::Nudge(delta) => self.nudge(delta),
            Control::Palette { grid, prototype } => {
                self.tools
                    .choose_terrain(self.session.catalog(), &grid, &prototype)
                    .map_err(|error| error.to_string())?;
                if self.view.grid.as_deref() != Some(grid.as_str()) {
                    self.view.grid = Some(grid);
                    self.action(Action::ClearSelection)?;
                }
                Ok(())
            }
            Control::CancelDraft => {
                self.tools.cancel();
                Ok(())
            }
            Control::Coordinates | Control::Field(_) | Control::ApplyDraft => {
                let inspected = inspector::inspect(
                    self.session.world(),
                    self.session.catalog(),
                    self.session.selection(),
                    self.session.generation(),
                    self.session.history().document().revision(),
                );
                let unavailable = self.blocked.is_some() || !self.session.can_prepare();
                match control {
                    Control::Coordinates => self
                        .tools
                        .begin_move(&inspected, unavailable)
                        .map_err(|error| error.to_string()),
                    Control::Field(field) => self
                        .tools
                        .begin_field(&inspected, &field, unavailable)
                        .map_err(|error| error.to_string()),
                    _ => {
                        let action = self
                            .tools
                            .build_action(&inspected, unavailable)
                            .map_err(|error| error.to_string())?;
                        self.action(action)
                    }
                }
            }
        }
    }

    fn frame(&self, cells: bool) -> Result<FrameOutput> {
        let mut renderer = ReferenceRenderer::new(offscreen_target(
            self.view.camera.width,
            self.view.camera.height,
        ))
        .map_err(|error| error.to_string())?;
        renderer
            .render(RenderRequest {
                scene: if cells {
                    &self.prepared.cell_scene
                } else {
                    &self.prepared.scene
                },
                resources: self.prepared.resources.clone(),
            })
            .map_err(|error| error.to_string())
    }

    fn capture(&mut self, name: &str) -> Result<()> {
        let frame = self.frame(false)?;
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|error| error.to_string())?
                .write_image_data(&frame.rgba8)
                .map_err(|error| error.to_string())?;
        }
        self.evidence.retain(&format!("{name}.png"), &bytes)?;
        self.evidence.retain(&format!("{name}.json"), serde_json::to_string_pretty(&serde_json::json!({
            "format":"tilewright.prototype-frame/1", "path":"cpu-reference", "frame_generation":self.view.frame_revision,
            "source_revision":tilewright_editor_document::revision_hex(&self.session.world().source_digest()),
            "committed_revision":tilewright_editor_document::revision_hex(&self.session.history().document().revision()),
            "catalog_revision":tilewright_editor_document::revision_hex(&self.prepared.catalog_revision),
            "width":frame.width,"height":frame.height,"status":self.session.status().to_string(),
            "toolkit":"0.1.0","renderer":"0.5.1","pending":self.session.history().pending().is_some()
        })).map_err(|error| error.to_string())?.as_bytes())
    }

    fn select_visible_object(&mut self, for_field: bool) -> Result<()> {
        let frame = self.frame(false)?;
        let hit = self
            .prepared
            .picks
            .iter()
            .find_map(|(id, key)| {
                let SelectionKey::Object { uid } = key else {
                    return None;
                };
                let capabilities = self.session.catalog().editing();
                let eligible = if for_field {
                    capabilities.fields.iter().any(|(object, _)| object == uid)
                } else {
                    capabilities.movable_uids.contains(uid)
                };
                if !eligible {
                    return None;
                }
                pixel_for(&frame, *id)
            })
            .ok_or("no visible object with the requested edit capability in reference mask")?;
        self.tools.set_mode(ToolMode::Select);
        self.click(hit.0, hit.1)
    }

    fn select_empty_cell(&mut self) -> Result<()> {
        let frame = self.frame(true)?;
        // The authored neutral sample explicitly leaves (6,4) empty. The
        // ordinary mask path must find and select that cell before painting.
        let hit = self
            .prepared
            .cell_picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Cell { x: 6, y: 4, .. } => pixel_for(&frame, *id),
                _ => None,
            })
            .ok_or("neutral empty cell is not selectable")?;
        self.tools.set_mode(ToolMode::Terrain);
        self.click(hit.0, hit.1)
    }

    fn demo_step(&mut self, step: usize) -> Result<bool> {
        match step {
            0 => {
                self.capture("before")?;
                self.select_visible_object(false)?;
            }
            1 => {
                self.tools.set_mode(ToolMode::Move);
                self.nudge([1, 0])?;
                self.capture("pending")?;
            }
            2 => {
                self.action(Action::Cancel)?;
                self.capture("cancel")?;
                self.nudge([1, 0])?;
                self.action(Action::Accept)?;
                self.capture("after")?;
            }
            3 => {
                self.action(Action::Undo)?;
                self.action(Action::Accept)?;
                self.capture("undo")?;
            }
            4 => {
                self.action(Action::Redo)?;
                self.action(Action::Accept)?;
                self.capture("redo")?;
            }
            5 if !self.session.catalog().editing().fields.is_empty() => {
                self.select_visible_object(true)?;
                self.field()?;
                self.action(Action::Accept)?;
                self.capture("field")?;
            }
            6 if !self.session.catalog().editing().terrain.is_empty() => {
                self.select_empty_cell()?;
                let choice = tools::terrain_palette(self.session.catalog())
                    .into_iter()
                    .filter(|choice| match self.session.selection() {
                        Some(SelectionKey::Cell { grid, .. }) => *grid == choice.grid,
                        _ => false,
                    })
                    .next_back()
                    .ok_or("empty demo terrain palette")?;
                self.tools
                    .choose_terrain(self.session.catalog(), &choice.grid, &choice.prototype)
                    .map_err(|error| error.to_string())?;
                self.paint()?;
                self.action(Action::Accept)?;
                self.capture("terrain")?;
            }
            7 => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn demo_all(&mut self) -> Result<()> {
        for step in 0..8 {
            if self.demo_step(step)? {
                break;
            }
        }
        Ok(())
    }
}

fn pixel_for(frame: &FrameOutput, id: SemanticId) -> Option<(u32, u32)> {
    let index = u64::try_from(frame.semantic_ids.iter().position(|value| *value == id.0)?).ok()?;
    Some((
        u32::try_from(index % u64::from(frame.width)).ok()?,
        u32::try_from(index / u64::from(frame.width)).ok()?,
    ))
}

struct Evidence {
    root: SourceRoot,
    _lock: tilewright_source::SourceLock,
}
impl Evidence {
    fn new(root: SourceRoot) -> Result<Self> {
        let lock = root.lock().map_err(|error| error.to_string())?;
        Ok(Self { root, _lock: lock })
    }
    fn retain(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if self.root.exists(name).map_err(|error| error.to_string())? {
            return Err(format!("evidence output already exists: {name}"));
        }
        let directory = self
            .root
            .directory("", false)
            .map_err(|error| error.to_string())?
            .ok_or("evidence root missing")?;
        let access = directory
            .own_access()
            .map_err(|error| error.to_string())?
            .new_file();
        directory
            .write(name, Some(&access), bytes)
            .map_err(|error| error.to_string())?;
        if !directory.sync().map_err(|error| error.to_string())? {
            return Err("evidence directory sync unavailable".into());
        }
        Ok(())
    }
}

fn native(mut app: Application, demo: bool) -> Result<()> {
    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let mut window = video
        .window("Tilewright offline prototype", 1280, 800)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    window
        .set_minimum_size(720, 480)
        .map_err(|error| error.to_string())?;
    let (width, height) = window.size_in_pixels();
    app.ui = WorkspaceUi::new(window.size(), (width, height));
    app.view.camera.width = width;
    app.view.camera.height = height;
    app.rebuild()?;
    let mut renderer = WgpuRenderer::new(TargetDescriptor {
        kind: TargetKind::Window,
        width,
        height,
        backend: BackendPreference::Automatic,
    })
    .map_err(|error| error.to_string())?;
    println!("native adapter={:?}", renderer.adapter());
    let mut events = sdl.event_pump().map_err(|error| error.to_string())?;
    let mut demo_step = 0;
    let mut next_demo = Instant::now() + Duration::from_millis(800);
    let mut dirty = true;
    let mut phase = None;
    let mut minimized = false;
    'running: loop {
        for event in events.poll_iter() {
            if matches!(
                event,
                Event::Quit { .. }
                    | Event::Window {
                        win_event: WindowEvent::CloseRequested,
                        ..
                    }
            ) {
                break 'running;
            }
            if matches!(
                event,
                Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    ..
                }
            ) && app.tools.draft_text().is_none()
            {
                break 'running;
            }
            let result = native_event(&mut app, &mut renderer, &window, &event, &mut minimized);
            if app.tools.draft_text().is_some() {
                video.text_input().start(&window);
            } else {
                video.text_input().stop(&window);
            }
            dirty = true;
            if let Err(error) = result {
                eprintln!("{}", terminal_field(&error));
                app.notice = Some(error);
            }
        }
        if demo && !minimized && Instant::now() >= next_demo {
            if app.demo_step(demo_step)? {
                break 'running;
            }
            phase = if (demo_step == 5 && app.session.catalog().editing().fields.is_empty())
                || (demo_step == 6 && app.session.catalog().editing().terrain.is_empty())
            {
                None
            } else {
                [
                    "before", "pending", "after", "undo", "redo", "field", "terrain",
                ]
                .get(demo_step)
                .copied()
            };
            demo_step += 1;
            next_demo = Instant::now() + Duration::from_millis(2500);
            dirty = true;
        }
        if dirty && !minimized {
            if native_present(&mut app, &mut renderer, &mut window)?
                && let Some(name) = phase.take()
            {
                println!("native-phase={name}");
            }
            dirty = false;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pollster::block_on(renderer.finish()).map_err(|error| error.to_string())?;
    println!("native window closed cleanly");
    Ok(())
}

fn native_event(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &sdl3::video::Window,
    event: &Event,
    minimized: &mut bool,
) -> Result<()> {
    match event {
        Event::Window {
            win_event: WindowEvent::Minimized,
            ..
        } => {
            *minimized = true;
            Ok(())
        }
        Event::Window {
            win_event:
                WindowEvent::Restored | WindowEvent::PixelSizeChanged(..) | WindowEvent::Resized(..),
            ..
        } => {
            let size = window.size_in_pixels();
            *minimized = size.0 == 0 || size.1 == 0;
            if *minimized {
                return Ok(());
            }
            renderer
                .resize(size.0, size.1)
                .map_err(|error| error.to_string())?;
            app.ui = WorkspaceUi::new(window.size(), size);
            app.view.camera.width = size.0;
            app.view.camera.height = size.1;
            app.rebuild()
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } if !*minimized => {
            let (px, py) =
                crate::viewport::physical_pixel([*x, *y], window.size(), window.size_in_pixels())
                    .ok_or("pointer outside physical viewport")?;
            app.refresh_ui();
            match app.ui.hit(px, py) {
                Hit::Control(control) => app.control(control),
                Hit::World(x, y) => {
                    app.tools.cancel();
                    app.click(x, y)?;
                    if app.tools.mode() == ToolMode::Terrain {
                        app.paint()
                    } else {
                        Ok(())
                    }
                }
                Hit::Panel | Hit::Outside => Ok(()),
            }
        }
        Event::MouseWheel {
            integer_y,
            mouse_x,
            mouse_y,
            ..
        } if !*minimized => {
            let Some((x, y)) = crate::viewport::physical_pixel(
                [*mouse_x, *mouse_y],
                window.size(),
                window.size_in_pixels(),
            ) else {
                return Ok(());
            };
            let amount = usize::try_from(integer_y.unsigned_abs())
                .unwrap_or(0)
                .min(32);
            let scroll = if app.ui.sidebar.contains(x, y) {
                &mut app.sidebar_scroll
            } else if app.ui.review.contains(x, y) {
                &mut app.review_scroll
            } else {
                return Ok(());
            };
            *scroll = if *integer_y > 0 {
                scroll.saturating_sub(amount)
            } else {
                scroll.saturating_add(amount)
            };
            Ok(())
        }
        Event::TextInput { text, .. } if app.tools.draft_text().is_some() => {
            app.tools.input(text).map_err(|error| error.to_string())
        }
        Event::KeyDown {
            keycode: Some(Keycode::A),
            keymod,
            repeat: false,
            ..
        } if app.tools.draft_text().is_some()
            && keymod.intersects(sdl3::keyboard::Mod::LCTRLMOD | sdl3::keyboard::Mod::RCTRLMOD) =>
        {
            while app.tools.draft_text().is_some_and(|text| !text.is_empty()) {
                app.tools.backspace();
            }
            Ok(())
        }
        Event::KeyDown {
            keycode: Some(key),
            repeat: false,
            ..
        } => native_key(app, *key),
        _ => Ok(()),
    }
}

fn native_key(app: &mut Application, key: Keycode) -> Result<()> {
    // Text focus owns every key before global shortcuts. Unicode text arrives
    // exclusively through SDL TextInput, never by guessing keycode characters.
    if app.tools.draft_text().is_some() {
        return match key {
            Keycode::Return | Keycode::KpEnter => app.control(Control::ApplyDraft),
            Keycode::Escape => app.control(Control::CancelDraft),
            Keycode::Backspace => {
                app.tools.backspace();
                Ok(())
            }
            _ => Ok(()),
        };
    }
    match key {
        Keycode::Return | Keycode::KpEnter => app.action(Action::Accept),
        Keycode::Escape => app.action(Action::Cancel),
        Keycode::Z => app.action(Action::Undo),
        Keycode::Y => app.action(Action::Redo),
        Keycode::R => app.action(Action::Reopen),
        Keycode::Left if app.tools.mode() == ToolMode::Move => app.nudge([-1, 0]),
        Keycode::Right if app.tools.mode() == ToolMode::Move => app.nudge([1, 0]),
        Keycode::Up if app.tools.mode() == ToolMode::Move => app.nudge([0, -1]),
        Keycode::Down if app.tools.mode() == ToolMode::Move => app.nudge([0, 1]),
        Keycode::T => app.control(Control::Terrain),
        Keycode::W | Keycode::A | Keycode::S | Keycode::D => {
            let change = match key {
                Keycode::W => [0.0, -1.0],
                Keycode::A => [-1.0, 0.0],
                Keycode::S => [0.0, 1.0],
                _ => [1.0, 0.0],
            };
            app.view.camera.center[0] += change[0];
            app.view.camera.center[1] += change[1];
            app.rebuild()
        }
        Keycode::Plus | Keycode::Equals | Keycode::Minus => {
            let factor = if key == Keycode::Minus { 0.8 } else { 1.25 };
            app.view.camera.zoom = (app.view.camera.zoom * factor).clamp(0.25, 8.0);
            app.rebuild()
        }
        _ => Ok(()),
    }
}

fn native_present(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &mut sdl3::video::Window,
) -> Result<bool> {
    app.refresh_ui();
    let title = if let Some(reason) = &app.blocked {
        format!("Tilewright BLOCKED: {reason} | Q quit")
    } else {
        format!(
            "Tilewright {} | {} | {}",
            app.session.catalog().presentation(),
            app.session.status(),
            HELP
        )
    };
    window
        .set_title(&terminal_field(&title))
        .map_err(|error| error.to_string())?;
    match tilewright_render_sdl3::present_existing(
        renderer,
        window,
        RenderRequest {
            scene: &app.prepared.scene,
            resources: app.prepared.resources.clone(),
        },
    ) {
        Ok(proof) => {
            println!(
                "native presented {}x{} backend={} adapter={}",
                proof.width,
                proof.height,
                terminal_field(&proof.backend),
                terminal_field(&proof.adapter_name)
            );
            Ok(true)
        }
        Err(error) => {
            let reason = error.to_string();
            app.blocked = Some(reason.clone());
            window
                .set_title(&format!(
                    "Tilewright BLOCKED: {} | Q quit",
                    terminal_field(&reason)
                ))
                .map_err(|error| error.to_string())?;
            eprintln!("{}", terminal_field(&reason));
            Ok(false)
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "tests/prototype.rs"]
mod tests;
