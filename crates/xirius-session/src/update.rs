//! Whether a newer Xirius is out, and getting it into place (UI-262, UI-282).
//!
//! The asking: the latest release on GitHub, asked once a day at launch while
//! `app.update.check` is on, and by `app.update.check` whenever it is run. With
//! `app.update.beta` on, the list is read instead, so a pre-release can be had too.
//!
//! The getting: a release carries a small manifest (`Xirius-<version>-win-x64-update.json`) that
//! names the version, the installer, its SHA-256 and the channel, and a signature of the
//! manifest. The app checks the signature with the key built into it, checks that the manifest's
//! version is the release's tag and is newer than this program, that the channel fits, that
//! `SHA256SUMS.txt` has the same hash, and then that the installer has that hash. Only then is it
//! kept, under the app's folder. The window starts it with `/S /XRUN` once Xirius has left (the
//! installer waits for Xirius to close), and checks it once more right before that. A build with
//! the placeholder key (`UPDATE_KEY`, until the user makes one) installs nothing: the panel opens
//! the release's page in the browser, as it did before.
//!
//! The asking is a [`Fetch`], so a test gives its own and never reaches the network; the app's
//! runs the system's `curl` (Windows has one in System32 since 10 1803), which needs no TLS
//! crate. A failure says nothing on the screen: the next launch asks again.
//!
//! What was asked last is kept in `update.json` under the app's folder
//! ([`crate::recent::root`]): `{"checked_ms": …, "beta": …, "latest": {version, url, notes}}`.
//! A checked installer is kept under `update/<version>/` so the next launch can still offer it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// This program's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The repository (LIC-016).
pub const REPOSITORY: &str = "https://github.com/tekalu1/xirius";
/// Where a release's files are downloaded from; the tag and the name make the URL (UI-282).
pub const DOWNLOAD: &str = "https://github.com/tekalu1/xirius/releases/download";
/// Where the latest release is read.
pub const LATEST: &str = "https://api.github.com/repos/tekalu1/xirius/releases/latest";
/// Where every release is read, for the beta channel.
pub const RELEASES: &str = "https://api.github.com/repos/tekalu1/xirius/releases?per_page=30";
/// How long one answer is used before asking again.
pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// The pref that lets pre-releases be taken (UI-282).
pub const BETA_PREF: &str = "app.update.beta";
/// The file the release lists its files' hashes in.
pub const SUMS: &str = "SHA256SUMS.txt";
/// Where the downloaded installers are kept under the app's folder.
pub const DOWNLOAD_DIR: &str = "update";
/// Said by the installer when it could not put the new version in; the next start reads it.
pub const FAILED: &str = "update-failed.txt";
/// The channel a manifest names.
pub const STABLE: &str = "stable";
pub const BETA: &str = "beta";

/// The ed25519 key the app checks the manifest with (UI-282). It is the placeholder until the
/// user makes one: `docs/update-signing.md` has the steps, and the key goes here.
pub const PLACEHOLDER_KEY: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
/// The key this build checks with.
pub const UPDATE_KEY: &str = "b35c1648160e639968a72564e7e08675e219ee1b7a68ffb3e9b6e8b7083dbaa1";

/// Is `key_hex` still the placeholder (so a build with it installs nothing)?
pub fn is_placeholder_key(key_hex: &str) -> bool {
    key_hex.trim().eq_ignore_ascii_case(PLACEHOLDER_KEY)
}

/// Is this build's key still the placeholder (so it installs nothing)?
pub fn key_is_placeholder() -> bool {
    UPDATE_KEY == PLACEHOLDER_KEY
}

/// May this build install an update at all?
pub fn can_install() -> bool {
    !key_is_placeholder()
}

/// Why an update could not be checked.
pub const NO_KEY: &str = "このアプリには更新の鍵がありません";
/// Why a downloaded installer is thrown away.
pub const BAD_SIGNATURE: &str = "署名が一致しません";
/// Why a downloaded installer is thrown away.
pub const BAD_SUM: &str = "SHA-256 が一致しません";

/// The longest the system's `curl` is given for a small file.
const TIMEOUT_SECONDS: &str = "10";
/// The longest the system's `curl` is given for the installer.
const DOWNLOAD_TIMEOUT_SECONDS: &str = "900";
/// The most a small file (the API's answer, the manifest, its signature, the sums) may be.
const SMALL_MAX: u64 = 1 << 20;
/// The most the installer may be when the release does not say its size.
const SETUP_MAX: u64 = 512 << 20;
/// Changes shown at most.
const NOTES_MAX: usize = 8;

/// Reads a URL: its body, or why it could not.
pub type Fetch = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// Writes a URL to a file (at most `max` bytes), saying how many bytes are there now (all of
/// them at the end).
pub type Download =
    Arc<dyn Fn(&str, &Path, u64, &mut dyn FnMut(u64, u64)) -> Result<(), String> + Send + Sync>;

/// Starts the installer; tests put their own in.
pub type Spawn = Arc<dyn Fn(&Install) -> Result<(), String> + Send + Sync>;

/// The programs an install of Xirius puts in its folder (`scripts/release.py`, `xirius.nsi`).
pub const PROGRAMS: &[&str] = &[
    "xirius-ui.exe",
    "xirius.exe",
    "xirius-engine.exe",
    "xirius-plugin-host.exe",
    "xirius-plugin-scan.exe",
];

/// A file of a release, as GitHub names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    /// Its size in bytes, as GitHub says; 0 when it does not say.
    #[serde(default)]
    pub size: u64,
}

/// A release on GitHub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// `1.0.1` (the tag without its `v`).
    pub version: String,
    /// The release's page, where it is downloaded.
    pub url: String,
    /// The bullets of its description.
    pub notes: Vec<String>,
    /// A pre-release (UI-282).
    #[serde(default)]
    pub prerelease: bool,
    /// The files it carries.
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// The file named `name`.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The manifest of this version.
    pub fn manifest(&self) -> Option<&Asset> {
        self.asset(&manifest_name(&self.version))
    }

    /// The manifest's signature.
    pub fn signature(&self) -> Option<&Asset> {
        self.asset(&manifest_signature_name(&self.version))
    }

    /// The installer of this version.
    pub fn setup(&self) -> Option<&Asset> {
        self.asset(&setup_name(&self.version))
    }
}

/// What the last asking found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub checked_ms: u64,
    /// Asked with `app.update.beta` on.
    #[serde(default)]
    pub beta: bool,
    pub latest: Option<Release>,
}

impl Checked {
    /// The release when it is newer than `current`.
    pub fn newer_than(&self, current: &str) -> Option<&Release> {
        self.latest
            .as_ref()
            .filter(|r| is_newer(&r.version, current))
    }
}

/// What a signed manifest says (UI-282). Its bytes are what the signature covers, so the same
/// bytes are what a release carries and what a test makes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// `1.0.1`.
    pub version: String,
    /// The installer's name, `Xirius-<version>-win-x64-setup.exe`.
    pub file: String,
    /// The installer's SHA-256, as lowercase hex.
    pub sha256: String,
    /// `stable` or `beta`.
    pub channel: String,
    /// The oldest Xirius that may install this (UI-282: a signed but old interim version cannot
    /// be handed out to a program older than this).
    pub min_version: String,
}

/// The system's `curl`, without a window.
pub fn curl() -> Fetch {
    Arc::new(|url: &str| {
        let program = system_curl();
        let mut c = std::process::Command::new(&program);
        c.args([
            "-sSfL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            &SMALL_MAX.to_string(),
            "--max-time",
            TIMEOUT_SECONDS,
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: Xirius/{CURRENT}"),
            "--",
            url,
        ])
        .stdin(std::process::Stdio::null());
        hide_window(&mut c);
        let out = c
            .output()
            .map_err(|e| format!("{} を実行できません: {e}", program.display()))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    })
}

