//! Finding the bundles and opening the new or changed ones, each in a process of its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::cache::{BundleRecord, CacheFile, fingerprint};
use crate::child::{Answer, CHILD_ARG};
use crate::spawn::{self, Desktop, Exit};
use crate::{Catalog, Failure, FailureKind, default_cache_path, standard_dirs};

/// The program that opens one bundle: `program [args] --bundle <path> --out <file>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChildProgram {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// From this many bundles opened in one scan, a scan in which over half failed is taken to be
/// broken itself and does not replace the list.
const BROKEN_MIN: usize = 8;

static OWN: OnceLock<ChildProgram> = OnceLock::new();

impl ChildProgram {
    /// This program as the scan process (it answers [`CHILD_ARG`] itself, see [`child_main`]).
    /// The `xirius` command and the screen call it at start, so a scan never depends on where
    /// the files were put.
    ///
    /// [`child_main`]: crate::child_main
    pub fn this_program() -> Option<ChildProgram> {
        std::env::current_exe().ok().map(|program| ChildProgram {
            program,
            args: vec![CHILD_ARG.into()],
        })
    }

    /// Makes [`locate`](Self::locate) answer with this program.
    pub fn use_this_program() {
        if let Some(p) = Self::this_program() {
            let _ = OWN.set(p);
        }
    }

