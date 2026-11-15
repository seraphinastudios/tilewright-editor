// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `tilewright-editor sandbox`: the headless sandbox runner
//! (`docs/SANDBOX.md`). Like `prototype`, it is a deployment seam, started
//! only by the trusted provisioning helper (`tools/provision-sandbox.py`)
//! with the sandbox's fresh directories as open descriptors; it takes no
//! path at all.
use std::ffi::OsString;
use std::path::PathBuf;
use tilewright_editor_sandbox::{Sandbox, Settings};
use tilewright_editor_ui::Actor;
use tilewright_source::SourceRoot;

const OPTIONS: [&str; 6] = [
    "--world-fd",
    "--recovery-fd",
    "--proposals-fd",
    "--actor",
    "--receipt-sha256",
    "--client-label",
];

/// The runner's options, each given at most once.
struct Options {
    values: [Option<String>; OPTIONS.len()],
}

impl Options {
    fn read(arguments: &[OsString]) -> Result<Self, String> {
        let mut values: [Option<String>; OPTIONS.len()] = Default::default();
        let mut arguments = arguments.iter();
        while let Some(name) = arguments.next() {
            let index = OPTIONS
                .iter()
                .position(|option| name.to_str() == Some(option))
                .ok_or(
                    "unknown sandbox argument; start the runner with tools/provision-sandbox.py",
                )?;
            let value = arguments
                .next()
                .ok_or_else(|| format!("{} needs a value", OPTIONS[index]))?
                .to_str()
                .ok_or_else(|| format!("{} must be UTF-8", OPTIONS[index]))?;
            if values[index].replace(value.to_owned()).is_some() {
                return Err(format!("{} is given more than once", OPTIONS[index]));
            }
        }
        Ok(Self { values })
    }

    fn get(&self, name: &str) -> Option<&str> {
        let index = OPTIONS.iter().position(|option| *option == name)?;
        self.values[index].as_deref()
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.get(name).ok_or_else(|| {
            format!("missing {name}; start the runner with tools/provision-sandbox.py")
        })
    }

    /// A provisioned descriptor, as the path that reaches the directory
    /// it holds open.
    fn descriptor(&self, name: &str) -> Result<(u32, PathBuf), String> {
        let value: u32 = self
            .required(name)?
            .parse()
            .map_err(|_| format!("invalid {name}"))?;
        if value < 3 {
            return Err("provisioned descriptors must not be standard streams".into());
        }
        Ok((value, PathBuf::from(format!("/proc/self/fd/{value}"))))
    }
}

/// 64 lowercase hexadecimal digits.
fn digest(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let mut digest = [0; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        digest[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}

pub fn run(arguments: &[OsString]) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("the sandbox runner requires Linux; other platforms remain read-only".into());
    }
    let options = Options::read(arguments)?;
    let (world_fd, world) = options.descriptor("--world-fd")?;
    let (recovery_fd, recovery) = options.descriptor("--recovery-fd")?;
    let (proposals_fd, proposals) = options.descriptor("--proposals-fd")?;
    if world_fd == recovery_fd || world_fd == proposals_fd || recovery_fd == proposals_fd {
        return Err("provisioned descriptor numbers must be distinct".into());
    }
    let actor = Actor::new(options.required("--actor")?).map_err(|error| error.to_string())?;
    let receipt = digest(options.required("--receipt-sha256")?)
        .ok_or("--receipt-sha256 is 64 lowercase hexadecimal digits")?;
    let settings = Settings {
        client: options.get("--client-label").map(str::to_owned),
        receipt,
        editor: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let mut sandbox = Sandbox::open(
        &world,
        SourceRoot::new(&recovery),
        SourceRoot::new(&proposals),
        actor,
        settings,
    )
    .map_err(|error| error.to_string())?;
    match sandbox.serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        &mut std::io::stderr(),
    ) {
        // A client that closes the runner's output is done with it.
        Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(error.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, digest, run};
    use std::ffi::OsString;

    fn arguments(text: &[&str]) -> Vec<OsString> {
        text.iter().map(OsString::from).collect()
    }

    #[test]
    fn options_are_known_given_once_and_valued() {
        let options =
            Options::read(&arguments(&["--actor", "agent:a", "--world-fd", "5"])).unwrap();
        assert_eq!(options.get("--actor"), Some("agent:a"));
        assert_eq!(options.descriptor("--world-fd").unwrap().0, 5);
        assert!(options.descriptor("--recovery-fd").is_err());
        assert!(options.get("--client-label").is_none());
        for refused in [
            &["--root", "x"][..],
            &["--actor"],
            &["--actor", "agent:a", "--actor", "agent:b"],
            &["agent:a"],
        ] {
            assert!(Options::read(&arguments(refused)).is_err(), "{refused:?}");
        }
        for value in ["0", "2", "-1", "x", ""] {
            let options = Options::read(&arguments(&["--world-fd", value])).unwrap();
            assert!(options.descriptor("--world-fd").is_err(), "{value}");
        }
    }

    #[test]
    fn receipt_digests_are_64_lowercase_hexadecimal_digits() {
        let text = "0123456789abcdef".repeat(4);
        assert_eq!(digest(&text).unwrap()[..2], [0x01, 0x23]);
        for refused in [
            text.to_uppercase(),
            text[..63].to_owned(),
            format!("{text}0"),
            format!("+{}", &text[1..]),
        ] {
            assert_eq!(digest(&refused), None, "{refused}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_runner_refuses_to_start_without_the_provisioned_descriptors() {
        let digest = "0".repeat(64);
        for refused in [
            &[][..],
            &[
                "--world-fd",
                "3",
                "--recovery-fd",
                "3",
                "--proposals-fd",
                "4",
                "--actor",
                "agent:a",
                "--receipt-sha256",
                &digest,
            ],
            &[
                "--world-fd",
                "3",
                "--recovery-fd",
                "4",
                "--proposals-fd",
                "5",
                "--actor",
                "agent:a",
                "--receipt-sha256",
                "00",
            ],
            &[
                "--world-fd",
                "3",
                "--recovery-fd",
                "4",
                "--proposals-fd",
                "5",
                "--actor",
                " agent:a",
                "--receipt-sha256",
                &digest,
            ],
            &[
                "--world-fd",
                "3",
                "--recovery-fd",
                "4",
                "--proposals-fd",
                "5",
                "--receipt-sha256",
                &digest,
            ],
        ] {
            assert!(run(&arguments(refused)).is_err(), "{refused:?}");
        }
    }
}