/// The system's `curl`, writing the installer to a file and saying how much is there.
pub fn curl_download() -> Download {
    Arc::new(
        |url: &str, out: &Path, max: u64, progress: &mut dyn FnMut(u64, u64)| {
            let program = system_curl();
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("{} を作れません: {e}", dir.display()))?;
            }
            let mut c = std::process::Command::new(&program);
            c.args([
                "-sSfL",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-filesize",
                &max.to_string(),
                "--max-time",
                DOWNLOAD_TIMEOUT_SECONDS,
                "--retry",
                "2",
                "-o",
            ])
            .arg(out)
            .arg("--")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
            hide_window(&mut c);
            let mut child = c
                .spawn()
                .map_err(|e| format!("{} を実行できません: {e}", program.display()))?;
            let mut last = 0;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => {}
                    Err(e) => return Err(format!("ダウンロードできません: {e}")),
                }
                std::thread::sleep(Duration::from_millis(100));
                if let Ok(m) = std::fs::metadata(out) {
                    let got = m.len();
                    if got != last {
                        last = got;
                        progress(got, 0);
                    }
                }
            };
            let mut why = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut stderr, &mut why);
            }
            if !status.success() {
                let why = why.trim();
                return Err(if why.is_empty() {
                    format!("ダウンロードできません（{status}）")
                } else {
                    format!("ダウンロードできません（{why}）")
                });
            }
            let got = std::fs::metadata(out).map_or(last, |m| m.len());
            progress(got, 0);
            Ok(())
        },
    )
}

/// Keeps a program a shell starts from opening a window.
fn hide_window(c: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = c;
}

/// The `curl` of the system folder on Windows (not one a folder on `PATH` puts first).
pub fn system_curl() -> PathBuf {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = PathBuf::from(root).join("System32").join("curl.exe");
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("curl")
}

/// The release in GitHub's answer for the latest release.
pub fn parse(text: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(text).ok()?;
    release_of(&v)
}

/// The releases in GitHub's answer for the list.
pub fn parse_releases(text: &str) -> Option<Vec<Release>> {
    let v: Value = serde_json::from_str(text).ok()?;
    Some(
        v.as_array()?
            .iter()
            .filter(|r| !r["draft"].as_bool().unwrap_or(false))
            .filter_map(release_of)
            .collect(),
    )
}

fn release_of(v: &Value) -> Option<Release> {
    let tag = v["tag_name"].as_str()?.trim();
    let version = tag.strip_prefix(['v', 'V']).unwrap_or(tag).to_string();
    parse_version(&version)?;
    let notes = v["body"]
        .as_str()
        .unwrap_or("")
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .map(|n| n.trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .take(NOTES_MAX)
        .collect();
    let assets = v["assets"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a["name"].as_str()?.to_string(),
                        size: a["size"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version,
        url: v["html_url"].as_str().unwrap_or(REPOSITORY).to_string(),
        notes,
        prerelease: v["prerelease"].as_bool().unwrap_or(false),
        assets,
    })
}

/// The highest release of `list`, pre-releases included when `beta`; drafts are never in it.
pub fn pick(list: &[Release], beta: bool) -> Option<Release> {
    list.iter()
        .filter(|r| beta || !r.prerelease)
        .max_by(|a, b| {
            if is_newer(&a.version, &b.version) {
                std::cmp::Ordering::Greater
            } else if is_newer(&b.version, &a.version) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .cloned()
}

/// `Xirius-<version>-win-x64-setup.exe` (the name a release must carry, UI-282).
pub fn setup_name(version: &str) -> String {
    format!("Xirius-{version}-win-x64-setup.exe")
}

/// `Xirius-<version>-win-x64-update.json`: what the signature covers (UI-282).
pub fn manifest_name(version: &str) -> String {
    format!("Xirius-{version}-win-x64-update.json")
}

/// The manifest's signature: the manifest's name with `.sig` after it.
pub fn manifest_signature_name(version: &str) -> String {
    format!("{}.sig", manifest_name(version))
}

/// Where a release's file is: the tag and the name make it, so the API's URLs are not used.
pub fn release_url(version: &str, name: &str) -> String {
    format!("{DOWNLOAD}/v{version}/{name}")
}

/// `1.2.3` as numbers, and its pre-release's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    numbers: Vec<u64>,
    /// `beta.1` of `1.0.0-beta.1`; empty for a release.
    pre: Vec<String>,
}

fn parse_version(v: &str) -> Option<Version> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    // A build's `+…` does not take part in the order, but its characters are semver's.
    let (v, build) = match v.split_once('+') {
        Some((v, build)) => (v, Some(build)),
        None => (v, None),
    };
    if let Some(build) = build
        && !valid_identifiers(build)
    {
        return None;
    }
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (v, None),
    };
    let numbers: Vec<u64> = core.split('.').map(parse_number).collect::<Option<_>>()?;
    if numbers.is_empty() {
        return None;
    }
    let pre = match pre {
        Some(pre) if valid_pre_release(pre) => pre.split('.').map(str::to_string).collect(),
        Some(_) => return None,
        None => Vec::new(),
    };
    Some(Version { numbers, pre })
}

/// A number of a version: digits, without a leading zero (semver).
fn parse_number(part: &str) -> Option<u64> {
    if part.is_empty()
        || !part.bytes().all(|b| b.is_ascii_digit())
        || (part.len() > 1 && part.starts_with('0'))
    {
        return None;
    }
    part.parse().ok()
}

fn valid_identifiers(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn valid_pre_release(text: &str) -> bool {
    valid_identifiers(text)
        && text.split('.').all(|part| {
            !(part.len() > 1 && part.starts_with('0') && part.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Is `candidate` a later version than `current`? A release is later than its pre-releases;
/// between two pre-releases the parts after the dash are compared (`beta.2` before `beta.10`,
/// `beta` before `rc.1`), as semver does.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(a), Some(b)) = (parse_version(candidate), parse_version(current)) else {
        return false;
    };
    let n = a.numbers.len().max(b.numbers.len());
    let numbers = |v: &Version| {
        let mut parts = v.numbers.clone();
        parts.resize(n, 0);
        parts
    };
    match numbers(&a).cmp(&numbers(&b)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => false,
            (false, true) => false,
            (true, false) => true,
            (false, false) => pre_is_newer(&a.pre, &b.pre),
        },
    }
}

fn pre_is_newer(a: &[String], b: &[String]) -> bool {
    for (x, y) in a.iter().zip(b) {
        if x == y {
            continue;
        }
        return match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x > y,
            (Ok(_), Err(_)) => false,
            (Err(_), Ok(_)) => true,
            (Err(_), Err(_)) => x > y,
        };
    }
    a.len() > b.len()
}

/// The asking and its record in a folder.
#[derive(Clone)]
pub struct UpdateCheck {
    path: PathBuf,
    fetch: Fetch,
}

impl UpdateCheck {
    pub fn in_dir(root: &Path, fetch: Fetch) -> UpdateCheck {
        UpdateCheck {
            path: root.join("update.json"),
            fetch,
        }
    }

    /// The app's folder the answer is kept in.
    pub fn root(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// What was found the last time.
    pub fn stored(&self) -> Option<Checked> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// No answer from the last day for this channel.
    pub fn due(&self, now_ms: u64, beta: bool) -> bool {
        self.stored().is_none_or(|c| {
            c.beta != beta || now_ms.saturating_sub(c.checked_ms) >= DAY_MS || c.checked_ms > now_ms
        })
    }

    /// Asks now and keeps the answer.
    pub fn ask(&self, now_ms: u64, beta: bool) -> Result<Checked, String> {
        let body = (self.fetch)(if beta { RELEASES } else { LATEST })?;
        let latest = if beta {
            let list = parse_releases(&body).ok_or("リリースの一覧を読み取れません")?;
            pick(&list, true).ok_or("リリースの情報を読み取れません")?
        } else {
            parse(&body).ok_or("リリースの情報を読み取れません")?
        };
        let checked = Checked {
            checked_ms: now_ms,
            beta,
            latest: Some(latest),
        };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string(&checked).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, text).map_err(|e| e.to_string())?;
        Ok(checked)
    }

    /// What a launch does: asks when `enabled` and no answer is a day old (a failure is said
    /// nowhere), and gives the release newer than `current` that is known. Off, it asks nothing
    /// and shows nothing.
    pub fn at_launch(
        &self,
        enabled: bool,
        beta: bool,
        now_ms: u64,
        current: &str,
    ) -> Option<Release> {
        if !enabled {
            return None;
        }
        let checked = if self.due(now_ms, beta) {
            self.ask(now_ms, beta).ok().or_else(|| self.stored())
        } else {
            self.stored()
        };
        checked.and_then(|c| c.newer_than(current).cloned())
    }
}

/// How a download is going (UI-282): what it is doing and how many bytes are here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub got: u64,
    /// 0 when the release did not say.
    pub total: u64,
}

