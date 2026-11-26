// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A person's favourites in the desktop, through real SDL pointer events on
//! the CPU scene path: shown first, chosen as the catalog value they repeat
//! (so the prepared change is byte for byte the catalog row's), disabled
//! when this catalog does not grant them, ignored as a whole when
//! malformed, and written back only as new files in the session.
use super::{BRUSH_WINDOW, acknowledge_ui, cell_pixel, delete_application};
use crate::prototype::Application;
use crate::prototype::favourites_view::{self, Personal};
use crate::workspace_ui::{Control, WorkspaceUi};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tilewright_editor_ui::Action;
use tilewright_editor_ui::favourites::{FORMAT, Favourite, Favourites};
use tilewright_editor_ui::tools::TerrainChoice;
use tilewright_source::SourceRoot;

fn sand() -> TerrainChoice {
    TerrainChoice {
        grid: "floor".into(),
        prototype: "sand".into(),
    }
}

/// Makes the session's private `palettes/` directory under `root` holding
/// `bytes` as the helper copies them.
fn palettes_directory(root: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let directory = root.join("palettes");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let file = directory.join(favourites_view::FILE);
    fs::write(&file, bytes).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    directory
}

/// The 1280x800 application with a seeded random source and, given
/// `bytes`, the person's favourites file.
fn favourite_application(bytes: Option<&[u8]>) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = delete_application(|_| {});
    app.session
        .set_uid_source(Box::new(tilewright_editor_ui::uid::SeededUidSource::new(
            11,
        )));
    if let Some(bytes) = bytes {
        let directory = palettes_directory(temporary.path(), bytes);
        app.favourites = Some(Personal::open(SourceRoot::new(&directory)).unwrap());
    }
    (temporary, app)
}

/// The sidebar's rows as laid out: text, control, enabled, active.
fn rows(app: &mut Application) -> Vec<(String, Option<Control>, bool, bool)> {
    app.refresh_ui();
    let x = app.ui.sidebar_x();
    app.ui
        .widgets
        .iter()
        .filter(|widget| widget.layout.x >= x)
        .map(|widget| {
            (
                widget.text.clone(),
                widget.control.clone(),
                widget.enabled,
                widget.active,
            )
        })
        .collect()
}

fn texts(app: &mut Application) -> Vec<String> {
    rows(app).into_iter().map(|row| row.0).collect()
}

/// Clicks the first laid-out row with `control` through a real pointer
/// event, and returns its place among the sidebar's rows.
fn click_first(app: &mut Application, control: &Control) -> usize {
    acknowledge_ui(app);
    let at = rows(app)
        .iter()
        .position(|row| row.1.as_ref() == Some(control))
        .unwrap_or_else(|| panic!("no row for {control:?}"));
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(control))
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    let point = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    crate::prototype::native_pointer(app, point, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    at
}

