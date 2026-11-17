// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The binary's read-only commands print exactly what the observe crate
//! returns and exit with its code. Their outputs are tested there.
use std::ffi::OsString;
use std::process::Command;
use tilewright_editor_observe::cli;
use tilewright_editor_testkit::fixture_world;

#[test]
fn the_binary_runs_the_read_only_commands() {
    let root = fixture_world("prototype-neutral");
    for (command, extra) in [
        ("overview", &["--json"][..]),
        ("tile", &["main", "0", "7", "0"][..]),
        ("catalog", &["--match", "gate"][..]),
        ("lint", &["--json"][..]),
        ("tile", &["nowhere", "0", "0", "0"][..]),
        ("tile", &["main", "zero", "0", "0"][..]),
    ] {
        let mut arguments: Vec<OsString> = vec![root.clone().into_os_string()];
        arguments.extend(extra.iter().map(OsString::from));
        let expected = cli::run(command, &arguments);
        let output = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
            .arg(command)
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::from(expected.code)),
            "{command} {extra:?}"
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected.stdout);
        assert_eq!(String::from_utf8(output.stderr).unwrap(), expected.stderr);
    }
    let usage = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("unknown")
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(
        String::from_utf8(usage.stderr)
            .unwrap()
            .contains("overview [--json] ROOT")
    );
}

/// The version string a build reports: the release version the packaging
/// scripts set, otherwise the development form.
const BUILD_VERSION: &str = match option_env!("TILEWRIGHT_EDITOR_RELEASE_VERSION") {
    Some(version) => version,
    None => "0.1.0-dev",
};

#[test]
fn version_and_the_mcp_server_report_the_build_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("version")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        format!(
            "tilewright-editor {BUILD_VERSION} toolkit={} renderer={}\n",
            tilewright_editor_document::TOOLKIT_COMPATIBILITY,
            tilewright_editor_preview::RENDERER_COMPATIBILITY
        )
    );
    let input =
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{{MCP_META}}}}}"#);
    let output = mcp(
        &[fixture_world("prototype-neutral").into_os_string()],
        &input,
    );
    let reply: serde_json::Value =
        serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim_end()).unwrap();
    assert_eq!(
        reply["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["version"],
        BUILD_VERSION
    );
}

/// The `_meta` every MCP request needs.
const MCP_META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

/// Runs `tilewright-editor mcp` with `arguments` and `input` on stdin.
fn mcp(arguments: &[OsString], input: &str) -> std::process::Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("mcp")
        .args(arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn the_binary_serves_mcp_over_stdio_and_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    let before = listing(directory.path());
    let input = [
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{{MCP_META}}}}}"#),
        format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{{MCP_META}}}}}"#),
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#.to_owned(),
        format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"overview","arguments":{{"limit":500}},{MCP_META}}}}}"#
        ),
        format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"render","arguments":{{"region":[0,0,4,3]}},{MCP_META}}}}}"#
        ),
        "not json".to_owned(),
    ]
    .join("\n");
    let output = mcp(&[root.clone().into_os_string()], &input);
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("tilewright-editor mcp: serving revision "),
        "{stderr}"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let replies: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let ids: Vec<&serde_json::Value> = replies.iter().map(|reply| &reply["id"]).collect();
    assert_eq!(
        ids,
        [
            &1.into(),
            &2.into(),
            &3.into(),
            &4.into(),
            &serde_json::Value::Null
        ]
    );
    assert_eq!(replies[4]["error"]["code"], -32700);
    // The overview is the CLI's report, cursors aside.
    let mut overview = replies[2]["result"]["structuredContent"].clone();
    let expected: serde_json::Value = serde_json::from_str(
        &cli::run(
            "overview",
            &["--json".into(), root.clone().into_os_string()],
        )
        .stdout,
    )
    .unwrap();
    strip_cursors(&mut overview);
    assert_eq!(overview, expected);
    // The image is the PNG `render` writes for the same region.
    let image = &replies[3]["result"]["content"][1];
    assert_eq!(image["mimeType"], "image/png");
    let png = decode_base64(image["data"].as_str().unwrap());
    assert_eq!(listing(directory.path()), before, "mcp wrote something");
    let out = directory.path().join("region.png");
    let written = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .args(["render", "--region", "0", "0", "4", "3", "--out"])
        .arg(&out)
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(written.status.code(), Some(0));
    assert_eq!(png, std::fs::read(&out).unwrap());
    let report = &replies[3]["result"]["structuredContent"]["png"];
    assert_eq!(report["bytes"], png.len());
    // Usage errors.
    for arguments in [
        vec![],
        vec![root.clone().into_os_string(), "extra".into()],
        vec!["--root".into()],
    ] {
        let output = mcp(&arguments, "");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty());
    }
}