impl Progress {
    /// How far it is, 0 to 100; None when the size is not known.
    pub fn percent(&self) -> Option<u64> {
        (self.total > 0).then(|| (self.got.saturating_mul(100) / self.total).min(100))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Downloading,
    Verifying,
    Ready,
    Failed(String),
}

/// An installer that was downloaded and checked, ready to run when Xirius leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installer {
    pub path: PathBuf,
    pub version: String,
    pub manifest: Manifest,
    /// The exact bytes the signature covers.
    pub manifest_bytes: Vec<u8>,
    pub signature: String,
    pub sums: String,
}

impl Installer {
    /// Checks the installer again, right before it is started: the signature, the manifest, the
    /// hash in `SHA256SUMS.txt` and the file itself (UI-282).
    pub fn verify(&self) -> Result<(), String> {
        self.verify_with(UPDATE_KEY)
    }

    /// The same check with a given key (a test makes its own).
    pub fn verify_with(&self, key_hex: &str) -> Result<(), String> {
        verify_signature(&self.manifest_bytes, &self.signature, key_hex)?;
        let manifest: Manifest = serde_json::from_slice(&self.manifest_bytes)
            .map_err(|_| "manifest を読み取れません".to_string())?;
        if manifest.version != self.version {
            return Err("manifest のバージョンが一致しません".into());
        }
        if !is_newer(&manifest.version, CURRENT) {
            return Err("今のバージョンと同じか古い更新です".into());
        }
        if manifest.file != setup_name(&manifest.version) {
            return Err("manifest のファイル名が一致しません".into());
        }
        check_min_version(&manifest, CURRENT)?;
        let expected = expected_sum(&self.sums, &manifest.file)
            .ok_or_else(|| format!("{SUMS} に {} がありません", manifest.file))?;
        if !expected.eq_ignore_ascii_case(&manifest.sha256) {
            return Err(BAD_SUM.into());
        }
        let bytes =
            std::fs::read(&self.path).map_err(|e| format!("インストーラーを読めません: {e}"))?;
        if !sha256_hex(&bytes).eq_ignore_ascii_case(&manifest.sha256) {
            return Err(BAD_SUM.into());
        }
        Ok(())
    }
}

/// Downloads `release`'s manifest, signature, sums and installer under `root`, checks them all
/// and keeps them. `report` says how it goes (it is called from the downloading thread).
pub fn prepare(
    root: &Path,
    release: &Release,
    fetch: &Fetch,
    download: &Download,
    key_hex: &str,
    beta: bool,
    report: &mut dyn FnMut(Progress),
) -> Result<Installer, String> {
    if is_placeholder_key(key_hex) {
        return Err(NO_KEY.into());
    }
    let manifest_asset = release
        .manifest()
        .ok_or_else(|| format!("{} がありません", manifest_name(&release.version)))?;
    let signature_asset = release
        .signature()
        .ok_or_else(|| format!("{} がありません", manifest_signature_name(&release.version)))?;
    let setup_asset = release
        .setup()
        .ok_or_else(|| format!("{} がありません", setup_name(&release.version)))?;
    let sums_asset = release
        .asset(SUMS)
        .ok_or_else(|| format!("{SUMS} がありません"))?;

    let dir = release_dir(root, &release.version);
    let manifest_bytes = (fetch)(&release_url(&release.version, &manifest_asset.name))
        .map_err(|e| format!("manifest を取得できません: {e}"))?
        .into_bytes();
    if manifest_bytes.len() as u64 > SMALL_MAX {
        return Err("manifest が大きすぎます".into());
    }
    let signature = (fetch)(&release_url(&release.version, &signature_asset.name))
        .map_err(|e| format!("署名を取得できません: {e}"))?;
    let sums = (fetch)(&release_url(&release.version, &sums_asset.name))
        .map_err(|e| format!("{SUMS} を取得できません: {e}"))?;

    // The manifest is checked before the installer is taken, and before anything is written:
    // its version, channel, minimum version and hash (UI-282).
    let manifest = check_manifest(
        &manifest_bytes,
        &signature,
        &sums,
        key_hex,
        release,
        CURRENT,
        beta,
    )?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} を作れません: {e}", dir.display()))?;

    let part = part_path(&dir, &setup_asset.name);
    let final_path = dir.join(&setup_asset.name);
    let _ = std::fs::remove_file(&part);
    report(Progress {
        phase: Phase::Downloading,
        got: 0,
        total: setup_asset.size,
    });
    let result = (|| -> Result<(), String> {
        let total = setup_asset.size;
        let max = if total > 0 {
            total.saturating_add(1 << 16)
        } else {
            SETUP_MAX
        };
        (download)(
            &release_url(&release.version, &setup_asset.name),
            &part,
            max,
            &mut |got, _| {
                report(Progress {
                    phase: Phase::Downloading,
                    got,
                    total,
                });
            },
        )?;
        report(Progress {
            phase: Phase::Verifying,
            got: total,
            total,
        });
        let bytes = std::fs::read(&part).map_err(|e| format!("インストーラーを読めません: {e}"))?;
        if total > 0 && bytes.len() as u64 != total {
            return Err("インストーラーの大きさが一致しません".into());
        }
        if !sha256_hex(&bytes).eq_ignore_ascii_case(&manifest.sha256) {
            return Err(BAD_SUM.into());
        }
        // Windows does not rename over a file that is there: an older copy goes first.
        let _ = std::fs::remove_file(&final_path);
        std::fs::rename(&part, &final_path)
            .map_err(|e| format!("インストーラーを置けません: {e}"))?;
        std::fs::write(dir.join(&manifest_asset.name), &manifest_bytes)
            .map_err(|e| format!("manifest を置けません: {e}"))?;
        std::fs::write(dir.join(&signature_asset.name), &signature)
            .map_err(|e| format!("署名を置けません: {e}"))?;
        std::fs::write(dir.join(SUMS), &sums).map_err(|e| format!("{SUMS} を置けません: {e}"))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    // Only the version just taken is kept; older downloads go.
    remove_others(root, &release.version);
    report(Progress {
        phase: Phase::Ready,
        got: setup_asset.size,
        total: setup_asset.size,
    });
    Ok(Installer {
        path: final_path,
        version: release.version.clone(),
        manifest,
        manifest_bytes,
        signature,
        sums,
    })
}

/// The checked installer kept under `root` for a version newer than `current`, if any; anything
/// older or broken is removed (UI-282).
pub fn ready_installer(root: &Path, current: &str, beta: bool, key_hex: &str) -> Option<Installer> {
    if is_placeholder_key(key_hex) {
        return None;
    }
    let base = root.join(DOWNLOAD_DIR);
    let Ok(entries) = std::fs::read_dir(&base) else {
        return None;
    };
    let mut found: Vec<Installer> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        // Another window is still taking this one: leave its part and folder alone (UI-282).
        if has_live_part(&dir) {
            continue;
        }
        let version = dir.file_name().and_then(|n| n.to_str()).map(str::to_string);
        let Some(version) = version.filter(|v| is_newer(v, current)) else {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        };
        remove_parts(&dir);
        match installer_in(&dir, &version, current, beta, key_hex) {
            Some(installer) => found.push(installer),
            None => {
                // A folder that is not sound may be a window that has just started writing;
                // only one left for a day is cleaned up.
                if older_than_a_day(&dir) {
                    let _ = std::fs::remove_dir_all(&dir);
                }
            }
        }
    }
    found.sort_by(|a, b| {
        if is_newer(&a.version, &b.version) {
            std::cmp::Ordering::Greater
        } else if is_newer(&b.version, &a.version) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let best = found.pop();
    for other in found {
        let dir = other.path.parent().unwrap_or(Path::new(""));
        if !has_live_part(dir) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    best
}

/// The installer kept in `dir`, checked again; `None` when it is not sound.
fn installer_in(
    dir: &Path,
    version: &str,
    current: &str,
    beta: bool,
    key_hex: &str,
) -> Option<Installer> {
    let manifest_name = manifest_name(version);
    let manifest_bytes = std::fs::read(dir.join(&manifest_name)).ok()?;
    let signature = std::fs::read_to_string(dir.join(manifest_signature_name(version))).ok()?;
    let sums = std::fs::read_to_string(dir.join(SUMS)).ok()?;
    let channel = if parse_version(version)?.pre.is_empty() {
        STABLE
    } else {
        BETA
    };
    let release = Release {
        version: version.to_string(),
        url: release_url(version, &manifest_name),
        notes: Vec::new(),
        prerelease: channel == BETA,
        assets: Vec::new(),
    };
    let manifest = check_manifest(
        &manifest_bytes,
        &signature,
        &sums,
        key_hex,
        &release,
        current,
        beta,
    )
    .ok()?;
    let path = dir.join(&manifest.file);
    let bytes = std::fs::read(&path).ok()?;
    if !sha256_hex(&bytes).eq_ignore_ascii_case(&manifest.sha256) {
        return None;
    }
    Some(Installer {
        path,
        version: version.to_string(),
        manifest,
        manifest_bytes,
        signature,
        sums,
    })
}

/// The folder a version's installer is kept in.
fn release_dir(root: &Path, version: &str) -> PathBuf {
    root.join(DOWNLOAD_DIR).join(version)
}

/// The name of a download in progress: its own, so two Xirius windows do not take each other's
/// half-written file (UI-282).
fn part_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.part.{}", std::process::id()))
}

fn remove_parts(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(pid) = path.file_name().and_then(|n| n.to_str()).and_then(part_pid)
            && !pid_alive(pid)
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The PID in a part's name (`<name>.part.<pid>`).
fn part_pid(name: &str) -> Option<u32> {
    name.rsplit_once(".part.")
        .and_then(|(_, pid)| pid.parse().ok())
}

/// Is a window still writing a part in this folder? Its files are left alone (UI-282).
fn has_live_part(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(part_pid)
            .is_some_and(pid_alive)
    })
}

/// Has the folder not been touched for a day? A window may still be writing a new one.
fn older_than_a_day(dir: &Path) -> bool {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age >= Duration::from_secs(DAY_MS / 1000))
}