/// A drag from `from` to `to`, released: the pending change it prepared.
fn drag(app: &mut Application, from: [i64; 2], to: [i64; 2]) -> String {
    acknowledge_ui(app);
    let start = cell_pixel(app, from[0], from[1]);
    let end = cell_pixel(app, to[0], to[1]);
    crate::prototype::native_pointer(app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    crate::prototype::native_stroke_motion(app, end, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    crate::prototype::native_stroke_release(app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let sealed = app.session.history().pending().unwrap().sealed().to_owned();
    app.action(Action::Cancel).unwrap();
    sealed
}

/// Paints with sand, scatters speckle and places a stone, each chosen by
/// the first row that chooses it, and returns the changes prepared and the
/// rows clicked.
fn paint_scatter_place(app: &mut Application) -> (Vec<String>, Vec<usize>) {
    let mut sealed = Vec::new();
    let mut clicked = Vec::new();
    app.control(Control::Terrain).unwrap();
    clicked.push(click_first(
        app,
        &Control::Palette {
            grid: "floor".into(),
            prototype: "sand".into(),
        },
    ));
    sealed.push(drag(app, [0, 1], [3, 1]));
    clicked.push(click_first(app, &Control::ScatterPalette("speckle".into())));
    sealed.push(drag(app, [1, 2], [4, 2]));
    app.control(Control::Place).unwrap();
    clicked.push(click_first(app, &Control::PlacePalette("stone".into())));
    acknowledge_ui(app);
    let tile = cell_pixel(app, 6, 1);
    crate::prototype::native_pointer(app, tile, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    sealed.push(app.session.history().pending().unwrap().sealed().to_owned());
    (sealed, clicked)
}

#[test]
fn choosing_a_favourite_prepares_byte_for_byte_what_the_catalog_row_prepares() {
    let lists = Favourites {
        terrain: vec![sand()],
        scatter: vec!["speckle".into()],
        objects: vec!["stone".into()],
    };
    let (_with, mut app) = favourite_application(Some(&lists.encode()));
    let (favourite, clicked) = paint_scatter_place(&mut app);
    let (_without, mut plain) = favourite_application(None);
    let (catalog, plain_clicked) = paint_scatter_place(&mut plain);
    assert_eq!(favourite, catalog);
    assert_eq!(favourite.len(), 3);
    // With favourites the rows clicked were theirs, above the catalog's.
    for (with, without) in clicked.iter().zip(&plain_clicked) {
        assert!(with < without, "{clicked:?} {plain_clicked:?}");
    }
    // Nothing about favourites is recorded: no step names them.
    for sealed in &favourite {
        assert!(!sealed.contains("favourite"), "{sealed}");
    }
    // The prepared stroke's words are the catalog's too.
    assert_eq!(app.diff_lines(), plain.diff_lines());
}

#[test]
#[allow(clippy::too_many_lines)]
fn favourites_come_first_and_ungranted_ones_only_offer_their_removal() {
    let file = format!(
        r#"{{"format": "{FORMAT}", "terrain": [{{"grid": "floor", "value": "lava"}}, {{"grid": "floor", "value": "sand"}}], "scatter": ["dunes", "speckle"], "objects": ["tree", "stone"]}}"#
    );
    let (_temporary, mut app) = favourite_application(Some(file.as_bytes()));
    app.control(Control::Terrain).unwrap();
    let laid = rows(&mut app);
    let shown: Vec<&str> = laid.iter().map(|row| row.0.as_str()).collect();
    let header = shown.iter().position(|text| *text == "Favourites").unwrap();
    let palette = shown
        .iter()
        .position(|text| *text == "Terrain palette")
        .unwrap();
    assert!(header < palette, "{shown:?}");
    let section = &laid[header + 1..palette];
    let sand_row = laid
        .iter()
        .rev()
        .find(|row| row.0 == "floor / sand")
        .unwrap();
    let speckle_row = laid
        .iter()
        .find(|row| row.1 == Some(Control::ScatterPalette("speckle".into())))
        .unwrap();
    assert_eq!(
        section,
        [
            (
                "Remove floor / lava (not granted)".into(),
                Some(Control::FavouriteRemove(Favourite::Terrain(
                    TerrainChoice {
                        grid: "floor".into(),
                        prototype: "lava".into()
                    }
                ))),
                true,
                false
            ),
            sand_row.clone(),
            (
                "Remove scatter dunes".into(),
                Some(Control::FavouriteRemove(Favourite::Scatter("dunes".into()))),
                true,
                false
            ),
            speckle_row.clone(),
            (
                favourites_view::STAR.into(),
                Some(Control::FavouriteStar),
                false,
                false
            ),
        ]
    );
    // The favourite and the catalog row of sand are one row, twice.
    assert_eq!(
        laid.iter()
            .filter(|row| row.1 == sand_row.1 && row.0 == sand_row.0)
            .count(),
        2
    );
    // Choosing sand marks both rows and offers to unstar it.
    app.control(sand_row.1.clone().unwrap()).unwrap();
    let laid = rows(&mut app);
    assert!(
        laid.iter()
            .filter(|row| row.0 == "floor / sand")
            .all(|row| row.3)
    );
    assert!(laid.contains(&(
        favourites_view::UNSTAR.into(),
        Some(Control::FavouriteStar),
        true,
        true
    )));
    // Sand is second (after lava), so it can move up.
    assert!(laid.contains(&(
        favourites_view::RAISE.into(),
        Some(Control::FavouriteRaise),
        true,
        false
    )));
    // Place mode lists the prototypes the same way.
    app.control(Control::Place).unwrap();
    let laid = rows(&mut app);
    let header = laid.iter().position(|row| row.0 == "Favourites").unwrap();
    assert_eq!(
        laid[header + 1],
        (
            "Remove tree (not granted)".into(),
            Some(Control::FavouriteRemove(Favourite::Object("tree".into()))),
            true,
            false
        )
    );
    assert_eq!(
        laid[header + 2],
        (
            "stone / item".into(),
            Some(Control::PlacePalette("stone".into())),
            true,
            false
        )
    );
    // Its row carries the prototype's thumbnail, as the catalog row does.
    let thumbnails: Vec<_> = app
        .ui
        .widgets
        .iter()
        .filter(|widget| widget.control == Some(Control::PlacePalette("stone".into())))
        .map(|widget| widget.thumbnail)
        .collect();
    assert_eq!(thumbnails.len(), 2);
    assert!(thumbnails[0].is_some() && thumbnails[0] == thumbnails[1]);
    // Other modes show none.
    for mode in [Control::Select, Control::Zone, Control::Anchor] {
        app.control(mode).unwrap();
        assert!(!texts(&mut app).iter().any(|text| text == "Favourites"));
    }
}

#[test]
fn without_a_file_no_mode_shows_favourites() {
    let (_temporary, mut app) = favourite_application(None);
    for mode in [Control::Terrain, Control::Place] {
        app.control(mode).unwrap();
        let shown = texts(&mut app);
        assert!(!shown.iter().any(|text| text == "Favourites"), "{shown:?}");
        assert!(
            !shown
                .iter()
                .any(|text| text.starts_with("Star") || text.starts_with("Unstar"))
        );
    }
    assert!(app.control(Control::FavouriteStar).is_err());
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_change_is_a_new_file_in_the_session_and_nothing_is_replaced() {
    let empty = Favourites::default().encode();
    let (temporary, mut app) = favourite_application(Some(&empty));
    let directory = temporary.path().join("palettes");
    let saved = |index: u32| directory.join(favourites_view::saved_name(index));
    app.control(Control::Terrain).unwrap();
    assert!(texts(&mut app).contains(&favourites_view::EMPTY.to_owned()));
    // Nothing chosen yet: the star row is disabled and refuses.
    assert!(app.tools.terrain_choice().is_none());
    let error = app.control(Control::FavouriteStar).unwrap_err();
    assert_eq!(error, "Favourites not saved: choose a value first");
    assert!(!saved(1).exists());

    let grass = TerrainChoice {
        grid: "floor".into(),
        prototype: "grass".into(),
    };
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    click_first(&mut app, &Control::FavouriteStar);
    let one = Favourites {
        terrain: vec![sand()],
        ..Favourites::default()
    };
    assert_eq!(fs::read(saved(1)).unwrap(), one.encode());
    assert!(texts(&mut app).contains(&"Saved: favourites-0001.json".to_owned()));
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "grass".into(),
    })
    .unwrap();
    app.control(Control::FavouriteStar).unwrap();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    app.control(Control::FavouriteStar).unwrap();
    app.control(Control::Place).unwrap();
    // Place mode's chosen prototype on entry is bush.
    app.control(Control::FavouriteStar).unwrap();
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "grass".into(),
    })
    .unwrap();
    click_first(&mut app, &Control::FavouriteRaise);
    let five = Favourites {
        terrain: vec![grass.clone(), sand()],
        scatter: vec!["speckle".into()],
        objects: vec!["bush".into()],
    };
    assert_eq!(fs::read(saved(5)).unwrap(), five.encode());
    assert_eq!(
        Favourites::decode(&fs::read(saved(5)).unwrap()).unwrap(),
        five
    );
    // First now: it moves no further, and the refusal writes nothing.
    assert!(
        !rows(&mut app)
            .iter()
            .any(|row| row.1 == Some(Control::FavouriteRaise))
    );
    let error = app.control(Control::FavouriteRaise).unwrap_err();
    assert!(error.contains("already first"), "{error}");
    assert!(!saved(6).exists());
    // The section lists them in their order.
    let shown = texts(&mut app);
    let header = shown.iter().position(|text| text == "Favourites").unwrap();
    assert_eq!(
        &shown[header + 1..header + 4],
        [
            "floor / grass",
            "floor / sand",
            "floor / speckle: grass 3, sand 1"
        ]
    );

    // A file at the next name is someone else's: refused, left alone, and
    // the lists stay as they were.
    fs::write(saved(6), b"not ours").unwrap();
    let error = app.control(Control::FavouriteStar).unwrap_err();
    assert_eq!(
        error,
        "Favourites not saved: favourites-0006.json is already in the session; nothing was replaced"
    );
    assert_eq!(fs::read(saved(6)).unwrap(), b"not ours");
    assert_eq!(app.favourites.as_ref().unwrap().lists().unwrap(), &five);
    assert_eq!(app.favourites.as_ref().unwrap().saves(), 5);
    // The file the helper copied in is never written.
    assert_eq!(
        fs::read(directory.join(favourites_view::FILE)).unwrap(),
        empty
    );
    // Every saved file is private, and nothing else was made.
    let mut names: Vec<String> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "favourites-0001.json",
            "favourites-0002.json",
            "favourites-0003.json",
            "favourites-0004.json",
            "favourites-0005.json",
            "favourites-0006.json",
            "favourites.json"
        ]
    );
    for index in 1..=5 {
        let mode = fs::metadata(saved(index)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    // No edit was prepared by any of it.
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.history().depths(), (0, 0));
}