fn strip_cursors(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(members) => {
            members.retain(|key, _| !key.ends_with("_cursor"));
            members.values_mut().for_each(strip_cursors);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(strip_cursors),
        _ => {}
    }
}

/// Standard base64 with padding, the encoding the MCP image uses.
fn decode_base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {byte}"),
        }
    };
    let mut bytes = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let data: Vec<u8> = chunk.iter().copied().filter(|byte| *byte != b'=').collect();
        let group = data.iter().enumerate().fold(0_u32, |group, (index, byte)| {
            group | value(*byte) << (18 - 6 * index)
        });
        for index in 0..data.len().saturating_sub(1) {
            bytes.push(u8::try_from((group >> (16 - 8 * index)) & 0xff).unwrap());
        }
    }
    bytes
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every entry under `root` with its bytes and modification time.
fn listing(
    root: &std::path::Path,
) -> std::collections::BTreeMap<std::path::PathBuf, (Vec<u8>, std::time::SystemTime)> {
    let mut entries = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            let bytes = if metadata.is_dir() {
                pending.push(entry.path());
                Vec::new()
            } else {
                std::fs::read(entry.path()).unwrap()
            };
            entries.insert(entry.path(), (bytes, metadata.modified().unwrap()));
        }
    }
    entries
}

/// A copy of the neutral sample in `directory/name` whose catalog is format
/// 2 with the resources in `project` labelled `project`: the MIT pixels
/// unchanged, only the label differs, in a temporary copy.
fn labelled(directory: &std::path::Path, name: &str, project: &[&str]) -> std::path::PathBuf {
    let root = directory.join(name);
    copy_tree(&fixture_world("prototype-neutral"), &root);
    tilewright_editor_testkit::label_project_art(&root, project);
    root
}

/// Width, height and RGBA8 pixels of a PNG.
fn decode_png(bytes: &[u8]) -> tilewright_editor_preview::ReferenceFrame {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    tilewright_editor_preview::ReferenceFrame {
        width: info.width,
        height: info.height,
        rgba8: pixels,
    }
}

#[test]
fn render_files_and_mcp_images_with_project_art_are_the_same_marked_png() {
    let directory = tempfile::tempdir().unwrap();
    let plain = labelled(directory.path(), "plain", &[]);
    let root = labelled(directory.path(), "project", &["grass", "sand"]);
    let render = |root: &std::path::Path, name: &str| {
        let out = directory.path().join(name);
        let output = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
            .args(["render", "--json", "--out"])
            .arg(&out)
            .arg(root)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        (std::fs::read(out).unwrap(), report)
    };
    let (unmarked, before) = render(&plain, "plain.png");
    let (marked, report) = render(&root, "project.png");
    let unmarked = decode_png(&unmarked);
    let rows = tilewright_editor_preview::mark::rows(unmarked.width);
    assert_eq!(
        decode_png(&marked),
        tilewright_editor_preview::mark::marked(&unmarked).unwrap()
    );
    assert_eq!(
        report["catalog"]["project_art"],
        serde_json::json!({"resources": 2})
    );
    assert_eq!(report["image"]["mark_rows"], rows);
    assert_eq!(report["image"]["height"], unmarked.height + rows);
    assert!(before["catalog"].get("project_art").is_none());
    assert!(before["image"].get("mark_rows").is_none());
    // MCP render returns the file's bytes, and the same report.
    let input = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"render","arguments":{{}},{MCP_META}}}}}"#
    );
    let output = mcp(&[root.clone().into_os_string()], &input);
    assert_eq!(output.status.code(), Some(0));
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let image = &reply["result"]["content"][1];
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(decode_base64(image["data"].as_str().unwrap()), marked);
    assert_eq!(reply["result"]["structuredContent"], report);
}
