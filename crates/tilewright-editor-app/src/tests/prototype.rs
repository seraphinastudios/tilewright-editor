// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! App regression tests use the same Session, History, durable-store and CPU
//! scene paths as the scripted native demonstration; they initialize no GPU.
use super::{Application, Evidence};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tilewright_editor_commands::History;
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Session};
use tilewright_source::SourceRoot;

fn copy_private(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_private(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
            fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

fn application() -> (tempfile::TempDir, Application) {
    let temporary = std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT").map_or_else(
        || tempfile::tempdir().unwrap(),
        |root| tempfile::tempdir_in(root).unwrap(),
    );
    let world = temporary.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    for name in ["recovery", "evidence"] {
        let path = temporary.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let document = WorldDocument::open_prototype(&world).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let view = catalog.default_view(document.world(), 320, 240, 1);
    let prepared = tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .unwrap();
    let recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&temporary.path().join("recovery")),
        RecoveryLimits::default(),
    )
    .unwrap();
    let evidence = Evidence::new(SourceRoot::new(&temporary.path().join("evidence"))).unwrap();
    let app = Application {
        session: Session::new(History::new(document), catalog, resources),
        recovery,
        evidence,
        view,
        prepared,
        tools: tilewright_editor_ui::tools::ToolState::default(),
        ui: crate::workspace_ui::WorkspaceUi::new((320, 240), (320, 240)),
        notice: None,
        sidebar_scroll: 0,
        review_scroll: 0,
        blocked: None,
    };
    (temporary, app)
}

#[test]
fn full_neutral_script_reselects_after_commits_and_edits_field_and_empty_cell() {
    let (temporary, mut app) = application();
    app.demo_all().unwrap();
    let marker = app.session.world().object("marker_1").unwrap();
    assert!(marker.fields.iter().any(|(name, value)| name == "label"
        && value == &tilewright_source::FieldValue::String("Edited offline".into())));
    let chunk = &app.session.world().planes()["main"].chunks[&(0, 0, 0)];
    assert_eq!(chunk.ground[0].value(4 * 32 + 6), Some("sand"));
    for phase in [
        "before", "pending", "cancel", "after", "undo", "redo", "field", "terrain",
    ] {
        assert!(
            temporary
                .path()
                .join(format!("evidence/{phase}.png"))
                .is_file()
        );
    }
    assert!(app.session.history().pending().is_none());
    assert_eq!(
        app.prepared.source_revision,
        app.session.world().source_digest()
    );
}

#[test]
fn rejected_pending_undo_refreshes_actual_pending_scene_and_invalidates_old_picks() {
    let (_temporary, mut app) = application();
    app.select_visible_object(false).unwrap();
    app.nudge([1, 0]).unwrap();
    let pending = app.session.world().source_digest();
    let pending_generation = app.view.frame_revision;
    let error = app.action(Action::Undo).unwrap_err();
    assert!(error.contains("pending"), "{error}");
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.session.world().source_digest(), pending);
    assert_eq!(app.prepared.source_revision, pending);
    assert!(app.view.frame_revision > pending_generation);
    assert!(app.prepared.pick(pending_generation, 1, 1, false).is_err());
}

#[test]
fn background_click_cannot_paint_an_earlier_cell_selection() {
    let (_temporary, mut app) = application();
    app.select_empty_cell().unwrap();
    let original = app.session.world().source_digest();
    let frame = app.frame(true).unwrap();
    let background = frame.semantic_ids.iter().position(|id| *id == 0).unwrap();
    let x = u32::try_from(background % usize::try_from(frame.width).unwrap()).unwrap();
    let y = u32::try_from(background / usize::try_from(frame.width).unwrap()).unwrap();
    let result = app.click(x, y).and_then(|()| app.paint());
    assert!(result.unwrap_err().contains("no selectable semantic hit"));
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.world().source_digest(), original);
}

#[test]
fn focused_text_consumes_global_shortcuts_without_invalidating_world_generation() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    let (_temporary, mut app) = application();
    app.select_visible_object(true).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    let generation = app.session.generation();
    let camera = app.view.camera.center;
    let draft = app.tools.draft_text().unwrap().to_owned();
    for key in [
        Keycode::Q,
        Keycode::Z,
        Keycode::Y,
        Keycode::W,
        Keycode::A,
        Keycode::S,
        Keycode::D,
        Keycode::Left,
    ] {
        super::native_key(&mut app, key).unwrap();
    }
    app.refresh_ui();
    assert_eq!(app.tools.draft_text(), Some(draft.as_str()));
    assert_eq!(app.session.generation(), generation);
    assert_eq!(app.view.camera.center, camera);
    assert!(app.session.history().pending().is_none());
    app.tools.input(" qzywasd").unwrap();
    app.refresh_ui();
    assert_eq!(app.session.generation(), generation);
    assert!(app.tools.draft_text().unwrap().ends_with(" qzywasd"));
    super::native_key(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.draft_text().is_none());
    assert_eq!(app.session.generation(), generation);
}

#[test]
fn terrain_requires_explicit_palette_and_panel_controls_never_publish() {
    use crate::workspace_ui::Control;
    let (_temporary, mut app) = application();
    app.select_empty_cell().unwrap();
    assert!(app.paint().unwrap_err().contains("Choose"));
    let revision = app.session.history().document().revision();
    let choice = tilewright_editor_ui::tools::terrain_palette(app.session.catalog())
        .into_iter()
        .next_back()
        .unwrap();
    app.control(Control::Palette {
        grid: choice.grid,
        prototype: choice.prototype,
    })
    .unwrap();
    app.refresh_ui();
    assert_eq!(app.session.history().document().revision(), revision);
    assert!(app.session.history().pending().is_none());
    app.paint().unwrap();
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.session.history().document().revision(), revision);
}