#[test]
fn a_malformed_file_is_ignored_as_a_whole_and_never_written_back() {
    let file = format!(r#"{{"format": "{FORMAT}", "objects": ["stone", "stone"]}}"#);
    let (temporary, mut app) = favourite_application(Some(file.as_bytes()));
    let personal = app.favourites.as_ref().unwrap();
    assert_eq!(
        personal.summary(),
        "favourites: file ignored: objects entry 2 repeats an earlier entry"
    );
    app.control(Control::Place).unwrap();
    let laid = rows(&mut app);
    let header = laid.iter().position(|row| row.0 == "Favourites").unwrap();
    let reason: Vec<_> = laid[header + 1..header + 4]
        .iter()
        .map(|row| (row.0.as_str(), row.1.is_none(), row.2))
        .collect();
    assert_eq!(
        reason,
        [
            (favourites_view::IGNORED, true, false),
            ("objects entry 2 repeats an", true, false),
            ("earlier entry", true, false)
        ]
    );
    assert!(!laid.iter().any(|row| row.1 == Some(Control::FavouriteStar)));
    let error = app.control(Control::FavouriteStar).unwrap_err();
    assert!(error.contains("was ignored"), "{error}");
    assert_eq!(
        fs::read_dir(temporary.path().join("palettes"))
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("favourites-")
            })
            .count(),
        0
    );
    // Editing goes on as without favourites.
    app.control(Control::PlacePalette("stone".into())).unwrap();
    assert_eq!(app.tools.placement_choice(), Some("stone"));
    // Too large is ignored the same way.
    let large = vec![b' '; tilewright_editor_ui::favourites::MAXIMUM_BYTES + 1];
    let root = tempfile::tempdir_in(temporary.path()).unwrap();
    let directory = palettes_directory(root.path(), &large);
    let personal = Personal::open(SourceRoot::new(&directory)).unwrap();
    assert_eq!(
        personal.summary(),
        "favourites: file ignored: the file is larger than 64 KiB"
    );
}

