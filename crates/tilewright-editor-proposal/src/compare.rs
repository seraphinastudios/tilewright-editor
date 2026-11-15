// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Whether a step prepared again is the step that was recorded.
//!
//! The recorded transaction carries the actor of the sandbox session (the
//! agent); the same Action prepared again in the person's session carries
//! the person's actor. The actor is a top-level member of the transaction
//! text and nothing else depends on it: `base` and `result` are digests of
//! file bytes, and the files never hold the actor. So two transactions are
//! the same step when every top-level member but `actor` is the same value:
//! the format, the title, the operations with every precondition (`test`,
//! `expect`), and the digests of every file consulted and written. Values
//! are compared by the strict reader's canonical form, so whitespace and
//! comments do not matter but member order, every value and every number's
//! spelling do. Each side must have its actor, as a string.
use std::{collections::BTreeSet, fmt};
use tilewright_jsonc as jsonc;

/// How two transactions differ, without any of their text: the top-level
/// members that differ and, for `base` and `result`, how many file
/// entries; for `operations`, the first operation that differs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Divergence {
    /// A side could not be read as a JSON object with a string actor.
    pub unreadable: bool,
    /// Top-level members other than `actor` that differ or are on one
    /// side only, by name when it is a transaction member, else `other`.
    pub members: Vec<&'static str>,
    /// File entries of `base` that differ or are on one side only.
    pub base_files: usize,
    /// File entries of `result` that differ or are on one side only.
    pub result_files: usize,
    /// The index of the first operation that differs, if they do.
    pub operation: Option<usize>,
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unreadable {
            return f.write_str("a transaction is not a JSON object with an actor");
        }
        write!(f, "members differ: {}", self.members.join(", "))?;
        if let Some(operation) = self.operation {
            write!(f, "; first differing operation {}", operation + 1)?;
        }
        if self.base_files > 0 {
            write!(f, "; base differs in {} file(s)", self.base_files)?;
        }
        if self.result_files > 0 {
            write!(f, "; result differs in {} file(s)", self.result_files)?;
        }
        Ok(())
    }
}

/// The names this comparison reports; anything else is `other`.
const MEMBERS: [&str; 6] = ["format", "title", "base", "result", "operations", "custom"];

/// Compares a recorded transaction with one prepared again, ignoring only
/// the `actor` member's value; see the module documentation.
///
/// # Errors
/// Returns how they differ.
pub fn compare_transactions(recorded: &str, prepared: &str) -> Result<(), Divergence> {
    let limits = jsonc::Limits::default();
    let unreadable = || Divergence {
        unreadable: true,
        ..Divergence::default()
    };
    let (Ok(recorded), Ok(prepared)) = (
        jsonc::Document::parse(recorded, &limits),
        jsonc::Document::parse(prepared, &limits),
    ) else {
        return Err(unreadable());
    };
    let members = |document: &jsonc::Document| -> Option<Vec<(String, String)>> {
        let members = document.root().as_object()?;
        document.root().get("actor")?.as_str()?;
        Some(
            members
                .iter()
                .filter(|member| member.key != "actor")
                .map(|member| (member.key.clone(), document.canonical(&member.value)))
                .collect(),
        )
    };
    let (Some(left), Some(right)) = (members(&recorded), members(&prepared)) else {
        return Err(unreadable());
    };
    if left == right {
        return Ok(());
    }
    let mut divergence = Divergence::default();
    let name = |key: &str| {
        MEMBERS
            .into_iter()
            .find(|known| *known == key)
            .unwrap_or("other")
    };
    // Positions matter too: the same members in another order differ.
    for index in 0..left.len().max(right.len()) {
        match (left.get(index), right.get(index)) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => {
                for member in [a, b].into_iter().flatten() {
                    let member = name(&member.0);
                    if !divergence.members.contains(&member) {
                        divergence.members.push(member);
                    }
                }
            }
        }
    }
    let root = |document: &'_ jsonc::Document, key: &str| document.root().get(key).cloned();
    for (key, count) in [
        ("base", &mut divergence.base_files),
        ("result", &mut divergence.result_files),
    ] {
        *count = differing_entries(
            &recorded,
            root(&recorded, key).as_ref(),
            &prepared,
            root(&prepared, key).as_ref(),
        );
    }
    let operations = |document: &jsonc::Document| -> Vec<String> {
        document
            .root()
            .get("operations")
            .and_then(jsonc::Value::as_array)
            .map(|items| items.iter().map(|item| document.canonical(item)).collect())
            .unwrap_or_default()
    };
    let (a, b) = (operations(&recorded), operations(&prepared));
    divergence.operation = (0..a.len().max(b.len())).find(|&index| a.get(index) != b.get(index));
    Err(divergence)
}

/// File entries (`{"path": ..., "sha256": ...}` items) of two digest
/// lists that differ or are on one side only.
fn differing_entries(
    left: &jsonc::Document,
    a: Option<&jsonc::Value>,
    right: &jsonc::Document,
    b: Option<&jsonc::Value>,
) -> usize {
    let entries = |document: &jsonc::Document, value: Option<&jsonc::Value>| -> BTreeSet<String> {
        value
            .and_then(jsonc::Value::as_array)
            .map(|items| items.iter().map(|item| document.canonical(item)).collect())
            .unwrap_or_default()
    };
    entries(left, a)
        .symmetric_difference(&entries(right, b))
        .count()
}
