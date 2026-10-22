//! UI-031: GPUI is used as published, or every change made to it is written in `PATCHED.md`.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The entries under 「修正の一覧」: one `###` heading each.
fn entries(doc: &str) -> Vec<&str> {
    let (_, list) = doc.split_once("## 修正の一覧").expect("修正の一覧");
    list.split("\n### ").skip(1).collect()
}

/// Lines of a manifest that point GPUI somewhere other than the registry.
fn manifest_forks(name: &str, text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim_start();
        let patch = l.starts_with("[patch") || l.starts_with("[replace");
        let moved = l.contains("gpui") && (l.contains("git =") || l.contains("path ="));
        if patch || moved {
            found.push(format!("{name}:{}: {l}", n + 1));
        }
    }
    found
}

/// The `gpui-pre*` packages of a lock file that are not a registry version with a checksum.
fn lock_forks(lock: &str) -> Vec<String> {
    let mut found = Vec::new();
    for block in lock.split("[[package]]") {
        let name = block
            .lines()
            .find_map(|l| l.strip_prefix("name = \""))
            .unwrap_or("");
        if !name.starts_with("gpui-pre") {
            continue;
        }
        let registry =
            block.contains("source = \"registry+https://github.com/rust-lang/crates.io-index\"");
        if !registry || !block.contains("checksum = ") {
            found.push(format!("Cargo.lock: {name} は registry の版ではない"));
        }
    }
    found
}

fn forks(root: &Path) -> Vec<String> {
    let mut manifests = vec![root.join("Cargo.toml")];
    for dir in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        manifests.push(dir.path().join("Cargo.toml"));
    }
    let mut found: Vec<String> = manifests
        .into_iter()
        .filter(|m| m.exists())
        .flat_map(|m| manifest_forks(&m.display().to_string(), &read(&m)))
        .collect();
    found.extend(lock_forks(&read(&root.join("Cargo.lock"))));
    found
}

#[test]
fn a_change_to_gpui_is_written_down_with_its_reason() {
    let root = root();
    let doc = read(&root.join("crates/xirius-ui/PATCHED.md"));
    let written = entries(&doc);
    let forks = forks(&root);
    assert!(
        forks.is_empty() || !written.is_empty(),
        "GPUI に差分があるのに PATCHED.md に 1 件も無い: {forks:#?}"
    );
    for e in &written {
        let name = e.lines().next().unwrap_or("");
        for key in ["- 何を:", "- なぜ:", "- 上流:", "- 戻せる条件:"] {
            assert!(e.contains(key), "PATCHED.md の「{name}」に「{key}」が無い");
        }
    }
}

#[test]
fn the_document_says_zero_while_there_is_no_fork() {
    let root = root();
    let doc = read(&root.join("crates/xirius-ui/PATCHED.md"));
    if forks(&root).is_empty() {
        assert!(entries(&doc).is_empty() && doc.contains("## いまの差分: 0 件"));
    } else {
        assert!(
            !doc.contains("## いまの差分: 0 件"),
            "差分があるのに 0 件と書いてある"
        );
    }
}

#[test]
fn the_checker_sees_each_kind_of_fork() {
    let patched = "[patch.crates-io]
gpui-pre = { path = \"../zed/crates/gpui\" }
";
    assert_eq!(manifest_forks("m", patched).len(), 2);
    let git = "gpui = { package = \"gpui-pre\", git = \"https://example.invalid/zed\" }
";
    assert_eq!(manifest_forks("m", git).len(), 1);
    let plain = "gpui = { package = \"gpui-pre\", version = \"=0.3.8\" }
";
    assert!(manifest_forks("m", plain).is_empty());

    let registry = "[[package]]
name = \"gpui-pre\"
version = \"0.3.8\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
checksum = \"ab\"
";
    assert!(lock_forks(registry).is_empty());
    let from_git = "[[package]]
name = \"gpui-pre\"
version = \"0.3.8\"
source = \"git+https://example.invalid/zed\"
";
    assert_eq!(lock_forks(from_git).len(), 1);
}

/// UI-033: the note in `GPUI.md` about which GPUI is used follows the version that is pinned.
/// The note lives in the published doc, not in a file the public repository leaves out.
#[test]
fn gpui_md_names_the_pinned_gpui_and_where_to_read() {
    let root = root();
    let manifest = read(&root.join("crates/xirius-ui/Cargo.toml"));
    let pinned = manifest
        .lines()
        .find(|l| l.contains("package = \"gpui-pre\""))
        .and_then(|l| l.split("version = \"").nth(1))
        .and_then(|v| v.split('"').next())
        .expect("gpui-pre の版");
    let doc = read(&root.join("crates/xirius-ui/GPUI.md"));
    assert!(
        doc.contains(&format!("`{pinned}`")),
        "GPUI.md の GPUI の版が Cargo.toml の {pinned} と違う"
    );
    for needle in [
        "ViewContext",
        "gpui_platform::application()",
        "docs/contexts.md",
        "examples/",
        "PATCHED.md",
    ] {
        assert!(doc.contains(needle), "GPUI.md に「{needle}」が無い");
    }
    for doc in ["GPUI.md", "PATCHED.md"] {
        assert!(root.join("crates/xirius-ui").join(doc).exists(), "{doc}");
    }
}