/// Removes every kept installer other than `version`.
fn remove_others(root: &Path, version: &str) {
    let Ok(entries) = std::fs::read_dir(root.join(DOWNLOAD_DIR)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path.file_name().and_then(|n| n.to_str()) != Some(version)
            && !has_live_part(&path)
        {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// The file the installer leaves when it could not put the new version in (UI-282); the next
/// start takes it away and says so.
pub fn take_failure(root: &Path) -> Option<String> {
    let mut paths = vec![root.join(FAILED)];
    // The installer writes the note where the app's folder is by default; a window that moved
    // its folder with the environment variable still hears about it (UI-282).
    let default = xirius_plugins::default_cache_path()
        .with_file_name("keep")
        .join(FAILED);
    if !paths.contains(&default) {
        paths.push(default);
    }
    for path in paths {
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
            return Some("前回の更新を入れられませんでした。Xirius のほかのプログラムが動いていないか確かめてください".into());
        }
    }
    None
}

/// One program of the system's process list.
#[cfg(windows)]
struct ProcessEntry {
    pid: u32,
    parent: u32,
    name: String,
    created: Option<u64>,
}

/// The programs running now, as the system lists them.
#[cfg(windows)]
fn process_snapshot() -> Vec<ProcessEntry> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut out = Vec::new();
    // SAFETY: the snapshot is ours, the entry is filled by the call, and both are closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let pid = entry.th32ProcessID;
                out.push(ProcessEntry {
                    pid,
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..end]),
                    created: process_created(pid),
                });
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    out
}

/// When the process was started; `None` when this program may not ask.
#[cfg(windows)]
fn process_created(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: the handle is ours and closed; the four times are filled by the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(process);
        (ok != 0).then_some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

/// Is `pid` this program or one of its descendants? The programs this window started (the
/// engine, the plugin hosts, the scanner) go when it goes, so they do not block an update
/// (UI-282). A process started before this one is not its child, even when a PID was reused.
#[cfg(windows)]
fn is_descendant(pid: u32, own: u32, entries: &[ProcessEntry]) -> bool {
    if pid == own {
        return true;
    }
    let created = entries
        .iter()
        .find(|e| e.pid == pid)
        .and_then(|e| e.created);
    let own_created = entries
        .iter()
        .find(|e| e.pid == own)
        .and_then(|e| e.created);
    let mut current = pid;
    for _ in 0..64 {
        let Some(entry) = entries.iter().find(|e| e.pid == current) else {
            return false;
        };
        if entry.parent == own {
            return match (created, own_created) {
                (Some(created), Some(own)) => created >= own,
                _ => true,
            };
        }
        if entry.parent == 0 || entry.parent == current {
            return false;
        }
        current = entry.parent;
    }
    false
}

/// The Xirius programs of `dir` that the user runs separately, except this window and the
/// programs it started (UI-282).
#[cfg(windows)]
pub fn running_programs(dir: &Path, own_pid: u32) -> Vec<String> {
    let entries = process_snapshot();
    let mut found = Vec::new();
    for entry in &entries {
        if entry.pid == own_pid
            || is_descendant(entry.pid, own_pid, &entries)
            || !PROGRAMS.iter().any(|p| p.eq_ignore_ascii_case(&entry.name))
        {
            continue;
        }
        if let Some(path) = image_path(entry.pid)
            && path.parent().is_some_and(|parent| same_dir(parent, dir))
        {
            found.push(entry.name.clone());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// A process's program file, when this program may ask.
#[cfg(windows)]
fn image_path(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    // SAFETY: the handle is ours and closed; the buffer is ours and its length is given.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize])))
    }
}

#[cfg(not(windows))]
pub fn running_programs(_: &Path, _: u32) -> Vec<String> {
    Vec::new()
}

/// Is a process with this PID running? A `.part` of a live window is not cleaned up (UI-282).
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;

    if pid == 0 {
        return false;
    }
    // SAFETY: the handle is ours and closed; the wait only asks whether it has ended.
    unsafe {
        let process = OpenProcess(SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        let state = WaitForSingleObject(process, 0);
        CloseHandle(process);
        if state != WAIT_TIMEOUT {
            return false;
        }
    }
    // A PID is reused, and Windows ignores its low two bits: only a running Xirius program
    // counts as the window that is writing the part.
    image_path(pid)
        .and_then(|path| {
            path.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        })
        .is_some_and(|name| name.starts_with("xirius"))
}

#[cfg(not(windows))]
fn pid_alive(_: u32) -> bool {
    false
}

/// Checks a manifest against the release it came with, this program and the channel (UI-282).
pub fn check_manifest(
    manifest_bytes: &[u8],
    sig_hex: &str,
    sums: &str,
    key_hex: &str,
    release: &Release,
    current: &str,
    beta: bool,
) -> Result<Manifest, String> {
    verify_signature(manifest_bytes, sig_hex, key_hex)?;
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|_| "manifest を読み取れません".to_string())?;
    if parse_version(&manifest.version).is_none() {
        return Err("manifest のバージョンを読み取れません".into());
    }
    if manifest.version != release.version {
        return Err("manifest のバージョンが Release と一致しません".into());
    }
    if !is_newer(&manifest.version, current) {
        return Err("今のバージョンと同じか古い更新です".into());
    }
    if manifest.file != setup_name(&manifest.version) {
        return Err("manifest のファイル名が一致しません".into());
    }
    check_min_version(&manifest, current)?;
    let tag_channel = if release.prerelease { BETA } else { STABLE };
    if manifest.channel != tag_channel {
        return Err("manifest のチャンネルが Release と一致しません".into());
    }
    if manifest.channel == BETA && !beta {
        return Err("ベータ版は受け取らない設定です".into());
    }
    let expected = expected_sum(sums, &manifest.file)
        .ok_or_else(|| format!("{SUMS} に {} がありません", manifest.file))?;
    if !expected.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(BAD_SUM.into());
    }
    Ok(manifest)
}

/// The lowest version a manifest allows to install it (UI-282).
fn check_min_version(manifest: &Manifest, current: &str) -> Result<(), String> {
    if parse_version(&manifest.min_version).is_none() {
        return Err("manifest の min_version を読み取れません".into());
    }
    if is_newer(&manifest.min_version, current) {
        return Err("この版には更新できません（今のバージョンが古すぎます）".into());
    }
    Ok(())
}

/// Checks the signature of `bytes` (the manifest) with `key_hex`.
pub fn verify_signature(bytes: &[u8], sig_hex: &str, key_hex: &str) -> Result<(), String> {
    if is_placeholder_key(key_hex) {
        return Err(NO_KEY.into());
    }
    let key = hex_array::<32>(key_hex).ok_or(NO_KEY)?;
    let key = VerifyingKey::from_bytes(&key).map_err(|_| NO_KEY.to_string())?;
    let sig = hex_array::<64>(sig_hex).ok_or("署名のファイルを読み取れません")?;
    let sig = Signature::from_bytes(&sig);
    key.verify_strict(bytes, &sig)
        .map_err(|_| BAD_SIGNATURE.to_string())
}

/// The hash `sums` gives `name` (`<hex>  <name>`, as `sha256sum` writes it).
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut field = line.split_whitespace();
        let hash = field.next()?;
        let file = field.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_string())
    })
}

