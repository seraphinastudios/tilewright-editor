// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Reading proposal documents: every check that needs no world, the
//! bounds at their limits, and hostile documents. Steps are prepared but
//! never published, so this runs on every platform.

mod common;

use common::{AGENT, RECEIPT, Records, body, document, edit, fix_digests, sandbox, sha256, world};
use serde_json::{Value, json};
use tilewright_editor_proposal::{FORMAT, Proposal, encode_action, hex, limits};
use tilewright_editor_ui::{Action, RefusalCode};

/// A change to a proposal body.
type Change = Box<dyn Fn(&mut Value)>;

/// One prepared step: its action, sealed transaction, revisions and uid.
struct Prepared {
    action: Action,
    transaction: String,
    before: [u8; 32],
    after: [u8; 32],
    placed: Option<String>,
}

fn prepare(kind: usize) -> Prepared {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let before = session.history().document().revision();
    let action = edit(kind, before);
    session
        .dispatch(action.clone(), &mut Records::default())
        .unwrap();
    let pending = session.history().pending().unwrap();
    let transaction = pending.sealed().to_owned();
    let sealed: Value = serde_json::from_str(&transaction).unwrap();
    Prepared {
        placed: matches!(action, Action::Place { .. }).then(|| {
            sealed["operations"][0]["object"]["uid"]
                .as_str()
                .unwrap()
                .to_owned()
        }),
        after: pending.world().source_digest(),
        action,
        transaction,
        before,
    }
}

fn step(prepared: &Prepared) -> Value {
    json!({
        "action": encode_action(&prepared.action).unwrap(),
        "placed": prepared.placed,
        "transaction": prepared.transaction,
        "transaction_sha256": hex(&sha256(&prepared.transaction)),
        "before": hex(&prepared.before),
        "after": hex(&prepared.after),
    })
}

fn proposal_body(steps: &[Value]) -> Value {
    let first = steps
        .first()
        .map_or(Value::Null, |step| step["before"].clone());
    let last = steps
        .last()
        .map_or(Value::Null, |step| step["after"].clone());
    json!({
        "editor": "0.1.0",
        "base": first,
        "result": last,
        "catalog": hex(&[9; 32]),
        "actor": AGENT,
        "client": "an \u{1b}[2J label",
        "receipt": hex(&RECEIPT),
        "steps": steps,
    })
}

/// `count` copies of one step with made-up revisions that link: nothing
/// that needs no world can tell them from real ones.
fn chained(prepared: &Prepared, count: usize) -> Value {
    let revision = |index: usize| -> [u8; 32] {
        let mut revision = [0xab; 32];
        revision[..8].copy_from_slice(&(index as u64).to_be_bytes());
        revision
    };
    let steps = (0..count)
        .map(|index| {
            let mut action = prepared.action.clone();
            if let Action::Paint { expected, .. } = &mut action {
                *expected = revision(index);
            }
            step(&Prepared {
                action,
                transaction: prepared.transaction.clone(),
                before: revision(index),
                after: revision(index + 1),
                placed: None,
            })
        })
        .collect::<Vec<_>>();
    proposal_body(&steps)
}

fn code(bytes: &[u8]) -> Result<Proposal, RefusalCode> {
    Proposal::decode(bytes).map_err(|error| error.code())
}

#[test]
fn a_well_formed_document_is_read_exactly() {
    let paint = prepare(2);
    let place = prepare(6);
    let bytes = document(&proposal_body(&[step(&place)]));
    let proposal = Proposal::decode(&bytes).unwrap();
    assert_eq!(proposal.bytes(), bytes.as_slice());
    assert_eq!(proposal.steps()[0].action(), &place.action);
    assert_eq!(proposal.steps()[0].placed(), place.placed.as_deref());
    assert_eq!(proposal.steps()[0].transaction(), place.transaction);
    assert_eq!(proposal.base(), place.before);
    assert_eq!(proposal.result(), place.after);
    assert_eq!(proposal.catalog(), [9; 32]);
    assert_eq!(proposal.receipt(), RECEIPT);
    assert_eq!(proposal.actor().as_str(), AGENT);
    assert_eq!(proposal.editor(), "0.1.0");
    assert_eq!(proposal.client(), Some("an \u{1b}[2J label"));
    assert_eq!(
        proposal.client_display().as_deref(),
        Some("an \\u{1b}[2J label")
    );
    let digest = sha256(&body(&bytes).to_string());
    assert_eq!(proposal.body_sha256(), digest);
    assert_eq!(
        proposal.file_name(),
        format!("proposal-{}.json", hex(&digest))
    );
    let mut no_client = proposal_body(&[step(&paint)]);
    no_client["client"] = Value::Null;
    assert_eq!(
        Proposal::decode(&document(&no_client)).unwrap().client(),
        None
    );
}

