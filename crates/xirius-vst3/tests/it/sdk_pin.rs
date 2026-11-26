//! The VST3 SDK is 3.8.0 or later (MIT) and pinned as a submodule; no copy of an older SDK
//! is in the repository (LIC-003).

use std::path::PathBuf;
use std::process::Command;

/// The commit of the tag `v3.8.0_build_66` in github.com/steinbergmedia/vst3sdk.
const PINNED: &str = "9fad9770f2ae8542ab1a548a68c1ad1ac690abe0";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `git <args>` at the repository's root, or `None` outside a git checkout.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn the_bindings_come_from_sdk_3_8_or_later() {
    use vst3::Steinberg::Vst::{SDKVersionMajor, SDKVersionMinor};
    assert!((SDKVersionMajor, SDKVersionMinor) >= (3, 8));
}

#[test]
fn the_sdk_is_a_submodule_at_3_8_0_and_no_older_copy_is_tracked() {
    let modules = std::fs::read_to_string(root().join(".gitmodules")).expect(".gitmodules");
    assert!(modules.contains("path = third_party/vst3sdk"));
    assert!(modules.contains("url = https://github.com/steinbergmedia/vst3sdk.git"));
    let Some(entry) = git(&["ls-files", "-s", "third_party/vst3sdk"]) else {
        eprintln!("not a git checkout; the submodule is not checked");
        return;
    };
    // A gitlink (mode 160000) at the pinned commit, not files copied in.
    let mut fields = entry.split_whitespace();
    assert_eq!(fields.next(), Some("160000"), "{entry}");
    assert_eq!(fields.next(), Some(PINNED), "{entry}");

    // No SDK headers or sources anywhere else in the tree.
    let files = git(&["ls-files"]).expect("ls-files");
    let copies: Vec<&str> = files
        .lines()
        .filter(|f| {
            let f = f.to_ascii_lowercase();
            f.contains("pluginterfaces/")
                || f.ends_with("ivstaudioprocessor.h")
                || f.ends_with("ipluginbase.h")
                || f.ends_with("vstversion.h")
        })
        .collect();
    assert!(copies.is_empty(), "copies of the SDK: {copies:?}");
}
