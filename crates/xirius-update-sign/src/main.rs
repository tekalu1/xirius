//! The key of the app's updates and the signature of its manifest (UI-282).
//!
//! A release signs a small manifest (`Xirius-<version>-win-x64-update.json`) that names the
//! version, the installer, its SHA-256 and the channel. The private key is 32 bytes written as 64
//! hex characters; the public key is what goes into `crates/xirius-session/src/update.rs`
//! (`UPDATE_KEY`). The signature file is 64 bytes as 128 hex characters, named
//! `<manifest>.sig`. The steps are in `docs/update-signing.md`.
//!
//! ```text
//! xirius-update-sign keygen --out <private key file>
//! xirius-update-sign pubkey --key <private key file>
//! xirius-update-sign sign --key <private key file> --file <manifest> --out <manifest>.sig
//! xirius-update-sign verify --app <update.rs> --manifest <manifest> --sig <manifest>.sig --file <setup.exe> [--version <version>]
//! ```

use std::path::Path;
use std::process::ExitCode;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
使い方:
  xirius-update-sign keygen --out <秘密鍵のファイル>
  xirius-update-sign pubkey --key <秘密鍵のファイル>
  xirius-update-sign sign --key <秘密鍵のファイル> --file <manifest> --out <署名のファイル>
  xirius-update-sign verify --app <update.rs> --manifest <manifest> --sig <署名> --file <setup.exe> [--version <版>]";

/// The key a build without one has (`update.rs`).
const PLACEHOLDER: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("xirius-update-sign: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(USAGE.into());
    };
    let rest = &args[1..];
    match command {
        "keygen" => {
            let out = value(rest, "--out").ok_or(USAGE)?;
            keygen(Path::new(&out))
        }
        "pubkey" => {
            let key = value(rest, "--key").ok_or(USAGE)?;
            let key = read_key(Path::new(&key))?;
            println!("{}", hex(&key.verifying_key().to_bytes()));
            Ok(())
        }
        "sign" => {
            let key = value(rest, "--key").ok_or(USAGE)?;
            let file = value(rest, "--file").ok_or(USAGE)?;
            let out = value(rest, "--out").ok_or(USAGE)?;
            sign(Path::new(&key), Path::new(&file), Path::new(&out))
        }
        "verify" => {
            let app = value(rest, "--app").ok_or(USAGE)?;
            let manifest = value(rest, "--manifest").ok_or(USAGE)?;
            let sig = value(rest, "--sig").ok_or(USAGE)?;
            let file = value(rest, "--file").ok_or(USAGE)?;
            let version = value(rest, "--version");
            verify(
                Path::new(&app),
                Path::new(&manifest),
                Path::new(&sig),
                Path::new(&file),
                version.as_deref(),
            )
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("{other} はありません\n{USAGE}")),
    }
}

fn value(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

/// Makes a key and writes its private half; prints the public half.
fn keygen(out: &Path) -> Result<(), String> {
    if inside_repository(out) {
        return Err(format!(
            "{} はリポジトリの中です（秘密鍵はリポジトリの外に書いてください）",
            out.display()
        ));
    }
    if let Some(dir) = out.parent()
        && !dir.as_os_str().is_empty()
        && !dir.is_dir()
    {
        return Err(format!("{} がありません", dir.display()));
    }
    let key = SigningKey::generate(&mut OsRng);
    // create_new: a key is never written over one that is there (even one that appeared after
    // a look).
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| format!("{} に書けません: {e}", out.display()))?;
    use std::io::Write;
    file.write_all(format!("{}\n", hex(&key.to_bytes())).as_bytes())
        .map_err(|e| format!("{} に書けません: {e}", out.display()))?;
    println!("秘密鍵: {}", out.display());
    println!("公開鍵: {}", hex(&key.verifying_key().to_bytes()));
    println!("公開鍵を crates/xirius-session/src/update.rs の UPDATE_KEY に写してください。");
    Ok(())
}