#[test]
fn the_step_and_size_bounds_hold_exactly_at_their_limits() {
    let paint = prepare(2);
    assert!(code(&document(&chained(&paint, limits::STEPS))).is_ok());
    assert_eq!(
        code(&document(&chained(&paint, limits::STEPS + 1))).unwrap_err(),
        RefusalCode::ProposalBudget
    );
    assert_eq!(
        code(&document(&proposal_body(&[]))).unwrap_err(),
        RefusalCode::ProposalEmpty
    );
    // A file of exactly the size bound reads; one byte more is refused
    // before it is read.
    let steps = 17;
    let mut body = chained(&paint, steps);
    let padding = limits::BYTES - document(&body).len();
    let each = padding.div_ceil(steps);
    let mut left = padding;
    for step in body["steps"].as_array_mut().unwrap() {
        let pad = each.min(left);
        left -= pad;
        let padded = format!("{}{}", paint.transaction, " ".repeat(pad));
        assert!(padded.len() <= limits::TRANSACTION_BYTES);
        step["transaction"] = Value::from(padded);
    }
    fix_digests(&mut body);
    let bytes = document(&body);
    assert_eq!(bytes.len(), limits::BYTES);
    assert!(code(&bytes).is_ok());
    assert_eq!(
        code(&vec![b' '; limits::BYTES + 1]).unwrap_err(),
        RefusalCode::ProposalBudget
    );
    // One step's transaction: at most its bound.
    for (extra, ok) in [(0, true), (1, false)] {
        let mut body = proposal_body(&[step(&paint)]);
        let pad = limits::TRANSACTION_BYTES - paint.transaction.len() + extra;
        body["steps"][0]["transaction"] =
            Value::from(format!("{}{}", paint.transaction, " ".repeat(pad)));
        fix_digests(&mut body);
        let result = code(&document(&body));
        assert_eq!(result.is_ok(), ok, "{result:?}");
        if !ok {
            assert_eq!(result.unwrap_err(), RefusalCode::ProposalBudget);
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_check_that_needs_no_world_refuses_with_its_code() {
    let paint = prepare(2);
    let place = prepare(6);
    let no_op = prepare_no_op();
    let transaction = |body: &mut Value, change: &dyn Fn(&str) -> String| {
        let text = body["steps"][0]["transaction"].as_str().unwrap().to_owned();
        let changed = change(&text);
        assert_ne!(changed, text);
        body["steps"][0]["transaction"] = Value::from(changed);
    };
    let unsealed = {
        let sealed: Value = serde_json::from_str(&paint.transaction).unwrap();
        let mut unsealed = sealed.clone();
        unsealed.as_object_mut().unwrap().remove("base");
        unsealed.as_object_mut().unwrap().remove("result");
        unsealed.to_string()
    };
    let cases: Vec<(&str, RefusalCode, Change)> = vec![
        (
            "base",
            RefusalCode::ProposalChain,
            Box::new(|b| b["base"] = json!(hex(&[1; 32]))),
        ),
        (
            "result",
            RefusalCode::ProposalChain,
            Box::new(|b| b["result"] = json!(hex(&[1; 32]))),
        ),
        (
            "expected",
            RefusalCode::ProposalChain,
            Box::new(|b| b["steps"][0]["action"]["expected"] = json!(hex(&[1; 32]))),
        ),
        (
            "no change",
            RefusalCode::ProposalChain,
            Box::new(|b| {
                b["steps"][0]["after"] = b["steps"][0]["before"].clone();
                b["result"] = b["base"].clone();
            }),
        ),
        (
            "actor",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["actor"] = json!("agent:other")),
        ),
        (
            "invalid actor",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["actor"] = json!(" agent")),
        ),
        (
            "transaction actor",
            RefusalCode::ProposalInvalid,
            Box::new(move |b| transaction(b, &|t| t.replacen("agent:sandbox", "agent:sandbax", 1))),
        ),
        (
            "not a transaction",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["steps"][0]["transaction"] = json!("{}")),
        ),
        (
            "unsealed",
            RefusalCode::ProposalInvalid,
            Box::new(move |b| b["steps"][0]["transaction"] = json!(unsealed)),
        ),
        (
            "placed on paint",
            RefusalCode::ProposalInvalid,
            Box::new(|b| {
                b["steps"][0]["placed"] = json!(tilewright_editor_ui::uid::format_uid([1; 16]));
            }),
        ),
        (
            "editor version",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["editor"] = json!("0.1.0 beta")),
        ),
        (
            "editor version empty",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["editor"] = json!("")),
        ),
        (
            "editor version long",
            RefusalCode::ProposalBudget,
            Box::new(|b| b["editor"] = json!("1".repeat(limits::EDITOR_BYTES + 1))),
        ),
        (
            "client long",
            RefusalCode::ProposalBudget,
            Box::new(|b| b["client"] = json!("x".repeat(limits::CLIENT_BYTES + 1))),
        ),
        (
            "client type",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["client"] = json!(1)),
        ),
        (
            "unknown member",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["extra"] = json!(null)),
        ),
        (
            "unknown step member",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["steps"][0]["extra"] = json!(null)),
        ),
        (
            "missing step member",
            RefusalCode::ProposalInvalid,
            Box::new(|b| {
                b["steps"][0].as_object_mut().unwrap().remove("placed");
            }),
        ),
        (
            "receipt",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["receipt"] = json!("00")),
        ),
        (
            "steps type",
            RefusalCode::ProposalInvalid,
            Box::new(|b| b["steps"] = json!({})),
        ),
    ];
    for (name, expected, change) in cases {
        let mut body = proposal_body(&[step(&paint)]);
        change(&mut body);
        fix_digests(&mut body);
        assert_eq!(code(&document(&body)).unwrap_err(), expected, "{name}");
    }
    let place_cases: Vec<(&str, Change)> = vec![
        (
            "no uid",
            Box::new(|b| b["steps"][0]["placed"] = Value::Null),
        ),
        (
            "other uid",
            Box::new(|b| {
                b["steps"][0]["placed"] = json!(tilewright_editor_ui::uid::format_uid([1; 16]));
            }),
        ),
        (
            "not generated",
            Box::new(|b| b["steps"][0]["placed"] = json!("marker_1")),
        ),
        (
            "not generated, in its transaction too",
            Box::new(|b| {
                let assigned = b["steps"][0]["placed"].as_str().unwrap().to_owned();
                let text = b["steps"][0]["transaction"].as_str().unwrap();
                assert!(text.contains(&assigned));
                b["steps"][0]["transaction"] = json!(text.replace(&assigned, "marker_9"));
                b["steps"][0]["placed"] = json!("marker_9");
            }),
        ),
    ];
    for (name, change) in place_cases {
        let mut body = proposal_body(&[step(&place)]);
        change(&mut body);
        fix_digests(&mut body);
        assert_eq!(
            code(&document(&body)).unwrap_err(),
            RefusalCode::ProposalInvalid,
            "{name}"
        );
    }
    // Two placements of one uid.
    let mut second = Prepared {
        action: place.action.clone(),
        transaction: place.transaction.clone(),
        before: place.after,
        after: [0xcd; 32],
        placed: place.placed.clone(),
    };
    if let Action::Place { expected, .. } = &mut second.action {
        *expected = place.after;
    }
    let body = proposal_body(&[step(&place), step(&second)]);
    assert_eq!(
        code(&document(&body)).unwrap_err(),
        RefusalCode::ProposalInvalid
    );
    // A transaction that changes more files than one review shows, and
    // one at the limit.
    for (files, ok) in [
        (limits::CHANGED_FILES, true),
        (limits::CHANGED_FILES + 1, false),
    ] {
        let mut sealed: Value = serde_json::from_str(&paint.transaction).unwrap();
        let entries = |digest: &str| -> Value {
            (0..files)
                .map(|index| json!({"path": format!("planes/main/layers/0/{index}_0.jsonc"), "sha256": digest}))
                .collect()
        };
        sealed["base"] = entries(&"0".repeat(64));
        sealed["result"] = entries(&"1".repeat(64));
        let body = proposal_body(&[step(&Prepared {
            transaction: sealed.to_string(),
            ..prepare(2)
        })]);
        let result = code(&document(&body));
        assert_eq!(
            result.as_ref().err(),
            (!ok).then_some(&RefusalCode::ProposalBudget),
            "{files}"
        );
    }
    // A transaction that changes no file.
    let body = proposal_body(&[step(&Prepared {
        after: [0xcd; 32],
        ..no_op
    })]);
    assert_eq!(
        code(&document(&body)).unwrap_err(),
        RefusalCode::ProposalInvalid
    );
}

