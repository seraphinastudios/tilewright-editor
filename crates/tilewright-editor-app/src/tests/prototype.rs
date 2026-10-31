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
        unsupported_runs: 0,
        presented_generation: None,
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

#[test]
fn unsupported_draft_is_visible_local_state_and_cannot_prepare_a_lossy_edit() {
    use crate::workspace_ui::Control;
    let (_temporary, mut app) = application();
    app.select_visible_object(true).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.input("\u{301}").unwrap();
    let draft = app.tools.draft_text().unwrap().to_owned();
    let revision = app.session.history().document().revision();
    let generation = app.session.generation();
    assert!(
        app.control(Control::ApplyDraft)
            .unwrap_err()
            .contains("unsupported")
    );
    app.refresh_ui();
    assert_eq!(app.tools.draft_text(), Some(draft.as_str()));
    assert_eq!(app.session.history().document().revision(), revision);
    assert_eq!(app.session.generation(), generation);
    assert!(app.session.history().pending().is_none());
}

#[test]
fn input_requires_a_displayed_current_frame_and_panels_consume_blank_pixels() {
    let (_temporary, mut app) = application();
    let revision = app.session.world().source_digest();
    assert!(
        super::native_pointer(&mut app, [20.0, 20.0], (320, 240), (320, 240))
            .unwrap_err()
            .contains("displayed")
    );
    app.presented_generation = Some(app.view.frame_revision);
    super::native_pointer(&mut app, [250.0, 230.0], (320, 240), (320, 240)).unwrap();
    assert!(app.session.selection().is_none());
    assert_eq!(app.session.world().source_digest(), revision);
    app.rebuild().unwrap();
    assert!(!app.displayed_input_ready());
}

#[test]
fn composed_ui_preserves_world_geometry_and_rejects_exhausted_identities() {
    use tilewright_scene::{ResourceId, SceneBundle, SceneLimits, SemanticId};
    let (_temporary, mut app) = application();
    app.refresh_ui();
    let presentation = crate::workspace_render::compose(
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap();
    for original in app.prepared.scene.quads() {
        assert!(presentation.scene.quads().contains(original));
    }
    for (resource, semantic) in [(u64::MAX, 1), (1, u32::MAX)] {
        let mut quads = vec![app.prepared.scene.quads()[0]];
        quads[0].resource_id = ResourceId(resource);
        quads[0].semantic_id = SemanticId(semantic);
        let world = SceneBundle::from_quads(
            app.view.frame_revision,
            0,
            app.prepared.scene.viewport(),
            app.prepared.scene.clear_color(),
            quads,
            SceneLimits::default(),
        )
        .unwrap();
        let error =
            crate::workspace_render::compose(&app.ui, &world, app.prepared.resources.clone())
                .err()
                .unwrap();
        assert!(matches!(
            error,
            crate::workspace_render::CompositionError::ResourceIdentityExhausted
                | crate::workspace_render::CompositionError::SemanticIdentityExhausted
        ));
    }
}

#[test]
fn cpu_evidence_png_contains_shared_workspace_ui_and_preserves_world_revision_proof() {
    let (temporary, mut app) = application();
    let world = app.frame(false).unwrap();
    let generation = app.session.generation();
    let revision = app.session.world().source_digest();
    let composed = app.workspace_frame().unwrap();
    assert_eq!(
        (composed.width, composed.height),
        (world.width, world.height)
    );
    assert_ne!(composed.rgba8, world.rgba8);
    assert_eq!(app.session.generation(), generation);
    assert_eq!(app.session.world().source_digest(), revision);
    app.capture("workspace-cpu-proof").unwrap();
    let bytes = fs::read(temporary.path().join("evidence/workspace-cpu-proof.png")).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!(&decoded[..info.buffer_size()], composed.rgba8.as_slice());
    let proof: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary.path().join("evidence/workspace-cpu-proof.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(proof["format"], "tilewright.prototype-frame/1");
    assert_eq!(proof["path"], "cpu-reference");
    assert_eq!(proof["composition"], "workspace-ui");
    assert_eq!(proof["frame_generation"], generation);
    assert_eq!(
        proof["source_revision"],
        tilewright_editor_document::revision_hex(&revision)
    );
    assert_eq!(
        proof["committed_revision"],
        tilewright_editor_document::revision_hex(&app.session.history().document().revision())
    );
}
