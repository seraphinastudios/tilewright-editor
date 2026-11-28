// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A person's favourites file: its strict, closed, bounded shape, refused
//! as a whole with a reason that never quotes it; its exact bytes as
//! written back; and which entries the neutral sample's catalog grants.
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::favourites::{
    FORMAT, Favourite, Favourites, MAXIMUM_BYTES, MAXIMUM_ENTRIES, MAXIMUM_VALUE_BYTES,
};
use tilewright_editor_ui::tools::TerrainChoice;

fn terrain(grid: &str, value: &str) -> TerrainChoice {
    TerrainChoice {
        grid: grid.into(),
        prototype: value.into(),
    }
}

fn sample() -> Favourites {
    Favourites {
        terrain: vec![terrain("floor", "sand"), terrain("floor", "grass")],
        scatter: vec!["speckle".into()],
        objects: vec!["stone".into(), "lamp".into()],
    }
}

#[test]
fn the_written_bytes_are_fixed_and_read_back_as_the_same_lists() {
    let bytes = sample().encode();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            "{{\n  \"format\": \"{FORMAT}\",\n  \"terrain\": [\n    {{\"grid\": \"floor\", \"value\": \"sand\"}},\n    {{\"grid\": \"floor\", \"value\": \"grass\"}}\n  ],\n  \"scatter\": [\n    \"speckle\"\n  ],\n  \"objects\": [\n    \"stone\",\n    \"lamp\"\n  ]\n}}\n"
        )
    );
    assert_eq!(Favourites::decode(&bytes).unwrap(), sample());
    let empty = Favourites::default().encode();
    assert_eq!(
        String::from_utf8(empty.clone()).unwrap(),
        format!(
            "{{\n  \"format\": \"{FORMAT}\",\n  \"terrain\": [],\n  \"scatter\": [],\n  \"objects\": []\n}}\n"
        )
    );
    assert_eq!(Favourites::decode(&empty).unwrap(), Favourites::default());
    // Escaped text survives the round trip.
    let quoted = Favourites {
        objects: vec!["a\"b\\c \u{e9}".into()],
        ..Favourites::default()
    };
    assert_eq!(Favourites::decode(&quoted.encode()).unwrap(), quoted);
}

