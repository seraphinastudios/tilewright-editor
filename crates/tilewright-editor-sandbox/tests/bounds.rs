// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The proposal bounds as the runner meets them, and random request lines.
#![cfg(target_os = "linux")]

mod common;

use common::{ACTOR, Layout, move_marker, ok, refused};
use serde_json::Value;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_proposal::{ProposalStore, limits};
use tilewright_editor_sandbox::Sandbox;
use tilewright_editor_ui::{Actor, Session};
use tilewright_source::SourceRoot;

/// Inverses kept in memory: the bound test accepts hundreds of steps.
#[derive(Default)]
struct Records(usize);
impl RecoveryStore for Records {
    fn retain(&mut self, _inverse: &str) -> Result<(), String> {
        self.0 += 1;
        Ok(())
    }
}

fn in_memory(layout: &Layout) -> common::Driver<Records> {
    let document = WorldDocument::open_prototype(&layout.path("world")).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let session = Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new(ACTOR).unwrap(),
    );
    let store = ProposalStore::open(SourceRoot::new(&layout.path("proposals"))).unwrap();
    common::Driver {
        sandbox: Sandbox::new(session, Records::default(), store, Layout::settings(None)).unwrap(),
    }
}

#[test]
fn a_submit_past_the_step_bound_is_refused_and_the_session_stays_usable() {
    let layout = Layout::new(|_| {});
    let mut driver = in_memory(&layout);
    for step in 0..=limits::STEPS {
        ok(driver.edit(move_marker(if step % 2 == 0 { 4 } else { 3 }, 2)));
        ok(driver.simple("accept"));
    }
    let refusal = driver.simple("submit");
    refused(&refusal, "proposal.budget");
    assert_eq!(refusal["state"]["undo"], limits::STEPS + 1);
    assert!(layout.files("proposals").is_empty());
    // The agent undoes one step and submits the rest.
    ok(driver.simple("undo"));
    ok(driver.simple("accept"));
    let submitted = ok(driver.simple("submit"));
    assert_eq!(submitted["proposal"]["steps"], limits::STEPS);
    assert_eq!(layout.files("proposals").len(), 1);
}

/// A small deterministic generator (xorshift64*), so a failure replays.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap()).unwrap()
    }
}

#[test]
fn random_request_lines_each_get_exactly_one_well_formed_reply() {
    let layout = Layout::new(|_| {});
    let mut driver = layout.open(Some("fuzz"));
    let base = driver.revision();
    let seeds: Vec<String> = [
        r#"{"request":"status"}"#.to_owned(),
        r#"{"request":"review"}"#.to_owned(),
        r#"{"request":"accept"}"#.to_owned(),
        r#"{"request":"cancel"}"#.to_owned(),
        r#"{"request":"undo"}"#.to_owned(),
        r#"{"request":"redo"}"#.to_owned(),
        r#"{"request":"submit"}"#.to_owned(),
        format!(
            r#"{{"request":"edit","action":{{"kind":"move","uid":"marker_1","x":4,"y":2,"expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"paint_stroke","plane":"main","layer":0,"grid":"floor","cells":[[0,1],[1,1]],"proto":"sand","expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"fill_terrain","plane":"main","layer":0,"grid":"floor","x":1,"y":1,"proto":"sand","contiguous":true,"expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"scatter_terrain","plane":"main","layer":0,"grid":"floor","cells":[[0,1],[1,1]],"palette":"speckle","seed":1234,"expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"adjust_heights","plane":"main","layer":0,"cells":[[0,1],[1,1]],"amount":-8,"expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"flatten_heights","plane":"main","layer":0,"cells":[[0,1],[1,1]],"target":3,"expected":"{base}"}}}}"#
        ),
        format!(
            r#"{{"request":"edit","action":{{"kind":"set_field","uid":"marker_1","field":"label","value":{{"a":[1,2.5,null,true]}},"expected":"{base}"}}}}"#
        ),
    ]
    .into();
    let alphabet: &[u8] = b"{}[]\":,\\ 0123456789-.eE+abcnrtufl\r\t\x00\x1b\x7f\xc3\xa9\xff";
    let mut random = Random(0x5eed_5a2d_b0c5_0001);
    let mut input = Vec::new();
    let mut lines = 0;
    for _ in 0..1500 {
        let mut line = seeds[random.below(seeds.len())].clone().into_bytes();
        for _ in 0..random.below(4) {
            match random.below(4) {
                0 if !line.is_empty() => {
                    let at = random.below(line.len());
                    line.truncate(at);
                }
                1 => {
                    let at = random.below(line.len() + 1);
                    line.insert(at, alphabet[random.below(alphabet.len())]);
                }
                2 if !line.is_empty() => {
                    let at = random.below(line.len());
                    line[at] = alphabet[random.below(alphabet.len())];
                }
                _ => {
                    let at = random.below(line.len() + 1);
                    let other = seeds[random.below(seeds.len())].as_bytes();
                    line.splice(at..at, other[..random.below(other.len())].iter().copied());
                }
            }
        }
        line.retain(|byte| *byte != b'\n');
        input.extend_from_slice(&line);
        input.push(b'\n');
        lines += 1;
    }
    let (replies, _) = driver.serve(&input);
    assert_eq!(replies.len(), lines);
    let mut succeeded = 0;
    for line in &replies {
        let reply = common::reply(line);
        if reply["ok"] == Value::Bool(true) {
            succeeded += 1;
        }
    }
    // The mix is not all refusals: some edits, accepts and undos ran.
    assert!(succeeded > 50, "{succeeded}");
    assert!(ok(driver.simple("status"))["state"]["revision"].is_string());
}