/// The SHA-256 of `bytes`, as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hex_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    // A byte position is a character position only for ASCII; a `.sig` with other characters
    // is not one (UI-282).
    if !text.is_ascii() || text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Where Xirius runs from and where the installer puts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub exe_dir: PathBuf,
    pub install_dir: PathBuf,
}

impl Location {
    /// This program's folder and the folder the installer used (HKCU's `InstallLocation`, or
    /// `%LOCALAPPDATA%\Programs\Xirius` when there is none).
    pub fn per_user() -> Option<Location> {
        let exe = std::env::current_exe().ok()?;
        let exe_dir = exe.parent()?.to_path_buf();
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let install_dir = installed_dir().unwrap_or_else(|| per_user_dir(&local));
        Some(Location {
            exe_dir,
            install_dir,
        })
    }

    /// Is this program where the installer put it? A folder the zip was unpacked to is not, and
    /// an update there would leave two copies (UI-282).
    pub fn installable(&self) -> bool {
        same_dir(&self.exe_dir, &self.install_dir)
    }

    /// The installer's command for this program, or why there is none.
    pub fn install_command(&self, setup: &Path) -> Result<Install, String> {
        if !self.installable() {
            return Err("この場所からは更新を入れられません".into());
        }
        if !setup.is_file() {
            return Err("インストーラーがありません".into());
        }
        Ok(Install {
            program: setup.to_path_buf(),
            // Silent, and Xirius comes back after the install (UI-282).
            args: vec!["/S".to_string(), "/XRUN".to_string()],
        })
    }
}

/// The folder the per-user installer uses under `%LOCALAPPDATA%` (`packaging/windows/xirius.nsi`).
pub fn per_user_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("Xirius")
}

/// The folder recorded when Xirius was installed (the user may have chosen another one).
#[cfg(windows)]
fn installed_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
    };

    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    };
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Xirius");
    let value = wide("InstallLocation");
    let mut buffer = vec![0u16; 2048];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: the buffers are ours, their sizes are given, and the call only reads them.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = (bytes as usize / 2).saturating_sub(1);
    let text = String::from_utf16_lossy(&buffer[..len]);
    (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()))
}

#[cfg(not(windows))]
fn installed_dir() -> Option<PathBuf> {
    None
}