#[test]
fn absent_lists_are_empty_and_the_bounds_themselves_are_accepted() {
    let minimal = format!("{{\"format\": \"{FORMAT}\"}}");
    assert_eq!(
        Favourites::decode(minimal.as_bytes()).unwrap(),
        Favourites::default()
    );
    let longest = "v".repeat(MAXIMUM_VALUE_BYTES);
    let full = Favourites {
        terrain: (0..MAXIMUM_ENTRIES)
            .map(|index| terrain(&longest, &format!("{index:0>128}")))
            .collect(),
        scatter: (0..MAXIMUM_ENTRIES)
            .map(|index| format!("{index:0>64}"))
            .collect(),
        objects: (0..MAXIMUM_ENTRIES)
            .map(|index| format!("{index:0>128}"))
            .collect(),
    };
    let bytes = full.encode();
    assert!(bytes.len() <= MAXIMUM_BYTES, "{}", bytes.len());
    assert_eq!(Favourites::decode(&bytes).unwrap(), full);
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_malformed_shape_is_refused_as_a_whole_without_quoting_the_file() {
    // Every case carries this text where the file allows one, so a reason
    // that quoted the file would show it.
    let secret = "secretvalue";
    let head = format!("\"format\": \"{FORMAT}\"");
    let many = |count: usize| {
        (0..count)
            .map(|index| format!("\"{secret}{index}\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    let cases: Vec<(String, &str)> = vec![
        (
            format!("{{{head}, \"objects\": [\"{secret}\"],}}"),
            "jsonc.trailing_comma",
        ),
        (
            format!("{{{head}, // {secret}\n}}"),
            "jsonc.too_many_comments",
        ),
        (
            format!("{{{head}, \"objects\": [\"{secret}\"], \"objects\": []}}"),
            "jsonc.duplicate_key",
        ),
        (format!("\u{feff}{{{head}}}"), "jsonc.byte_order_mark"),
        (
            format!("{{{head}}} [\"{secret}\"]"),
            "jsonc.trailing_content",
        ),
        (format!("[\"{secret}\"]"), "not a JSON object"),
        (format!("{{\"format\": \"{secret}\"}}"), "the format is not"),
        (
            format!("{{\"objects\": [\"{secret}\"]}}"),
            "the format is not",
        ),
        (
            format!("{{{head}, \"{secret}\": []}}"),
            "a member other than",
        ),
        (
            format!("{{{head}, \"objects\": \"{secret}\"}}"),
            "objects is not a list",
        ),
        (
            format!("{{{head}, \"scatter\": {{\"{secret}\": 1}}}}"),
            "scatter is not a list",
        ),
        (
            format!("{{{head}, \"terrain\": [\"{secret}\"]}}"),
            "terrain entry 1 is not an object",
        ),
        (
            format!("{{{head}, \"terrain\": [{{\"grid\": \"{secret}\"}}]}}"),
            "terrain entry 1 does not have exactly grid and value",
        ),
        (
            format!(
                "{{{head}, \"terrain\": [{{\"grid\": \"a\", \"value\": \"b\", \"{secret}\": 1}}]}}"
            ),
            "terrain entry 1 does not have exactly grid and value",
        ),
        (
            format!("{{{head}, \"terrain\": [{{\"grid\": \"a\", \"name\": \"{secret}\"}}]}}"),
            "terrain entry 1 does not have exactly grid and value",
        ),
        (
            format!("{{{head}, \"terrain\": [{{\"grid\": 1, \"value\": \"{secret}\"}}]}}"),
            "terrain entry 1's grid is not a string",
        ),
        (
            format!("{{{head}, \"terrain\": [{{\"grid\": \"{secret}\", \"value\": \"\"}}]}}"),
            "terrain entry 1's value is not 1 to 128 bytes",
        ),
        (
            format!("{{{head}, \"objects\": [\"{secret}\", null]}}"),
            "objects entry 2 is not a string",
        ),
        (
            format!("{{{head}, \"objects\": [\"{secret}\\u0007\"]}}"),
            "objects entry 1 contains a control character",
        ),
        (
            format!(
                "{{{head}, \"objects\": [\"{secret}\", \"{}\"]}}",
                "o".repeat(129)
            ),
            "a string is longer than 128 bytes",
        ),
        (
            format!(
                "{{{head}, \"scatter\": [\"{secret}\", \"{}\"]}}",
                "s".repeat(65)
            ),
            "scatter entry 2 is not 1 to 64 bytes",
        ),
        (
            format!("{{{head}, \"objects\": [{}]}}", many(MAXIMUM_ENTRIES + 1)),
            "a list has more than 32 entries",
        ),
        (
            format!("{{{head}, \"objects\": [\"{secret}\", \"{secret}\"]}}"),
            "objects entry 2 repeats an earlier entry",
        ),
        (
            format!("{{{head}, \"scatter\": [\"{secret}\", \"b\", \"{secret}\"]}}"),
            "scatter entry 3 repeats an earlier entry",
        ),
        (
            format!(
                "{{{head}, \"terrain\": [{{\"grid\": \"a\", \"value\": \"{secret}\"}}, {{\"value\": \"{secret}\", \"grid\": \"a\"}}]}}"
            ),
            "terrain entry 2 repeats an earlier entry",
        ),
        (
            format!("{{{head}, \"objects\": [[\"{secret}\"]]}}"),
            "objects entry 1 is not a string",
        ),
        (
            format!("{{{head}, \"terrain\": [{{\"grid\": [\"{secret}\"], \"value\": \"b\"}}]}}"),
            "jsonc.depth_limit",
        ),
        (
            format!("{{{head}, \"objects\": [\"{secret}\"]"),
            "jsonc.unexpected_end",
        ),
    ];
    for (text, reason) in &cases {
        let refused = Favourites::decode(text.as_bytes()).unwrap_err().to_string();
        assert!(refused.contains(reason), "{text}: {refused}");
        assert!(!refused.contains(secret), "{text}: {refused}");
    }
    // Not UTF-8, and larger than the bound (even when it would parse).
    let refused = Favourites::decode(b"{\"format\": \"\xff\"}").unwrap_err();
    assert_eq!(refused.to_string(), "the file is not UTF-8");
    let mut large = format!("{{{head}}}").into_bytes();
    large.resize(MAXIMUM_BYTES + 1, b' ');
    let refused = Favourites::decode(&large).unwrap_err();
    assert_eq!(refused.to_string(), "the file is larger than 64 KiB");
    let mut fits = format!("{{{head}}}").into_bytes();
    fits.resize(MAXIMUM_BYTES, b' ');
    assert_eq!(Favourites::decode(&fits).unwrap(), Favourites::default());
}

#[test]
fn the_catalog_decides_which_entries_are_granted_and_the_file_keeps_the_rest() {
    let document = WorldDocument::open(&fixture_world("prototype-neutral")).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let granted = [
        Favourite::Terrain(terrain("floor", "sand")),
        Favourite::Terrain(terrain("floor", "grass")),
        Favourite::Scatter("speckle".into()),
        Favourite::Object("stone".into()),
        Favourite::Object("gate".into()),
    ];
    for favourite in &granted {
        assert!(favourite.granted(&catalog), "{favourite:?}");
    }
    let refused = [
        Favourite::Terrain(terrain("floor", "lava")),
        Favourite::Terrain(terrain("wall", "sand")),
        Favourite::Scatter("dunes".into()),
        Favourite::Object("tree".into()),
        // A terrain value is not a prototype Place puts down.
        Favourite::Object("sand".into()),
    ];
    for favourite in &refused {
        assert!(!favourite.granted(&catalog), "{favourite:?}");
    }
    // A file naming them all still loads, in its order.
    let mixed = Favourites {
        terrain: vec![terrain("floor", "lava"), terrain("floor", "sand")],
        scatter: vec!["dunes".into(), "speckle".into()],
        objects: vec!["tree".into(), "stone".into()],
    };
    let read = Favourites::decode(&mixed.encode()).unwrap();
    assert_eq!(
        read.entries(false),
        vec![
            Favourite::Terrain(terrain("floor", "lava")),
            Favourite::Terrain(terrain("floor", "sand")),
            Favourite::Scatter("dunes".into()),
            Favourite::Scatter("speckle".into()),
        ]
    );
    assert_eq!(
        read.entries(true),
        vec![
            Favourite::Object("tree".into()),
            Favourite::Object("stone".into())
        ]
    );
}

#[test]
fn starring_unstarring_and_raising_keep_each_list_bounded_and_ordered() {
    let mut favourites = Favourites::default();
    let sand = Favourite::Terrain(terrain("floor", "sand"));
    let grass = Favourite::Terrain(terrain("floor", "grass"));
    assert!(favourites.toggle(&sand).unwrap());
    assert!(favourites.toggle(&grass).unwrap());
    assert_eq!(favourites.position(&grass), Some(1));
    assert!(favourites.raise(&grass));
    assert_eq!(favourites.position(&grass), Some(0));
    // The first moves no further; an unstarred value does not move.
    assert!(!favourites.raise(&grass));
    assert!(!favourites.raise(&Favourite::Object("stone".into())));
    assert!(!favourites.toggle(&grass).unwrap());
    assert_eq!(favourites.terrain, vec![terrain("floor", "sand")]);
    for index in 0..MAXIMUM_ENTRIES {
        assert!(
            favourites
                .toggle(&Favourite::Object(format!("p{index}")))
                .unwrap()
        );
    }
    let refused = favourites
        .toggle(&Favourite::Object("one more".into()))
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "a favourites list holds at most 32 entries"
    );
    // Another list still has room, and a full one can still unstar.
    assert!(
        favourites
            .toggle(&Favourite::Scatter("speckle".into()))
            .unwrap()
    );
    assert!(!favourites.toggle(&Favourite::Object("p0".into())).unwrap());
    assert_eq!(favourites.objects.len(), MAXIMUM_ENTRIES - 1);
}