#[test]
fn a_directory_or_file_that_is_not_private_or_missing_is_refused() {
    let temporary = std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT").map_or_else(
        || tempfile::tempdir().unwrap(),
        |root| tempfile::tempdir_in(root).unwrap(),
    );
    let open = |directory: &Path| {
        Personal::open(SourceRoot::new(directory))
            .err()
            .unwrap_or_default()
    };
    let empty = Favourites::default().encode();
    let directory = palettes_directory(temporary.path(), &empty);
    assert_eq!(open(&directory), "");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(open(&directory).contains("mode exactly 0700"));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let file = directory.join(favourites_view::FILE);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(open(&directory).contains("mode exactly 0600"));
    fs::remove_file(&file).unwrap();
    assert!(open(&directory).contains("has no favourites.json"));
    let elsewhere = temporary.path().join("elsewhere.json");
    fs::write(&elsewhere, &empty).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
    assert!(open(&directory).contains("favourites refused"));
}

#[test]
fn the_scripted_demonstration_and_a_shared_descriptor_refuse_favourites() {
    let arguments = |extra: &[&str]| -> Vec<String> {
        ["--palettes-fd", "9"]
            .iter()
            .chain(extra)
            .map(|text| (*text).to_owned())
            .collect()
    };
    let world = std::path::PathBuf::from("/proc/self/fd/3");
    let others = [&world];
    let error = super::super::palettes_argument(&arguments(&[]), &others, None, true).unwrap_err();
    assert!(error.contains("never reach evidence"), "{error}");
    let nine = std::path::PathBuf::from("/proc/self/fd/9");
    let error =
        super::super::palettes_argument(&arguments(&[]), &[&nine], None, false).unwrap_err();
    assert!(error.contains("distinct"), "{error}");
    let error =
        super::super::palettes_argument(&arguments(&[]), &others, Some(&nine), false).unwrap_err();
    assert!(error.contains("distinct"), "{error}");
    assert_eq!(
        super::super::palettes_argument(&arguments(&[]), &others, None, false).unwrap(),
        Some(nine)
    );
    assert_eq!(
        super::super::palettes_argument(&[], &others, None, true).unwrap(),
        None
    );
}