/// What the window starts once it has closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Starts the installer without waiting for it; Xirius leaves while the installer waits for it.
pub fn spawn_detached(install: &Install) -> Result<(), String> {
    let mut c = std::process::Command::new(&install.program);
    c.args(&install.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hide_window(&mut c);
    c.spawn()
        .map(|_| ())
        .map_err(|e| format!("インストーラーを起動できません: {e}"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
            .collect()
    };
    parts(a) == parts(b)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const ANSWER: &str = r#"{
        "tag_name": "v1.0.1",
        "html_url": "https://github.com/tekalu1/xirius/releases/tag/v1.0.1",
        "body": "変更点\r\n- 録音の開始が速くなりました\r\n* MIDI の入力の不具合を修正\r\n\r\nそのほか",
        "assets": [
            {"name": "Xirius-1.0.1-win-x64-setup.exe", "browser_download_url": "https://example.test/setup.exe", "size": 19},
            {"name": "Xirius-1.0.1-win-x64-update.json", "size": 130},
            {"name": "Xirius-1.0.1-win-x64-update.json.sig", "size": 129},
            {"name": "SHA256SUMS.txt", "size": 300}
        ]
    }"#;

    const LIST: &str = r#"[
        {"tag_name": "v1.1.0", "html_url": "https://example.test/stable", "body": "- 安定版", "prerelease": false},
        {"tag_name": "v1.2.0-beta.2", "html_url": "https://example.test/beta2", "body": "- ベータ", "prerelease": true},
        {"tag_name": "v1.0.0", "html_url": "https://example.test/old", "body": "", "prerelease": false},
        {"tag_name": "v9.9.9-draft", "html_url": "https://example.test/draft", "body": "", "prerelease": true, "draft": true}
    ]"#;

    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-update-{tag}-"))
            .tempdir()
            .unwrap()
    }

    /// A fetch that answers `body` and counts the asks, never reaching the network.
    fn stub(
        body: Result<&'static str, &'static str>,
    ) -> (Fetch, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let asked = Arc::new(AtomicUsize::new(0));
        let urls = Arc::new(Mutex::new(Vec::new()));
        let (a, u) = (asked.clone(), urls.clone());
        let fetch: Fetch = Arc::new(move |url: &str| {
            a.fetch_add(1, Ordering::SeqCst);
            u.lock().unwrap().push(url.to_string());
            body.map(str::to_string).map_err(str::to_string)
        });
        (fetch, asked, urls)
    }

    /// A key and its public half, made in the test.
    fn keypair() -> (ed25519_dalek::SigningKey, String) {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let public = key.verifying_key().to_bytes();
        let hex = public.iter().map(|b| format!("{b:02x}")).collect();
        (key, hex)
    }

    fn signed(key: &ed25519_dalek::SigningKey, bytes: &[u8]) -> String {
        use ed25519_dalek::Signer;
        key.sign(bytes)
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn manifest(version: &str, bytes: &[u8], channel: &str) -> Vec<u8> {
        format!(
            "{{\"version\":\"{version}\",\"file\":\"{}\",\"sha256\":\"{}\",\"channel\":\"{channel}\",\"min_version\":\"1.0.0\"}}\n",
            setup_name(version),
            sha256_hex(bytes)
        )
        .into_bytes()
    }

    fn sums_for(name: &str, bytes: &[u8]) -> String {
        format!("{}  {name}\n", sha256_hex(bytes))
    }

    fn release(version: &str, prerelease: bool) -> Release {
        Release {
            version: version.into(),
            url: format!("{REPOSITORY}/releases/tag/v{version}"),
            notes: vec!["変わったこと".into()],
            prerelease,
            assets: Vec::new(),
        }
    }

    #[test]
    fn the_answer_gives_the_version_the_page_the_bullets_and_the_files() {
        let r = parse(ANSWER).unwrap();
        assert_eq!(r.version, "1.0.1");
        assert_eq!(
            r.url,
            "https://github.com/tekalu1/xirius/releases/tag/v1.0.1"
        );
        assert_eq!(
            r.notes,
            ["録音の開始が速くなりました", "MIDI の入力の不具合を修正"]
        );
        assert_eq!(r.setup().map(|a| a.size), Some(19));
        assert_eq!(r.manifest().unwrap().size, 130);
        assert_eq!(r.signature().unwrap().size, 129);
        assert!(!r.prerelease);
        // The names a release must use (UI-282).
        assert_eq!(setup_name("1.0.1"), "Xirius-1.0.1-win-x64-setup.exe");
        assert_eq!(manifest_name("1.0.1"), "Xirius-1.0.1-win-x64-update.json");
        assert_eq!(
            manifest_signature_name("1.0.1"),
            "Xirius-1.0.1-win-x64-update.json.sig"
        );
        assert_eq!(
            release_url("1.0.1", "SHA256SUMS.txt"),
            "https://github.com/tekalu1/xirius/releases/download/v1.0.1/SHA256SUMS.txt"
        );
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"tag_name": "nightly"}"#).is_none());
    }

    #[test]
    fn versions_compare_by_their_numbers_and_then_their_pre_release() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("1.1", "1.0.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(!is_newer("x", "1.0.0"));
        // A release is later than its pre-releases, and the parts after the dash count.
        assert!(is_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
        assert!(is_newer("1.0.0-beta.3", "1.0.0-beta.2"));
        assert!(is_newer("1.0.0-beta.10", "1.0.0-beta.2"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0-beta.9"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
        // A build's `+…` does not count.
        assert!(!is_newer("1.0.0+build.2", "1.0.0"));
    }

    #[test]
    fn the_beta_channel_reads_the_list_and_takes_the_highest_pre_release() {
        let list = parse_releases(LIST).unwrap();
        assert_eq!(list.len(), 3, "the draft is not a release");
        assert_eq!(pick(&list, true).unwrap().version, "1.2.0-beta.2");
        assert_eq!(pick(&list, false).unwrap().version, "1.1.0");
        assert_eq!(pick(&[], true), None);
    }

    #[test]
    fn a_launch_asks_once_a_day_and_keeps_the_newer_release_meanwhile() {
        let tmp = scratch("daily");
        let (fetch, asked, urls) = stub(Ok(ANSWER));
        let check = UpdateCheck::in_dir(tmp.path(), fetch);
        let t0 = 1_000_000_000_000;
        let r = check.at_launch(true, false, t0, "1.0.0").unwrap();
        assert_eq!(r.version, "1.0.1");
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        assert_eq!(urls.lock().unwrap()[0], LATEST);
        // Within the day: not asked again, the release still known.
        let again = check.at_launch(true, false, t0 + DAY_MS - 1, "1.0.0");
        assert_eq!(again.as_ref().map(|r| r.version.as_str()), Some("1.0.1"));
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        // A day on: asked again.
        check.at_launch(true, false, t0 + DAY_MS, "1.0.0");
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // The other channel asks at once.
        check.at_launch(true, true, t0 + DAY_MS + 1, "1.0.0");
        assert_eq!(asked.load(Ordering::SeqCst), 3);
        assert_eq!(urls.lock().unwrap()[2], RELEASES);
        // The same version as this one: nothing to show.
        assert!(check.at_launch(true, false, t0 + DAY_MS, "1.0.1").is_none());
    }

    #[test]
    fn turned_off_it_asks_nothing() {
        let tmp = scratch("off");
        let (fetch, asked, _) = stub(Ok(ANSWER));
        let check = UpdateCheck::in_dir(tmp.path(), fetch);
        assert!(check.at_launch(false, false, 1_000, "1.0.0").is_none());
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        assert!(check.stored().is_none());
    }

    /// An `update.json` of UI-262 (no beta flag, no files) still reads (UI-282).
    #[test]
    fn an_answer_kept_before_the_beta_channel_still_reads() {
        let old = r#"{"checked_ms":5,"latest":{"version":"1.0.1","url":"u","notes":["a"]}}"#;
        let checked: Checked = serde_json::from_str(old).unwrap();
        assert!(!checked.beta);
        let release = checked.latest.unwrap();
        assert!(!release.prerelease);
        assert!(release.assets.is_empty());
        assert_eq!(release.version, "1.0.1");
    }

    #[test]
    fn a_failure_says_nothing_and_the_next_launch_asks_again() {
        let tmp = scratch("fail");
        let (fetch, asked, _) = stub(Err("could not resolve host"));
        let check = UpdateCheck::in_dir(tmp.path(), fetch);
        assert!(check.at_launch(true, false, 1_000, "1.0.0").is_none());
        assert!(check.stored().is_none(), "nothing kept");
        assert!(check.at_launch(true, false, 2_000, "1.0.0").is_none());
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // An answer that is not a release is a failure too.
        let (fetch, _, _) = stub(Ok("<html>rate limited</html>"));
        let check = UpdateCheck::in_dir(tmp.path(), fetch);
        assert!(check.ask(3_000, false).is_err());
    }

    #[test]
    fn the_signature_and_the_hash_must_both_match() {
        let (key, public) = keypair();
        let bytes = b"the installer";
        let name = setup_name("1.0.1");
        let sums = sums_for(&name, bytes);
        let manifest = manifest("1.0.1", bytes, STABLE);
        let sig = signed(&key, &manifest);
        let release = release("1.0.1", false);
        assert_eq!(
            check_manifest(&manifest, &sig, &sums, &public, &release, "1.0.0", false)
                .map(|m| m.version),
            Ok("1.0.1".into())
        );

        // Another manifest byte: the signature fails.
        let mut other = manifest.clone();
        other[5] ^= 1;
        assert_eq!(
            check_manifest(&other, &sig, &sums, &public, &release, "1.0.0", false),
            Err(BAD_SIGNATURE.into())
        );
        // A `.sig` with a non-ASCII character is read as no signature, not a panic (UI-282).
        let mut bad = sig.clone();
        bad.replace_range(0..1, "あ");
        assert_eq!(
            check_manifest(&manifest, &bad, &sums, &public, &release, "1.0.0", false),
            Err("署名のファイルを読み取れません".into())
        );
        // The signature is right, the hash list is not.
        let other = sums_for(&name, b"something else");
        assert_eq!(
            check_manifest(&manifest, &sig, &other, &public, &release, "1.0.0", false),
            Err(BAD_SUM.into())
        );
    }

    /// UI-282: a correctly signed old release cannot be passed off as a new one.
    #[test]
    fn a_manifest_cannot_lower_the_version() {
        let (key, public) = keypair();
        let bytes = b"the installer";
        let old = manifest("1.0.0", bytes, STABLE);
        let sig = signed(&key, &old);
        let sums = sums_for(&setup_name("1.0.0"), bytes);
        // The tag says 9.9.9: the manifest's version is not the tag's.
        assert_eq!(
            check_manifest(
                &old,
                &sig,
                &sums,
                &public,
                &release("9.9.9", false),
                "1.0.0",
                false
            ),
            Err("manifest のバージョンが Release と一致しません".into())
        );
        // The tag and the manifest agree, but it is this program's version.
        assert_eq!(
            check_manifest(
                &old,
                &sig,
                &sums,
                &public,
                &release("1.0.0", false),
                "1.0.0",
                false
            ),
            Err("今のバージョンと同じか古い更新です".into())
        );
        // The manifest names another installer.
        let mut wrong = manifest("1.0.1", bytes, STABLE);
        wrong = String::from_utf8(wrong)
            .unwrap()
            .replace("Xirius-1.0.1-win-x64-setup.exe", "Xirius-1.0.1-win-x64.zip")
            .into_bytes();
        assert_eq!(
            check_manifest(
                &wrong,
                &signed(&key, &wrong),
                &sums,
                &public,
                &release("1.0.1", false),
                "1.0.0",
                false
            ),
            Err("manifest のファイル名が一致しません".into())
        );
    }

    #[test]
    fn the_channel_must_fit_the_release_and_the_setting() {
        let (key, public) = keypair();
        let bytes = b"the installer";
        let beta = manifest("1.1.0-beta.1", bytes, BETA);
        let beta_sig = signed(&key, &beta);
        let sums = sums_for(&setup_name("1.1.0-beta.1"), bytes);
        // A beta manifest on a stable release.
        assert_eq!(
            check_manifest(
                &beta,
                &beta_sig,
                &sums,
                &public,
                &release("1.1.0-beta.1", false),
                "1.0.0",
                true
            ),
            Err("manifest のチャンネルが Release と一致しません".into())
        );
        // A beta release with the setting off.
        assert_eq!(
            check_manifest(
                &beta,
                &beta_sig,
                &sums,
                &public,
                &release("1.1.0-beta.1", true),
                "1.0.0",
                false
            ),
            Err("ベータ版は受け取らない設定です".into())
        );
        // With the setting on, it is taken.
        assert!(
            check_manifest(
                &beta,
                &beta_sig,
                &sums,
                &public,
                &release("1.1.0-beta.1", true),
                "1.0.0",
                true
            )
            .is_ok()
        );
        // A stable manifest on a pre-release.
        let stable = manifest("1.1.0", bytes, STABLE);
        assert_eq!(
            check_manifest(
                &stable,
                &signed(&key, &stable),
                &sums_for(&setup_name("1.1.0"), bytes),
                &public,
                &release("1.1.0", true),
                "1.0.0",
                true
            ),
            Err("manifest のチャンネルが Release と一致しません".into())
        );
    }

    #[test]
    fn the_placeholder_key_installs_nothing() {
        let bytes = b"the installer";
        let manifest = manifest("1.0.1", bytes, STABLE);
        assert!(is_placeholder_key(PLACEHOLDER_KEY));
        assert!(!key_is_placeholder(), "this build checks with the real key");
        assert_eq!(
            check_manifest(
                &manifest,
                "00",
                &sums_for(&setup_name("1.0.1"), bytes),
                PLACEHOLDER_KEY,
                &release("1.0.1", false),
                "1.0.0",
                false
            ),
            Err(NO_KEY.into())
        );
    }

    /// A test's download: `body` is what it writes, and it may stop halfway.
    fn download_writing(body: &'static [u8], stop: bool) -> (Download, Arc<Mutex<Vec<String>>>) {
        let urls = Arc::new(Mutex::new(Vec::new()));
        let u = urls.clone();
        let download: Download = Arc::new(move |url, path, max, progress| {
            u.lock().unwrap().push(url.to_string());
            assert!(body.len() as u64 <= max, "the size cap is too small");
            let half = body.len() / 2;
            let first = &body[..if stop { half } else { body.len() }];
            std::fs::write(path, first).map_err(|e| e.to_string())?;
            progress(first.len() as u64, 0);
            if stop {
                return Err("connection closed".into());
            }
            Ok(())
        });
        (download, urls)
    }

    fn release_with_assets() -> Release {
        parse(ANSWER).unwrap()
    }

    /// A fetch for the three small files of [`release_with_assets`].
    fn small_files(manifest: Vec<u8>, signature: String, sums: String) -> Fetch {
        Arc::new(move |url: &str| {
            Ok(match url {
                "https://github.com/tekalu1/xirius/releases/download/v1.0.1/Xirius-1.0.1-win-x64-update.json" => {
                    String::from_utf8(manifest.clone()).unwrap()
                }
                "https://github.com/tekalu1/xirius/releases/download/v1.0.1/Xirius-1.0.1-win-x64-update.json.sig" => {
                    signature.clone()
                }
                "https://github.com/tekalu1/xirius/releases/download/v1.0.1/SHA256SUMS.txt" => {
                    sums.clone()
                }
                other => return Err(format!("no {other}")),
            })
        })
    }

    #[test]
    fn a_checked_download_is_kept_under_the_apps_folder() {
        let (key, public) = keypair();
        let body = b"the installer bytes";
        let manifest = manifest("1.0.1", body, STABLE);
        let signature = signed(&key, &manifest);
        let sums = sums_for(&setup_name("1.0.1"), body);
        let text_fetch = small_files(manifest.clone(), signature.clone(), sums);
        let (download, urls) = download_writing(body, false);
        let tmp = scratch("prepare");
        let mut seen = Vec::new();
        let installer = prepare(
            tmp.path(),
            &release_with_assets(),
            &text_fetch,
            &download,
            &public,
            false,
            &mut |p| seen.push(p),
        )
        .unwrap();
        let dir = tmp.path().join(DOWNLOAD_DIR).join("1.0.1");
        assert_eq!(installer.version, "1.0.1");
        assert_eq!(installer.path, dir.join(setup_name("1.0.1")));
        assert_eq!(std::fs::read(&installer.path).unwrap(), body);
        assert_eq!(
            std::fs::read(dir.join(manifest_name("1.0.1"))).unwrap(),
            manifest
        );
        assert!(dir.join(manifest_signature_name("1.0.1")).is_file());
        assert!(dir.join(SUMS).is_file());
        assert_eq!(
            urls.lock().unwrap()[0],
            "https://github.com/tekalu1/xirius/releases/download/v1.0.1/Xirius-1.0.1-win-x64-setup.exe"
        );
        assert!(
            !dir.join(format!(
                "{}.part.{}",
                setup_name("1.0.1"),
                std::process::id()
            ))
            .exists()
        );
        assert!(seen.iter().any(|p| p.phase == Phase::Downloading));
        assert!(seen.iter().any(|p| p.phase == Phase::Verifying));
        assert_eq!(seen.last().unwrap().phase, Phase::Ready);
        // The same version again (the app was left open): the kept one is replaced.
        let again = prepare(
            tmp.path(),
            &release_with_assets(),
            &text_fetch,
            &download,
            &public,
            false,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(again.path).unwrap(), body);
        // And it is found at the next start, and verified again.
        let found = ready_installer(tmp.path(), CURRENT, false, &public).unwrap();
        assert_eq!(found.version, "1.0.1");
        assert_eq!(found.verify_with(&public), Ok(()));
        // UI-282: a file replaced after it was checked is not run.
        std::fs::write(&found.path, b"tampered").unwrap();
        assert_eq!(found.verify_with(&public), Err(BAD_SUM.into()));
    }

    #[test]
    fn a_download_that_stops_halfway_leaves_nothing() {
        let (key, public) = keypair();
        let body = b"the installer bytes";
        let manifest = manifest("1.0.1", body, STABLE);
        let signature = signed(&key, &manifest);
        let sums = sums_for(&setup_name("1.0.1"), body);
        let text_fetch = small_files(manifest, signature, sums);
        let (download, _) = download_writing(body, true);
        let tmp = scratch("cut");
        let err = prepare(
            tmp.path(),
            &release_with_assets(),
            &text_fetch,
            &download,
            &public,
            false,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("connection closed"), "{err}");
        let dir = tmp.path().join(DOWNLOAD_DIR).join("1.0.1");
        assert!(!dir.join(setup_name("1.0.1")).exists());
        assert!(
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().contains(".part.")),
            "the part is thrown away"
        );
    }

    #[test]
    fn a_download_with_the_wrong_signature_is_thrown_away() {
        let (key, public) = keypair();
        let body = b"the installer bytes";
        let manifest = manifest("1.0.1", body, STABLE);
        let signature = signed(&key, b"another manifest");
        let sums = sums_for(&setup_name("1.0.1"), body);
        let text_fetch = small_files(manifest, signature, sums);
        let (download, _) = download_writing(body, false);
        let tmp = scratch("badsig");
        assert_eq!(
            prepare(
                tmp.path(),
                &release_with_assets(),
                &text_fetch,
                &download,
                &public,
                false,
                &mut |_| {},
            ),
            Err(BAD_SIGNATURE.into())
        );
        let dir = tmp.path().join(DOWNLOAD_DIR).join("1.0.1");
        assert!(!dir.join(setup_name("1.0.1")).exists());
    }

    #[test]
    fn two_windows_do_not_share_a_part_file() {
        let dir = Path::new("C:/xirius/update");
        let one = part_path(dir, "setup.exe");
        assert!(
            one.to_string_lossy()
                .contains(&format!(".part.{}", std::process::id()))
        );
        // The name holds the process, so another Xirius writes its own (UI-282).
        assert_ne!(
            part_path(dir, "setup.exe").to_string_lossy(),
            "setup.exe.part".to_string()
        );
    }

    #[test]
    fn an_old_download_is_cleaned_up_and_the_newest_kept() {
        let (key, public) = keypair();
        let tmp = scratch("clean");
        let base = tmp.path().join(DOWNLOAD_DIR);
        let body = b"new installer";
        for version in ["0.9.0", "1.0.0", "1.0.1", "1.0.2"] {
            let dir = base.join(version);
            std::fs::create_dir_all(&dir).unwrap();
            let m = manifest(version, body, STABLE);
            std::fs::write(dir.join(manifest_name(version)), &m).unwrap();
            std::fs::write(dir.join(manifest_signature_name(version)), signed(&key, &m)).unwrap();
            std::fs::write(dir.join(SUMS), sums_for(&setup_name(version), body)).unwrap();
            std::fs::write(dir.join(setup_name(version)), body).unwrap();
            // A part left by a window that was killed.
            std::fs::write(dir.join(format!("{}.part.9", setup_name(version))), b"half").unwrap();
        }
        for version in ["0.9.0", "1.0.0"] {
            assert!(!is_newer(version, CURRENT), "{version}");
        }
        let found = ready_installer(tmp.path(), CURRENT, false, &public).unwrap();
        // Both newer versions are sound; the newest is offered and the other is removed.
        assert_eq!(found.version, "1.0.2");
        assert!(base.join("1.0.2").is_dir());
        for gone in ["0.9.0", "1.0.0", "1.0.1"] {
            assert!(!base.join(gone).exists(), "{gone} is cleaned");
        }
        assert!(
            std::fs::read_dir(base.join("1.0.2"))
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().contains(".part.")),
            "the leftover part is gone"
        );
    }

    /// UI-282: a download of another window (its `.part` belongs to a live process) is left
    /// alone; one left by a dead process is cleaned up.
    #[test]
    fn a_live_process_that_is_not_xirius_does_not_hold_a_part() {
        // PID 4 is Windows' System process, and 9 is read as 8 (the low bits are ignored):
        // neither is a window writing a download.
        assert!(!pid_alive(4));
        assert!(!pid_alive(9));
        assert!(
            pid_alive(std::process::id()),
            "this test's own program is a Xirius one"
        );
    }

    #[test]
    fn a_part_of_a_live_window_is_not_cleaned_up() {
        let (_, public) = keypair();
        let tmp = scratch("livepart");
        let dir = tmp.path().join(DOWNLOAD_DIR).join("9.9.9");
        std::fs::create_dir_all(&dir).unwrap();
        let live = part_path(&dir, "setup.exe");
        std::fs::write(&live, b"half").unwrap();
        let dead = dir.join(format!("other.exe.part.{}", 9_999_999));
        std::fs::write(&dead, b"half").unwrap();

        // The folder is not touched while a part belongs to a live process.
        assert!(has_live_part(&dir));
        assert!(ready_installer(tmp.path(), CURRENT, false, &public).is_none());
        assert!(dir.is_dir());

        remove_parts(&dir);
        assert!(live.is_file(), "the live part stays");
        assert!(!dead.is_file(), "the dead part goes");
    }

    #[test]
    fn the_installer_runs_silently_and_asks_for_xirius() {
        let tmp = scratch("install");
        let setup = tmp.path().join(setup_name("1.0.1"));
        std::fs::write(&setup, b"x").unwrap();
        let location = Location {
            exe_dir: per_user_dir(tmp.path()),
            install_dir: per_user_dir(tmp.path()),
        };
        assert!(location.installable());
        let install = location.install_command(&setup).unwrap();
        assert_eq!(install.program, setup);
        assert_eq!(install.args, ["/S", "/XRUN"]);
        assert_eq!(
            per_user_dir(Path::new(r"X:\Local")),
            Path::new(r"X:\Local\Programs\Xirius")
        );
    }

    #[test]
    fn an_unpacked_folder_installs_nothing() {
        let tmp = scratch("zip");
        let setup = tmp.path().join(setup_name("1.0.1"));
        std::fs::write(&setup, b"x").unwrap();
        let location = Location {
            exe_dir: tmp.path().join("Downloads").join("Xirius-1.0.0-win-x64"),
            install_dir: tmp.path().join("local").join("Programs").join("Xirius"),
        };
        assert!(!location.installable());
        assert_eq!(
            location.install_command(&setup),
            Err("この場所からは更新を入れられません".into())
        );
    }

    /// UI-282: another Xirius program in the folder would make the silent installer wait and
    /// give up; the app must see it first. This window and the programs it started (the engine,
    /// the plugin hosts) are not others. The test runs the same shapes as an installed Xirius
    /// (`xirius-ui.exe` as this program, `xirius-plugin-host.exe` as its child) while they run.
    #[cfg(windows)]
    #[test]
    fn another_program_is_seen_but_this_window_and_its_children_are_not() {
        use std::os::windows::process::CommandExt;

        fn hidden(path: &Path) -> std::process::Child {
            let mut c = std::process::Command::new(path);
            c.args(["/c", "ping -n 20 127.0.0.1 >nul"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x0800_0000);
            c.spawn().unwrap()
        }
        fn wait_for(dir: &Path, pid: u32, name: &str) -> Vec<String> {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut names = Vec::new();
            while std::time::Instant::now() < deadline {
                names = running_programs(dir, pid);
                if names.iter().any(|n| n == name) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            names
        }

        let tmp = scratch("running");
        let dir = tmp.path();
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        let cmd = PathBuf::from(system).join("System32").join("cmd.exe");
        std::fs::copy(&cmd, dir.join("xirius-ui.exe")).unwrap();

        // This window itself: a copy of cmd running as xirius-ui.exe.
        let mut window = hidden(&dir.join("xirius-ui.exe"));
        let no_parent = u32::MAX;
        let names = wait_for(dir, no_parent, "xirius-ui.exe");
        assert!(names.iter().any(|n| n == "xirius-ui.exe"), "{names:?}");
        // Its own PID is not another program.
        assert!(
            running_programs(dir, window.id()).is_empty(),
            "this window is not another"
        );

        // A program this window started (the plugin host) is not another either.
        std::fs::copy(&cmd, dir.join("xirius-plugin-host.exe")).unwrap();
        let mut child = hidden(&dir.join("xirius-plugin-host.exe"));
        let names = wait_for(dir, no_parent, "xirius-plugin-host.exe");
        assert!(
            names.iter().any(|n| n == "xirius-plugin-host.exe"),
            "{names:?}"
        );
        assert!(
            running_programs(dir, std::process::id()).is_empty(),
            "a program this window started is not another"
        );

        let _ = window.kill();
        let _ = window.wait();
        let _ = child.kill();
        let _ = child.wait();
    }

    /// UI-282: the version in a tag is semver's characters only, so it is safe in a folder name
    /// and a URL.
    #[test]
    fn a_version_has_only_semver_characters() {
        assert!(parse_version("1.0.1").is_some());
        assert!(parse_version("v1.0.1-beta.2+build.5").is_some());
        for bad in [
            "1.0.01",
            "1.0.0-",
            "1.0.0-a/b",
            "1.0.0-a#b",
            "1.0.0-beta..1",
            "1.0.0+",
            "1.0.0+a_b",
            "1.0.0-01",
            "1.0.0-β",
        ] {
            assert!(parse_version(bad).is_none(), "{bad}");
        }
    }

    /// UI-282: a manifest signed by `scripts/update_sign.py` (Python's standard library) is
    /// accepted by the app's check. The same seed and bytes are in `scripts/test_update_sign.py`.
    #[test]
    fn a_python_signed_manifest_is_accepted() {
        let public = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
        let manifest = b"{\"channel\":\"stable\",\"file\":\"Xirius-1.0.1-win-x64-setup.exe\",\"min_version\":\"1.0.0\",\"sha256\":\"36b88446586b04e3c86320db363516229ae74a07ec30928b800aee7d751374da\",\"version\":\"1.0.1\"}\n";
        let signature = "36ce109fb240336ce5a5898b3b1dc9a23a5a2e94a69da9fecd7539a479003968ab163877c160e7c8981b18bba823d64b0c5d3ee96cd9bf43c21f1d6e253c500e";
        let setup = b"the installer";
        let sums = sums_for(&setup_name("1.0.1"), setup);
        assert!(
            check_manifest(
                manifest,
                signature,
                &sums,
                public,
                &release("1.0.1", false),
                "1.0.0",
                false
            )
            .is_ok()
        );
        // The public key of the test's seed is the one the Python signer makes.
        assert_eq!(keypair().1, public);
    }

    /// UI-282: a manifest that requires a newer Xirius is refused, so a signed interim version
    /// cannot be handed to a program older than it.
    #[test]
    fn a_manifest_needs_a_new_enough_program() {
        let (key, public) = keypair();
        let bytes = b"the installer";
        let manifest = String::from_utf8(manifest("1.0.2", bytes, STABLE))
            .unwrap()
            .replace("\"min_version\":\"1.0.0\"", "\"min_version\":\"1.0.1\"")
            .into_bytes();
        let sig = signed(&key, &manifest);
        let sums = sums_for(&setup_name("1.0.2"), bytes);
        assert_eq!(
            check_manifest(
                &manifest,
                &sig,
                &sums,
                &public,
                &release("1.0.2", false),
                "1.0.0",
                false
            ),
            Err("この版には更新できません（今のバージョンが古すぎます）".into())
        );
        assert!(
            check_manifest(
                &manifest,
                &sig,
                &sums,
                &public,
                &release("1.0.2", false),
                "1.0.1",
                false
            )
            .is_ok()
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_folder_recorded_at_install_is_the_one_compared() {
        // The registry may have an Xirius of this PC; only the shape is checked here.
        if let Some(location) = Location::per_user() {
            assert_eq!(
                location.installable(),
                same_dir(&location.exe_dir, &location.install_dir)
            );
        }
    }
}
