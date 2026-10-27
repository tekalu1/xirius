//! Audition the written files independently of the editable song.
use crate::{
    host::{HostConfig, SessionHost},
    output::{Output, OutputMode},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use xirius_core::{
    id::{ClipId, Node, TrackId},
    model::{Clip, ClipBody, Project, Track, TrackKind},
};

pub struct Preview {
    pub job: String,
    host: Option<SessionHost>,
    output: Option<Output>,
    end: u64,
    started: Instant,
    heard: bool,
    peak: f32,
}

impl Preview {
    pub fn start(
        job: &str,
        files: &[PathBuf],
        mode: OutputMode,
        settings: Option<xirius_prefs::Shared>,
    ) -> Result<Self, String> {
        let mut project = Project::new("試聴");
        let mut seconds = 0.0f64;
        for path in files {
            let info = crate::decode::probe(path)?;
            seconds = seconds.max(info.seconds());
            let id = TrackId(project.alloc());
            let clip = ClipId(project.alloc());
            let mut t = Track::new(id, TrackKind::Audio, "試聴");
            t.clips.push(Clip {
                id: clip,
                name: "試聴".into(),
                color: None,
                take: None,
                controllers: vec![],
                start: 0,
                length: (info.seconds() * 2.0 * f64::from(project.ppq))
                    .ceil()
                    .max(1.0) as i64,
                muted: false,
                body: ClipBody::Audio {
                    file: path.display().to_string(),
                    offset: 0,
                    gain_db: 0.0,
                    process: Default::default(),
                },
            });
            project.tracks.insert(id, Arc::new(t));
            project.root.push(Node::Track(id));
        }
        if files.is_empty() {
            return Err("書き出したファイルがありません".into());
        }
        let (mut host, audio, _) = SessionHost::start(&project, HostConfig::default())
            .map_err(|e| format!("試聴を開始できません: {e:?}"))?;
        host.io_settings = settings.clone();
        crate::io_store::apply(&mut host)?;
        let prefs = settings.map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).prefs.clone());
        let output = match prefs {
            Some(prefs) => Output::configured(mode, audio, &prefs),
            None => Output::start(mode, audio),
        }?
        .ok_or("オーディオ出力がありません")?;
        if !host.seek_frame(0) || !host.play() {
            return Err("試聴を開始できません".into());
        }
        let end = (seconds * f64::from(host.rate())).round() as u64;
        Ok(Self {
            job: job.into(),
            host: Some(host),
            output: Some(output),
            end,
            started: Instant::now(),
            heard: false,
            peak: 0.0,
        })
    }

    pub fn poll(&mut self) -> Value {
        let host = self.host.as_mut().expect("live preview");
        if crate::host::on_window_thread() {
            host.set_windowed();
        }
        let status = host.poll();
        self.heard |= status.playing;
        for level in host.take_levels().values() {
            self.peak = self.peak.max(level.peak[0]).max(level.peak[1]);
        }
        let ended = status.frame >= self.end
            || (!status.playing
                && (self.heard || self.started.elapsed() > Duration::from_secs(60)));
        json!({"job":self.job,"playing":!ended,"frame":status.frame,"frames":self.end,"peak":self.peak})
    }
}

fn retiring() -> &'static std::sync::Mutex<Vec<crate::background::Job<()>>> {
    static RETIRING: std::sync::OnceLock<std::sync::Mutex<Vec<crate::background::Job<()>>>> =
        std::sync::OnceLock::new();
    RETIRING.get_or_init(Default::default)
}
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn retirement_ready() -> Result<bool, String> {
    if FAILED.load(std::sync::atomic::Ordering::Acquire) {
        return Err("試聴の出力を閉じられません。資源を保持しています".into());
    }
    let Ok(mut tasks) = retiring().try_lock() else {
        return Ok(false);
    };
    tasks.retain(|task| !task.finished());
    Ok(tasks.is_empty())
}
impl Drop for Preview {
    fn drop(&mut self) {
        let output = self.output.take();
        let host = self.host.take();
        if crate::host::on_window_thread() {
            match crate::background::Job::spawn("preview-retire", move || {
                #[cfg(feature = "test-io")]
                let _ = xirius_core::io_test::before(
                    "export_output_stop",
                    std::path::Path::new("preview"),
                );
                drop(output);
                drop(host);
                Ok(())
            }) {
                Ok(task) => {
                    // No value is claimed; finished must acknowledge cleanup,
                    // rather than leave the worker waiting in READY forever.
                    task.cancel();
                    retiring()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(task);
                }
                Err(_) => FAILED.store(true, std::sync::atomic::Ordering::Release),
            }
        } else {
            drop(output);
            drop(host);
        }
    }
}

#[cfg(all(test, feature = "test-io"))]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex, mpsc};
    #[test]
    fn preview_stop_retires_output_off_ui_and_holds_new_output_on_worker() {
        let root = tempfile::tempdir().unwrap();
        let wav = root.path().join("preview.wav");
        crate::export::write_wav(&wav, &[0.1; 48000], &[0.1; 48000], 48000, 16).unwrap();
        let preview =
            Preview::start("fixture", &[wav], OutputMode::Silent { paced: true }, None).unwrap();
        let ui = std::thread::current().id();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let (tx, entered) = mpsc::channel();
        let once = std::sync::atomic::AtomicBool::new(false);
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
            if op == "export_output_stop" && !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        crate::host::mark_window_thread();
        drop(preview);
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        for _ in 0..32 {
            assert!(!retirement_ready().unwrap());
        }
        drop(release);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !retirement_ready().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