#[test]
fn the_section_fits_at_1280x800_720x480_and_twice_the_pixels() {
    let long = "v".repeat(128);
    let file = format!(
        r#"{{"format": "{FORMAT}", "terrain": [{{"grid": "floor", "value": "sand"}}, {{"grid": "floor", "value": "{long}"}}], "scatter": ["speckle"], "objects": ["stone", "{long}"]}}"#
    );
    for (logical, physical) in [
        ((1280, 800), (1280, 800)),
        ((720, 480), (720, 480)),
        ((1280, 800), (2560, 1600)),
        ((720, 480), (1440, 960)),
    ] {
        let (_temporary, mut app) = favourite_application(Some(file.as_bytes()));
        app.ui = WorkspaceUi::new(logical, physical);
        app.control(Control::Palette {
            grid: "floor".into(),
            prototype: "sand".into(),
        })
        .unwrap();
        for mode in [Control::Terrain, Control::Place] {
            app.control(mode.clone()).unwrap();
            // Scrolled through, every row of the section is laid out whole
            // and fits its width.
            let mut seen = std::collections::BTreeSet::new();
            for scroll in 0..40 {
                app.sidebar_scroll = scroll;
                app.refresh_ui();
                for widget in &app.ui.widgets {
                    let measured = app
                        .text
                        .layout(
                            &widget.text,
                            crate::workspace_render::text_size(widget.layout.height),
                            None,
                        )
                        .unwrap();
                    #[allow(clippy::cast_precision_loss)]
                    let available = widget.layout.width.saturating_sub(8) as f32;
                    assert!(
                        measured.size()[0] <= available + 0.001,
                        "{logical:?} {mode:?}: {} exceeds {available}",
                        widget.text
                    );
                    seen.insert(widget.text.clone());
                }
            }
            assert!(seen.contains("Favourites"), "{logical:?} {mode:?}");
            assert!(
                seen.iter()
                    .any(|text| text.starts_with(favourites_view::REMOVE) && text.ends_with("vvv")),
                "{logical:?} {mode:?} {seen:?}"
            );
            let star = if mode == Control::Terrain {
                favourites_view::UNSTAR
            } else {
                favourites_view::STAR
            };
            assert!(seen.contains(star), "{logical:?} {mode:?} {seen:?}");
        }
    }
}