/// Is `path` inside a git working tree (a `.git` at it or above it)?
fn inside_repository(path: &Path) -> bool {
    let start = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => dir.to_path_buf(),
        // A bare name (`--out update.key`) is written in the current folder.
        None => std::env::current_dir().unwrap_or_default(),
    };
    let mut dir = std::fs::canonicalize(&start).unwrap_or(start);
    loop {
        if dir.join(".git").exists() {
            return true;
        }
        if !dir.pop() {
            return false;
        }
    }
}

/// Signs `file` (the manifest) and writes the signature.
fn sign(key: &Path, file: &Path, out: &Path) -> Result<(), String> {
    let key = read_key(key)?;
    let bytes = std::fs::read(file).map_err(|e| format!("{} を読めません: {e}", file.display()))?;
    let signature = key.sign(&bytes);
    std::fs::write(out, format!("{}\n", hex(&signature.to_bytes())))
        .map_err(|e| format!("{} に書けません: {e}", out.display()))?;
    println!("署名: {}", out.display());
    Ok(())
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    file: String,
    sha256: String,
}

/// Checks that the manifest's signature is the app's key's, and that the installer is the one
/// the manifest names (UI-282, the release job's check).
fn verify(
    app: &Path,
    manifest_path: &Path,
    sig_path: &Path,
    setup: &Path,
    version: Option<&str>,
) -> Result<(), String> {
    let key = app_key(app)?;
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("{} を読めません: {e}", manifest_path.display()))?;
    let sig = std::fs::read_to_string(sig_path)
        .map_err(|e| format!("{} を読めません: {e}", sig_path.display()))?;
    let sig = unhex::<64>(&sig).ok_or("署名のファイルを読み取れません")?;
    let sig = Signature::from_bytes(&sig);
    key.verify_strict(&manifest_bytes, &sig)
        .map_err(|_| "署名が一致しません".to_string())?;

    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "manifest を読み取れません".to_string())?;
    if let Some(version) = version
        && manifest.version != version
    {
        return Err(format!(
            "manifest のバージョン {} がタグの {version} と違います",
            manifest.version
        ));
    }
    let name = setup
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if manifest.file != name {
        return Err(format!(
            "manifest のファイル名 {} が {name} と違います",
            manifest.file
        ));
    }
    let bytes =
        std::fs::read(setup).map_err(|e| format!("{} を読めません: {e}", setup.display()))?;
    let got = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !got.eq_ignore_ascii_case(&manifest.sha256) {
        return Err("SHA-256 が一致しません".into());
    }
    println!(
        "ok: {} {}（公開鍵 {}）",
        manifest.version,
        manifest.file,
        hex(&key.to_bytes())
    );
    Ok(())
}

/// The public key written in the app's `update.rs`.
fn app_key(path: &Path) -> Result<VerifyingKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let line = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("pub const UPDATE_KEY: &str ="))
        .ok_or_else(|| format!("{} に UPDATE_KEY がありません", path.display()))?;
    let hex = line
        .trim()
        .trim_end_matches(';')
        .trim()
        .trim_matches('"')
        .trim();
    if hex.eq_ignore_ascii_case(PLACEHOLDER) {
        return Err("アプリの UPDATE_KEY が仮のままです（公開鍵を入れてください）".into());
    }
    let bytes = unhex::<32>(hex).ok_or("UPDATE_KEY を読み取れません")?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| "UPDATE_KEY が公開鍵ではありません".to_string())
}

fn read_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} を読めません: {e}", path.display()))?;
    let bytes = unhex::<32>(&text)
        .ok_or_else(|| format!("{} は 64 文字の 16 進数ではありません", path.display()))?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    // A byte position is a character position only for ASCII (UI-282).
    if !text.is_ascii() || text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The path a `keygen --out` of this tool's test writes.
