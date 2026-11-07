//! Approved crash isolation policy (PLG-022). Recovery runs before any plugin is loaded.
use std::path::{Path, PathBuf};
use xirius_core::model::{Plugin, PluginIsolation};
use xirius_plugins::{Catalog, Entry};
use xirius_prefs::{File, Prefs, Settings};

pub fn legacy_override(
    prefs: &Prefs,
    catalog: Option<&Catalog>,
    x: &Plugin,
) -> Option<PluginIsolation> {
    let unique =
        catalog.is_some_and(|c| c.entries.iter().filter(|e| e.name == x.name).count() == 1);
    if unique {
        match prefs.text(&format!("plugins.process.{}", x.name)).as_str() {
            "shared" => return Some(PluginIsolation::Shared),
            "dedicated" => return Some(PluginIsolation::Dedicated),
            _ => {}
        }
    }
    None
}
pub fn legacy(prefs: &Prefs, catalog: Option<&Catalog>, x: &Plugin) -> PluginIsolation {
    legacy_override(prefs, catalog, x).unwrap_or(x.isolation)
}
/// A format and class pair, preserving CLAP's case-sensitive ID.
pub fn find<'a>(catalog: &'a Catalog, plugin: &Plugin) -> Option<&'a Entry> {
    catalog.entries.iter().find(|e| {
        e.format.eq_ignore_ascii_case(&plugin.format)
            && if plugin.format.eq_ignore_ascii_case("vst3") {
                e.uid.eq_ignore_ascii_case(&plugin.uid)
            } else {
                e.uid == plugin.uid
            }
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recovered {
    pub name: String,
    pub key: String,
}

fn field<'a>(log: &'a str, tag: &str) -> Option<&'a str> {
    let mut lines = log.lines();
    while let Some(line) = lines.next() {
        if line == tag {
            return lines.next();
        }
    }
    None
}
fn module_path(log: &str) -> Option<PathBuf> {
    let hex = field(log, "fault-module-utf16:")?;
    if hex.len() % 4 != 0 || !hex.is_ascii() {
        return None;
    }
    let units: Option<Vec<_>> = (0..hex.len())
        .step_by(4)
        .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).ok())
        .collect();
    String::from_utf16(&units?).ok().map(PathBuf::from)
}
fn same_bundle(module: &Path, bundle: &Path) -> bool {
    // Windows module paths are absolute. Never match by file name or vendor DLL name.
    let m = module
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let b = bundle
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    m == b || m.strip_prefix(&b).is_some_and(|s| s.starts_with('/'))
}
/// Only fatal UI reports with an unambiguous, registered identity are attributed.
pub fn culprit<'a>(log: &str, catalog: &'a Catalog) -> Option<&'a Entry> {
    if !log.lines().any(|l| l.starts_with("app=xirius-ui "))
        || !log.lines().any(|l| l == "fatal: unhandled exception")
    {
        return None;
    }
    let module = module_path(log);
    if module
        .as_ref()
        .and_then(|p| p.file_name())
        .is_some_and(|n| {
            n.to_string_lossy().eq_ignore_ascii_case("xirius-ui.exe")
                || n.to_string_lossy().eq_ignore_ascii_case("xirius.exe")
        })
    {
        return None;
    }
    let active = field(log, "fault-plugin:").and_then(|s| s.split_once(':'));
    if let Some((format, uid)) = active {
        let mut matches = catalog.entries.iter().filter(|e| {
            e.format.eq_ignore_ascii_case(format)
                && if format.eq_ignore_ascii_case("vst3") {
                    e.uid.eq_ignore_ascii_case(uid)
                } else {
                    e.uid == uid
                }
        });
        let entry = matches.next()?;
        if module.as_ref().is_some_and(|m| {
            catalog.entries.iter().any(|other| {
                same_bundle(m, &other.path)
                    && (other.format != entry.format || other.uid != entry.uid)
            })
        }) {
            return None;
        }
        // Multiple installed versions of the same class still have one policy identity.
        return (!entry.ara && !matches.any(|e| e.ara)).then_some(entry);
    }
    let module = module?;
    let mut matches = catalog
        .entries
        .iter()
        .filter(|e| !e.ara && same_bundle(&module, &e.path));
    let entry = matches.next()?;
    matches.next().is_none().then_some(entry)
}

