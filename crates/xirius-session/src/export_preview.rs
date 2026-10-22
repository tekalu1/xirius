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
    host: SessionHost,
    _output: Output,
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
            host,
            _output: output,
            end,
            started: Instant::now(),
            heard: false,
            peak: 0.0,
        })
    }

    pub fn poll(&mut self) -> Value {
        let status = self.host.poll();
        self.heard |= status.playing;
        for level in self.host.take_levels().values() {
            self.peak = self.peak.max(level.peak[0]).max(level.peak[1]);
        }
        let ended = status.frame >= self.end
            || (!status.playing
                && (self.heard || self.started.elapsed() > Duration::from_secs(60)));
        json!({"job":self.job,"playing":!ended,"frame":status.frame,"frames":self.end,"peak":self.peak})
    }
}