#[cfg(test)]
fn test_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("xirius-update-sign-{name}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_key_signs_and_its_public_half_checks() {
        let dir = test_path("key");
        std::fs::create_dir_all(&dir).unwrap();
        let private = dir.join("update.key");
        let _ = std::fs::remove_file(&private);
        keygen(&private).unwrap();
        assert!(keygen(&private).is_err(), "a key is not overwritten");
        let key = read_key(&private).unwrap();
        let public = hex(&key.verifying_key().to_bytes());

        let file = dir.join("manifest.json");
        let out = dir.join("manifest.json.sig");
        std::fs::write(&file, b"the manifest").unwrap();
        sign(&private, &file, &out).unwrap();
        let sig = std::fs::read_to_string(&out).unwrap();
        let sig = unhex::<64>(&sig).unwrap();
        let sig = Signature::from_bytes(&sig);
        let public = unhex::<32>(&public).unwrap();
        let public = VerifyingKey::from_bytes(&public).unwrap();
        public.verify_strict(b"the manifest", &sig).unwrap();
        assert!(public.verify_strict(b"another", &sig).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// UI-282: a key is never written inside the repository.
    #[test]
    fn a_key_is_not_written_inside_the_repository() {
        let dir = test_path("repo");
        let repo = dir.join("repo");
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let err = keygen(&repo.join("update.key")).unwrap_err();
        assert!(err.contains("リポジトリの中"), "{err}");
        assert!(!repo.join("update.key").exists());
        // A bare name is written in the current folder, which is inside the repository here.
        assert!(
            inside_repository(Path::new("update.key")),
            "a bare name is looked for from the current folder"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn verify_checks_the_app_key_the_version_and_the_installer() {
        use std::io::Write;
        let dir = test_path("verify");
        std::fs::create_dir_all(&dir).unwrap();
        let private = dir.join("update.key");
        let _ = std::fs::remove_file(&private);
        keygen(&private).unwrap();
        let key = read_key(&private).unwrap();
        let public = hex(&key.verifying_key().to_bytes());

        let setup = dir.join("Xirius-1.0.1-win-x64-setup.exe");
        std::fs::write(&setup, b"the installer").unwrap();
        let sha = Sha256::digest(b"the installer")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let manifest = dir.join("Xirius-1.0.1-win-x64-update.json");
        std::fs::write(
            &manifest,
            format!(
                "{{\"version\":\"1.0.1\",\"file\":\"{}\",\"sha256\":\"{sha}\",\"channel\":\"stable\"}}\n",
                setup.file_name().unwrap().to_string_lossy()
            ),
        )
        .unwrap();
        let sig = dir.join("manifest.sig");
        sign(&private, &manifest, &sig).unwrap();

        let app = dir.join("update.rs");
        let text = format!("pub const UPDATE_KEY: &str = \"{public}\";\n");
        let mut f = std::fs::File::create(&app).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        assert_eq!(verify(&app, &manifest, &sig, &setup, Some("1.0.1")), Ok(()));
        // A tag that is not the manifest's.
        assert!(verify(&app, &manifest, &sig, &setup, Some("9.9.9")).is_err());
        // An installer that is not the manifest's.
        std::fs::write(&setup, b"another installer").unwrap();
        assert!(verify(&app, &manifest, &sig, &setup, Some("1.0.1")).is_err());
        // The placeholder key is refused before anything is checked.
        std::fs::write(
            &app,
            format!("pub const UPDATE_KEY: &str = \"{PLACEHOLDER}\";\n"),
        )
        .unwrap();
        let err = verify(&app, &manifest, &sig, &setup, None).unwrap_err();
        assert!(err.contains("仮のまま"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// UI-282: a `.sig` with non-ASCII characters is not a signature (and does not panic).
    #[test]
    fn a_non_ascii_signature_is_not_read() {
        assert_eq!(unhex::<4>("ああああ"), None);
        assert_eq!(unhex::<4>("00112233"), Some([0x00, 0x11, 0x22, 0x33]));
    }
}