/// A move of an object onto its own tile: tests only, no changed file.
fn prepare_no_op() -> Prepared {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let before = session.history().document().revision();
    let location = session
        .history()
        .document()
        .world()
        .object("marker_1")
        .unwrap()
        .at
        .unwrap();
    let action = Action::Move {
        uid: "marker_1".into(),
        x: i64::from(location[0]),
        y: i64::from(location[1]),
        expected: before,
    };
    session
        .dispatch(action.clone(), &mut Records::default())
        .unwrap();
    let pending = session.history().pending().unwrap();
    assert_eq!(pending.changed().count(), 0);
    Prepared {
        action,
        transaction: pending.sealed().to_owned(),
        before,
        after: before,
        placed: None,
    }
}

#[test]
fn digests_and_the_one_canonical_encoding_are_enforced() {
    let paint = prepare(2);
    let bytes = document(&proposal_body(&[step(&paint)]));
    let text = String::from_utf8(bytes.clone()).unwrap();
    // The transaction digest, then the body digest.
    let mut body = body(&bytes);
    body["steps"][0]["transaction_sha256"] = json!(hex(&[0; 32]));
    assert_eq!(
        code(&document(&body)).unwrap_err(),
        RefusalCode::ProposalDigest
    );
    let digest_at = text.find("\"body_sha256\":\"").unwrap() + 15;
    let digit = if &text[digest_at..=digest_at] == "0" {
        "1"
    } else {
        "0"
    };
    let edited = format!("{}{digit}{}", &text[..digest_at], &text[digest_at + 1..]);
    assert_eq!(
        code(edited.as_bytes()).unwrap_err(),
        RefusalCode::ProposalDigest
    );
    let wrong_tag = text.replace(FORMAT, "tilewright.editor-proposal/2");
    let not_canonical = [
        format!("{text}\n"),
        format!(" {text}"),
        format!("\u{feff}{text}"),
        text.replacen("{\"body\":", "{ \"body\":", 1),
        text.replacen("{\"body\":", "{/**/\"body\":", 1),
        text.replacen("\"editor\":\"0.1.0\"", "\"editor\":\"0.1.\\u0030\"", 1),
        text.replacen("\"layer\":0", "\"layer\":-0", 1),
        text.replacen("\"layer\":0", "\"layer\":0e0", 1),
        // Members in another order.
        text.replacen(&format!(",\"format\":\"{FORMAT}\"}}"), "}", 1)
            .replacen(
                "{\"body\":",
                &format!("{{\"format\":\"{FORMAT}\",\"body\":"),
                1,
            ),
        // A duplicate key.
        text.replacen("{\"body\":", "{\"format\":1,\"body\":", 1),
        text.replacen("\"editor\":", "\"editor\":\"0.1.0\",\"editor\":", 1),
        wrong_tag,
        String::from_utf8_lossy(&[0xff, 0xfe]).into_owned(),
        "[]".into(),
        String::new(),
    ];
    for (index, other) in not_canonical.iter().enumerate() {
        assert_ne!(other, &text, "case {index}");
        assert_eq!(
            code(other.as_bytes()).unwrap_err(),
            RefusalCode::ProposalInvalid,
            "case {index}: {}",
            &other[..other.len().min(60)]
        );
    }
    assert_eq!(
        code(&[0xff, 0xfe, b'{']).unwrap_err(),
        RefusalCode::ProposalInvalid
    );
}