    /// `XIRIUS_PLUGIN_SCAN`, otherwise the program that called [`use_this_program`](Self::use_this_program),
    /// otherwise `xirius-plugin-scan` next to this program, otherwise `xirius` or `xirius-ui`
    /// next to it (with `__scan-plugin`). A test binary in `target/<profile>/deps` also looks
    /// one folder up.
    pub fn locate() -> Option<ChildProgram> {
        if let Some(p) = std::env::var_os("XIRIUS_PLUGIN_SCAN") {
            return Some(ChildProgram {
                program: p.into(),
                args: Vec::new(),
            });
        }
        if let Some(p) = OWN.get() {
            return Some(p.clone());
        }
        let exe = std::env::current_exe().ok()?;
        let mut dirs: Vec<PathBuf> = exe.parent().map(Path::to_path_buf).into_iter().collect();
        if let Some(d) = dirs.first().cloned()
            && d.file_name().is_some_and(|n| n == "deps")
            && let Some(up) = d.parent()
        {
            dirs.push(up.to_path_buf());
        }
        let exe_name = |stem: &str| format!("{stem}{}", std::env::consts::EXE_SUFFIX);
        for d in &dirs {
            let p = d.join(exe_name("xirius-plugin-scan"));
            if p.is_file() {
                return Some(ChildProgram {
                    program: p,
                    args: Vec::new(),
                });
            }
        }
        for stem in ["xirius", "xirius-ui"] {
            for d in &dirs {
                let p = d.join(exe_name(stem));
                if p.is_file() {
                    return Some(ChildProgram {
                        program: p,
                        args: vec![CHILD_ARG.into()],
                    });
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub dirs: Vec<PathBuf>,
    pub cache: PathBuf,
    pub program: Option<ChildProgram>,
    /// Open every bundle again, cached or not.
    pub full: bool,
    /// Bundles to open again, cached or not (the list's retry button).
    pub retry: Vec<PathBuf>,
    /// How many bundles are opened at once.
    pub jobs: usize,
    /// How long one bundle may take before its process is stopped.
    pub timeout: Duration,
    /// Passed on to every scan process (the tests' `--crash-on` and `--hang-on`).
    pub extra_args: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        ScanOptions {
            dirs: standard_dirs(),
            cache: default_cache_path(),
            program: ChildProgram::locate(),
            full: false,
            retry: Vec::new(),
            jobs: cores.clamp(1, 4),
            timeout: Duration::from_secs(30),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanReport {
    pub catalog: Catalog,
    /// Bundles found.
    pub bundles: usize,
    /// Taken from the cache unopened.
    pub cached: usize,
    /// Opened this time.
    pub probed: usize,
    /// Of those opened, the ones that gave no plugins.
    pub failed: usize,
    pub elapsed: Duration,
    /// Stopped before every bundle was opened ([`scan_until`]).
    pub stopped: bool,
}

pub fn scan(opts: &ScanOptions) -> Result<ScanReport, String> {
    scan_with(opts, &|_, _| {})
}

/// Like [`scan`], calling `progress(done, total)` as bundles are opened.
pub fn scan_with(
    opts: &ScanOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ScanReport, String> {
    scan_until(opts, progress, &|| false)
}

/// Like [`scan_with`], stopping as soon as `stop()` says so (PLG-021): no more bundles are
/// opened and the processes still opening one are ended. The list keeps the bundles opened
/// by then; the ones not reached keep what the cache had for them, and the next scan opens
/// them.
pub fn scan_until(
    opts: &ScanOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<ScanReport, String> {
    scan_seeing(opts, progress, &|_| {}, stop)
}

/// Like [`scan_until`], calling `seeing(bundle)` as each bundle starts to open (PLG-059: the
/// file the plugin page shows).
pub fn scan_seeing(
    opts: &ScanOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
    seeing: &(dyn Fn(&Path) + Sync),
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<ScanReport, String> {
    let start = Instant::now();
    let mut bundles: Vec<PathBuf> = Vec::new();
    for d in &opts.dirs {
        for b in xirius_vst3::find_bundles(d)
            .into_iter()
            .chain(xirius_clap::find_bundles(d))
        {
            if !bundles.contains(&b) {
                bundles.push(b);
            }
        }
    }
    let old = CacheFile::read(&opts.cache);
    let mut records: Vec<Option<BundleRecord>> = Vec::with_capacity(bundles.len());
    let mut todo: Vec<(usize, String)> = Vec::new();
    for (i, b) in bundles.iter().enumerate() {
        let fp = fingerprint(b);
        match old.get(b) {
            Some(r) if !opts.full && !opts.retry.contains(b) && r.fingerprint == fp => {
                records.push(Some(r.clone()))
            }
            _ => {
                records.push(None);
                todo.push((i, fp));
            }
        }
    }
    let cached = bundles.len() - todo.len();
    let mut opened = Vec::new();
    let unlaunched = AtomicUsize::new(0);
    if !todo.is_empty() {
        let program = opts
            .program
            .as_ref()
            .ok_or("プラグインのスキャン用プログラム（xirius-plugin-scan）が見つかりません")?;
        let desktop = Desktop::hidden();
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let found = Mutex::new(Vec::<(usize, BundleRecord)>::new());
        progress(0, todo.len());
        std::thread::scope(|s| {
            for _ in 0..opts.jobs.clamp(1, todo.len()) {
                s.spawn(|| {
                    while !stop() {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some((i, fp)) = todo.get(k) else { break };
                        seeing(&bundles[*i]);
                        let Some(r) = open(
                            program,
                            &bundles[*i],
                            fp,
                            opts,
                            desktop.as_ref(),
                            stop,
                            &unlaunched,
                        ) else {
                            break;
                        };
                        found.lock().unwrap().push((*i, r));
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(n, todo.len());
                    }
                });
            }
        });
        for (i, r) in found.into_inner().unwrap() {
            records[i] = Some(r);
            opened.push(i);
        }
        for (i, _) in &todo {
            if records[*i].is_none() {
                records[*i] = old.get(&bundles[*i]).cloned();
            }
        }
    }
    let file = CacheFile {
        version: 0,
        bundles: records.into_iter().flatten().collect(),
    };
    // A scan that is itself broken (the scan program does not start, or nearly every bundle
    // fails) says nothing about the plugins: the list that was there stays.
    let failed_now = opened
        .iter()
        .filter(|i| file.get(&bundles[**i]).is_some_and(|r| r.failure.is_some()))
        .count();
    let unlaunched = unlaunched.load(Ordering::Relaxed);
    if !opened.is_empty() && unlaunched == opened.len() {
        return Err(format!(
            "スキャン用プログラム（{}）を起動できないため、プラグインの一覧は更新しませんでした",
            opts.program
                .as_ref()
                .map_or(String::new(), |p| p.program.display().to_string())
        ));
    }
    if opened.len() >= BROKEN_MIN && failed_now * 2 > opened.len() {
        return Err(format!(
            "{} 件中 {} 件のスキャンが失敗したため、スキャンが壊れているとみなして一覧は更新しませんでした",
            opened.len(),
            failed_now
        ));
    }
    file.write(&opts.cache)
        .map_err(|e| format!("{}: {e}", opts.cache.display()))?;
    let failed = failed_now;
    Ok(ScanReport {
        catalog: file.catalog(),
        bundles: bundles.len(),
        cached,
        probed: opened.len(),
        failed,
        elapsed: start.elapsed(),
        stopped: opened.len() < todo.len(),
    })
}

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Opens one bundle in a scan process and turns what happened into a record; none when the
/// scan was stopped while it was open.
fn open(
    program: &ChildProgram,
    bundle: &Path,
    fp: &str,
    opts: &ScanOptions,
    desktop: Option<&Desktop>,
    stop: &dyn Fn() -> bool,
    unlaunched: &AtomicUsize,
) -> Option<BundleRecord> {
    let start = Instant::now();
    let out = std::env::temp_dir().join(format!(
        "xirius-scan-{}-{}.json",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&out);
    let mut args = program.args.clone();
    args.extend([
        "--bundle".into(),
        bundle.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ]);
    args.extend(opts.extra_args.iter().cloned());
    let exit = spawn::run(&program.program, &args, opts.timeout, desktop, stop);
    if matches!(exit, Ok(Exit::Stopped)) {
        let _ = std::fs::remove_file(&out);
        return None;
    }
    if exit.is_err() {
        unlaunched.fetch_add(1, Ordering::Relaxed);
    }
    let answer = std::fs::read(&out)
        .ok()
        .and_then(|b| serde_json::from_slice::<Answer>(&b).ok());
    let _ = std::fs::remove_file(&out);
    let fail = |kind, error: String| Failure {
        path: bundle.to_path_buf(),
        kind,
        error,
    };
    let (entries, failure) = match (exit, answer) {
        (
            Ok(_),
            Some(Answer {
                entries: Some(e), ..
            }),
        ) => (e, None),
        (Ok(_), Some(Answer { error: Some(e), .. })) => {
            (Vec::new(), Some(fail(FailureKind::Failed, e)))
        }
        (Ok(Exit::TimedOut | Exit::Stopped), _) => (
            Vec::new(),
            Some(fail(
                FailureKind::Timeout,
                format!(
                    "{} 秒以内に応答が無かったため中止しました",
                    opts.timeout.as_secs_f32()
                ),
            )),
        ),
        (Ok(Exit::Code(code)), _) => (
            Vec::new(),
            Some(fail(
                FailureKind::Crashed,
                format!("結果を返さずに終了しました（終了コード 0x{code:08X}）"),
            )),
        ),
        (Err(e), _) => (
            Vec::new(),
            Some(fail(
                FailureKind::Failed,
                format!("{} を起動できません: {e}", program.program.display()),
            )),
        ),
    };
    Some(BundleRecord {
        path: bundle.to_path_buf(),
        fingerprint: fp.to_string(),
        entries,
        failure,
        probe_ms: start.elapsed().as_millis() as u64,
    })
}