/// The setting and processed record IDs are persisted together in prefs.toml.
/// Saving must succeed before the startup may proceed to loading plugins.
pub fn recover(
    settings: &mut Settings,
    catalog: &Catalog,
    root: &Path,
) -> Result<Vec<Recovered>, String> {
    xirius_engine::crash::tidy_at(root).map_err(|e| e.to_string())?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut reports = Vec::new();
    for entry in entries {
        let folder = entry.map_err(|e| e.to_string())?.path();
        if folder.is_dir() && folder.join("crash.log").is_file() {
            reports.push(folder);
        }
    }
    reports.sort();
    let mut processed = settings.prefs.list("plugins.crash.processed");
    let mut recovered = Vec::new();
    let mut changed = false;
    for report in reports {
        let id = report
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if processed.contains(&id) {
            continue;
        }
        let log = std::fs::read_to_string(report.join("crash.log")).map_err(|e| e.to_string())?;
        // A live/first-chance or host report must never lead to an isolation override.
        if let Some(entry) = culprit(&log, catalog) {
            let key = xirius_prefs::isolation::key(&entry.format, &entry.uid);
            settings.prefs.set(&key, &serde_json::json!("dedicated"))?;
            if !recovered.iter().any(|r: &Recovered| r.key == key) {
                recovered.push(Recovered {
                    name: entry.name.clone(),
                    key,
                });
            }
        }
        processed.push(id);
        changed = true;
    }
    if changed {
        settings
            .prefs
            .set("plugins.crash.processed", &serde_json::json!(processed))?;
        settings.save(File::Prefs)?;
    }
    Ok(recovered)
}