/// `SplitMix64`, for reproducible hostile documents.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap()
    }
}

#[test]
fn hostile_documents_never_panic_and_never_yield_an_unverified_proposal() {
    let paint = prepare(2);
    let place = prepare(6);
    let bytes = document(&proposal_body(&[step(&place)]));
    let other = document(&proposal_body(&[step(&paint)]));
    let mut random = Random(57);
    let alphabet = b"{}[]\":,0123456789abcdef-. \\nulltrue";
    let mut accepted = 0;
    for round in 0..4000 {
        let mut mutated = if round % 2 == 0 {
            bytes.clone()
        } else {
            other.clone()
        };
        for _ in 0..=random.below(4) {
            let at = random.below(mutated.len());
            match random.below(4) {
                0 => mutated[at] ^= 1 << random.below(8),
                1 => mutated[at] = alphabet[random.below(alphabet.len())],
                2 => {
                    mutated.remove(at);
                }
                _ => mutated.insert(at, alphabet[random.below(alphabet.len())]),
            }
        }
        // Some mutations keep the body and fix its digest: the checks
        // after the digest see them too.
        if round % 3 == 0
            && let Ok(mut value) = serde_json::from_slice::<Value>(&mutated)
            && value.get("body").is_some()
        {
            fix_digests(&mut value["body"]);
            mutated = document(&value["body"]);
        }
        if let Ok(proposal) = Proposal::decode(&mutated) {
            accepted += 1;
            assert_eq!(proposal.bytes(), mutated.as_slice());
            assert_eq!(Proposal::decode(proposal.bytes()).unwrap(), proposal);
        }
    }
    // The unchanged documents are among the few that verify.
    assert!(accepted < 400, "{accepted}");
    for size in [0, 1, 2, 100] {
        let noise: Vec<u8> = (0..size)
            .map(|_| u8::try_from(random.below(256)).unwrap())
            .collect();
        assert!(Proposal::decode(&noise).is_err());
    }
}