#[test]
fn an_ungranted_favourite_is_removed_from_its_own_row_and_nothing_else_is() {
    let file = format!(
        r#"{{"format": "{FORMAT}", "terrain": [{{"grid": "floor", "value": "sand"}}], "objects": ["tree", "stone"]}}"#
    );
    let (temporary, mut app) = favourite_application(Some(file.as_bytes()));
    let saved = temporary
        .path()
        .join("palettes")
        .join(favourites_view::saved_name(1));
    app.control(Control::Place).unwrap();
    let tree = Control::FavouriteRemove(Favourite::Object("tree".into()));
    // A real click on the row removes it, and only it.
    click_first(&mut app, &tree);
    let left = Favourites {
        terrain: vec![sand()],
        objects: vec!["stone".into()],
        ..Favourites::default()
    };
    assert_eq!(fs::read(&saved).unwrap(), left.encode());
    assert!(!rows(&mut app).iter().any(|row| row.1 == Some(tree.clone())));
    // A granted favourite, or one not in the lists, is not removed this way.
    for control in [
        Control::FavouriteRemove(Favourite::Object("stone".into())),
        Control::FavouriteRemove(Favourite::Object("tree".into())),
        Control::FavouriteRemove(Favourite::Terrain(sand())),
    ] {
        let error = app.control(control).unwrap_err();
        assert!(error.contains("does not grant"), "{error}");
    }
    assert_eq!(app.favourites.as_ref().unwrap().saves(), 1);
    assert_eq!(app.favourites.as_ref().unwrap().lists().unwrap(), &left);
    assert!(app.session.history().pending().is_none());
}

#[test]
fn the_last_of_9999_saves_is_written_and_the_next_is_refused() {
    let (temporary, mut app) = favourite_application(Some(&Favourites::default().encode()));
    let directory = temporary.path().join("palettes");
    app.favourites
        .as_mut()
        .unwrap()
        .set_saves(favourites_view::MAXIMUM_SAVES - 1);
    app.control(Control::Place).unwrap();
    app.control(Control::FavouriteStar).unwrap();
    // Four digits, as the helper copies back.
    assert!(directory.join("favourites-9999.json").exists());
    let error = app.control(Control::FavouriteStar).unwrap_err();
    assert_eq!(
        error,
        "Favourites not saved: this session already wrote 9999 favourites files"
    );
    assert!(!directory.join("favourites-10000.json").exists());
    assert_eq!(
        app.favourites.as_ref().unwrap().lists().unwrap().objects,
        ["bush"]
    );
}

#[test]
fn the_directory_is_checked_again_before_every_save() {
    let (temporary, mut app) = favourite_application(Some(&Favourites::default().encode()));
    let directory = temporary.path().join("palettes");
    app.control(Control::Place).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
    let error = app.control(Control::FavouriteStar).unwrap_err();
    assert!(error.contains("changed"), "{error}");
    assert!(!directory.join(favourites_view::saved_name(1)).exists());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    app.control(Control::FavouriteStar).unwrap();
    assert!(directory.join(favourites_view::saved_name(1)).exists());
    // The checks themselves: mode, owner, file system, and a sync that
    // could not be made durable.
    let access = tilewright_source::Access {
        mode: 0o700,
        user: owner(&directory),
        group: 0,
        control: [0; 32],
    };
    assert!(favourites_view::private(access, None).is_ok());
    let open = tilewright_source::Access {
        mode: 0o755,
        ..access
    };
    assert!(
        favourites_view::private(open, None)
            .unwrap_err()
            .contains("mode exactly 0700")
    );
    let foreign = tilewright_source::Access {
        user: access.user + 1,
        ..access
    };
    assert!(
        favourites_view::private(foreign, None)
            .unwrap_err()
            .contains("current user's")
    );
    assert!(
        favourites_view::private(access, Some("nfs"))
            .unwrap_err()
            .contains("not on a checked local file system")
    );
    assert!(favourites_view::synced(true).is_ok());
    assert_eq!(
        favourites_view::synced(false).unwrap_err(),
        "the palettes directory could not be synced"
    );
}

/// The owner of `path`: this user, as `Access::new_file` names it.
fn owner(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().uid()
}

#[test]
fn starring_while_a_road_is_drawn_keeps_the_road() {
    let (temporary, mut app) = super::road::road_application(5, |_| {});
    let directory = palettes_directory(temporary.path(), &Favourites::default().encode());
    app.favourites = Some(Personal::open(SourceRoot::new(&directory)).unwrap());
    super::road::click(&mut app, [0, 1]).unwrap();
    super::road::click(&mut app, [3, 1]).unwrap();
    let points = app.tools.road().unwrap().points().to_vec();
    app.control(Control::FavouriteStar).unwrap();
    assert_eq!(app.tools.road().unwrap().points(), points.as_slice());
    assert_eq!(
        fs::read(directory.join(favourites_view::saved_name(1))).unwrap(),
        Favourites {
            terrain: vec![sand()],
            ..Favourites::default()
        }
        .encode()
    );
}