/// Keeps setup, processing and release attributable even when a runtime DLL faults.
/// The identity is prepared off the audio thread; entering the marker never allocates.
pub(crate) struct Attributed {
    identity: String,
    inner: Option<Box<dyn xirius_engine::Processor>>,
}
impl Attributed {
    pub(crate) fn new(plugin: &Plugin, processor: Box<dyn xirius_engine::Processor>) -> Self {
        Self {
            identity: format!("{}:{}", plugin.format.to_ascii_lowercase(), plugin.uid),
            inner: Some(processor),
        }
    }
    fn inner(&self) -> &dyn xirius_engine::Processor {
        self.inner.as_deref().expect("live processor")
    }
    fn inner_mut(&mut self) -> &mut (dyn xirius_engine::Processor + 'static) {
        self.inner.as_deref_mut().expect("live processor")
    }
}
impl Drop for Attributed {
    fn drop(&mut self) {
        let _context = xirius_vst3::crash_context::enter(&self.identity);
        drop(self.inner.take());
    }
}
impl xirius_engine::Processor for Attributed {
    fn name(&self) -> &str {
        self.inner().name()
    }
    fn prepare(&mut self, setup: &xirius_engine::ProcessSetup) {
        let _context = xirius_vst3::crash_context::enter(&self.identity);
        self.inner_mut().prepare(setup);
    }
    fn process(
        &mut self,
        io: &mut xirius_engine::processor::AudioIo<'_>,
        events: &[xirius_engine::processor::NoteEvent],
    ) {
        let _context = xirius_vst3::crash_context::enter(&self.identity);
        self.inner_mut().process(io, events);
    }
    fn latency(&self) -> u32 {
        self.inner().latency()
    }
    fn timeline_only(&self) -> bool {
        self.inner().timeline_only()
    }
    fn child_time(&self) -> Option<(u64, u64)> {
        self.inner().child_time()
    }
    fn begin_prepare(&mut self, setup: &xirius_engine::ProcessSetup) {
        let _context = xirius_vst3::crash_context::enter(&self.identity);
        self.inner_mut().begin_prepare(setup);
    }
    fn finish_prepare(&mut self, setup: &xirius_engine::ProcessSetup) {
        let _context = xirius_vst3::crash_context::enter(&self.identity);
        self.inner_mut().finish_prepare(setup);
    }
    fn preparing(&self) -> bool {
        self.inner().preparing()
    }
    fn allow_late(&mut self) {
        self.inner_mut().allow_late();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::model::PluginKind;
    fn catalog() -> Catalog {
        Catalog {
            entries: vec![Entry {
                uid: "ABCD".into(),
                name: "Fixture".into(),
                vendor: "Fixture".into(),
                kind: PluginKind::Effect,
                format: "vst3".into(),
                category: "Fx".into(),
                version: "1".into(),
                path: PathBuf::from("C:/fixture.vst3"),
                ara: false,
                requires_process_main: false,
            }],
            failures: vec![],
        }
    }
    fn log() -> &'static str {
        "app=xirius-ui version=test\nfatal: unhandled exception\nfault-plugin:\nvst3:ABCD\n"
    }
    #[test]
    fn recovery_is_persistent_once_and_manual_restore_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("crashes");
        let folder = root.join("123-456");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("crash.log"), log()).unwrap();
        std::fs::write(folder.join("crash.dmp"), b"fixture").unwrap();
        let prefs = dir.path().join("prefs");
        let mut settings = Settings::open(&prefs);
        let first = recover(&mut settings, &catalog(), &root).unwrap();
        assert_eq!(first.len(), 1);
        let key = &first[0].key;
        assert_eq!(Settings::open(&prefs).prefs.text(key), "dedicated");
        settings.prefs.set(key, &serde_json::json!("same")).unwrap();
        settings.save(File::Prefs).unwrap();
        let mut settings = Settings::open(&prefs);
        assert!(
            recover(&mut settings, &catalog(), &root)
                .unwrap()
                .is_empty()
        );
        assert_eq!(settings.prefs.text(key), "same");
    }
    #[test]
    fn only_fatal_ui_calls_with_unambiguous_registered_identity_are_attributed() {
        let c = catalog();
        assert!(culprit(log(), &c).is_some());
        for log in [
            "app=xirius-ui version=test\nfatal: unhandled exception\n",
            "panic: failure",
            "fault-plugin:\nvst3:ABCD",
            "app=xirius-plugin-host version=test\nfatal: unhandled exception\nfault-plugin:\nvst3:ABCD",
        ] {
            assert!(culprit(log, &c).is_none());
        }
        assert!(culprit(&log().replace("ABCD", "unknown"), &c).is_none());
        let mut ambiguous = c.clone();
        ambiguous.entries.push(c.entries[0].clone());
        assert!(
            culprit(log(), &ambiguous).is_some(),
            "duplicate installations retain one class identity"
        );
        let mut ara = c;
        ara.entries[0].ara = true;
        assert!(culprit(log(), &ara).is_none());
    }
    #[test]
    fn exact_module_path_matches_but_vendor_dll_and_ambiguous_bundle_do_not() {
        let c = catalog();
        let report = |path: &str| {
            format!(
                "app=xirius-ui version=test\nfatal: unhandled exception\nfault-module-utf16:\n{}\n",
                path.encode_utf16()
                    .map(|u| format!("{u:04x}"))
                    .collect::<String>()
            )
        };
        assert!(
            culprit(
                &report("C:/fixture.vst3/Contents/x86_64-win/fixture.vst3"),
                &c
            )
            .is_some()
        );
        assert!(culprit(&report("C:/vendor.dll"), &c).is_none());
        assert!(culprit(&(log().to_owned() + &report("C:/xirius-ui.exe")), &c).is_none());
        assert!(culprit(&report("C:/fixture.vst3-other"), &c).is_none());
        let mut c = c;
        let mut second = c.entries[0].clone();
        second.uid = "OTHER".into();
        c.entries.push(second);
        assert!(culprit(&report("C:/fixture.vst3"), &c).is_none());
    }
}
