// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render`'s image caps at their boundaries: the side, the pixels, the
//! PNG's bytes, and that a frame past the side or pixel cap is refused
//! before anything is encoded. One test in its own binary, because the
//! encoder it injects keeps its state in statics.
mod common;

use common::{call, edit, fixture_copy};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tilewright_editor_mcp::{
    MAXIMUM_IMAGE_BYTES, MAXIMUM_IMAGE_PIXELS, MAXIMUM_IMAGE_SIDE, Server,
};
use tilewright_editor_preview::ReferenceFrame;

/// How many bytes the test encoder returns, and how often it ran.
static PNG_BYTES: AtomicUsize = AtomicUsize::new(16);
static ENCODED: AtomicUsize = AtomicUsize::new(0);

#[allow(clippy::unnecessary_wraps)]
fn sized_encode(_: &ReferenceFrame) -> Option<(Vec<u8>, [u8; 32])> {
    ENCODED.fetch_add(1, Ordering::SeqCst);
    Some((vec![0; PNG_BYTES.load(Ordering::SeqCst)], [1; 32]))
}

fn failing_encode(_: &ReferenceFrame) -> Option<(Vec<u8>, [u8; 32])> {
    ENCODED.fetch_add(1, Ordering::SeqCst);
    None
}

fn render(server: &mut Server, region: [i64; 4], scale: u32) -> common::Called {
    call(server, "render", &json!({"region": region, "scale": scale}))
}

fn size(called: &common::Called) -> (u64, u64) {
    let image = &called.structured()["image"];
    (
        image["width"].as_u64().unwrap(),
        image["height"].as_u64().unwrap(),
    )
}

fn refused_code(called: &common::Called) -> String {
    let text = called.error_text();
    let rest = text.split_once(" failed: ").unwrap().1;
    rest.split_once(": ").unwrap().0.to_owned()
}

#[test]
fn every_render_cap_holds_at_its_boundary_and_is_checked_before_encoding() {
    // The neutral sample with a 64 × 64 view of 32-pixel tiles. Its art
    // stays inside the regions' tiles, so a region's image is centred on
    // the region and as wide and tall as its tiles' pixels.
    let (_directory, root) = fixture_copy("prototype-neutral");
    edit(&root, "preview.jsonc", |catalog| {
        catalog["view"]["bounds"] = json!([0, 0, 64, 64]);
    });
    let side = u64::from(MAXIMUM_IMAGE_SIDE);
    let mut server = Server::open(&root, "0.0.0-test", sized_encode);

    // Pixels: 2048 × 512 is exactly the cap; one row of tiles more is not.
    let at = render(&mut server, [0, 0, 64, 16], 1);
    assert!(!at.is_error(), "{}", at.line.get(..700).unwrap_or(&at.line));
    assert_eq!(size(&at), (side, 512));
    assert_eq!(side * 512, MAXIMUM_IMAGE_PIXELS);
    let before = ENCODED.load(Ordering::SeqCst);
    let over = render(&mut server, [0, 0, 64, 17], 1);
    assert_eq!(refused_code(&over), "output.budget");
    assert!(
        over.error_text().contains("1048576 pixels"),
        "{}",
        over.error_text()
    );
    assert_eq!(
        ENCODED.load(Ordering::SeqCst),
        before,
        "encoded past the pixel cap"
    );

    // Side: 2048 pixels wide at scale 2 passes; 2112 does not, though its
    // pixels are far under the pixel cap.
    let at = render(&mut server, [0, 0, 32, 1], 2);
    assert_eq!(size(&at), (side, 64));
    let before = ENCODED.load(Ordering::SeqCst);
    let over = render(&mut server, [0, 0, 33, 1], 2);
    assert_eq!(refused_code(&over), "output.budget");
    assert_eq!(
        ENCODED.load(Ordering::SeqCst),
        before,
        "encoded past the side cap"
    );

    // The side cap holds for the height too.
    assert_eq!(size(&render(&mut server, [0, 0, 1, 32], 2)), (64, side));
    assert_eq!(
        refused_code(&render(&mut server, [0, 0, 1, 33], 2)),
        "output.budget"
    );

    // Scale 4: 2048 × 512 again at the cap, and one tile row more past it.
    let at = render(&mut server, [0, 0, 16, 4], 4);
    assert_eq!(size(&at), (side, 512));
    assert_eq!(
        refused_code(&render(&mut server, [0, 0, 16, 5], 4)),
        "output.budget"
    );

    // PNG bytes: exactly the cap passes and is sent whole; one byte more
    // is refused with the PNG's own message.
    PNG_BYTES.store(MAXIMUM_IMAGE_BYTES, Ordering::SeqCst);
    let at = render(&mut server, [0, 0, 2, 2], 1);
    assert_eq!(at.structured()["png"]["bytes"], MAXIMUM_IMAGE_BYTES);
    let data = at.result()["content"][1]["data"].as_str().unwrap();
    assert_eq!(data.len(), MAXIMUM_IMAGE_BYTES * 4 / 3);
    assert!(data.bytes().all(|byte| byte == b'A'));
    PNG_BYTES.store(MAXIMUM_IMAGE_BYTES + 1, Ordering::SeqCst);
    let over = render(&mut server, [0, 0, 2, 2], 1);
    assert_eq!(refused_code(&over), "output.budget");
    assert!(
        over.error_text().contains("196608 bytes"),
        "{}",
        over.error_text()
    );
    assert_eq!(over.result()["content"].as_array().map(Vec::len), Some(1));

    // An encoder that fails is not a budget refusal.
    let mut failing = Server::open(&root, "0.0.0-test", failing_encode);
    let failed = render(&mut failing, [0, 0, 2, 2], 1);
    assert_eq!(refused_code(&failed), "render.failed");
    assert!(failed.error_text().contains("could not be encoded"));
    let _: Value = serde_json::from_str(&failed.line).unwrap();
}
