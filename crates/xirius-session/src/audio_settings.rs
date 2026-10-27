//! Audio/MIDI settings and device observations. Kept outside the song's history.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use xirius_core::command::{ArgDef, Args, CommandDef, Handler, Output};
use xirius_core::{CmdError, CmdResult, ExtContext, Extension, Risk};
use xirius_engine::devices::{BUFFERS, Backend, Device, Latency, Unopened};
mod backend;
mod routes;

pub type Shared = Arc<Mutex<Io>>;

/// The initial interface is shared by the displayed channels and the capture stream: the one
/// chosen before while it is there (`devices` empty means not known, and keeps it), and
/// otherwise [`automatic`] (ENG-061).
pub fn configured_device(prefs: &xirius_prefs::Prefs, devices: &[Device]) -> String {
    let wanted = prefs.text("audio.device");
    if !wanted.is_empty() && (devices.is_empty() || devices.iter().any(|d| d.name == wanted)) {
        wanted
    } else {
        automatic(devices)
    }
}

/// The device a person who has chosen nothing gets (ENG-061): the system's default output
/// (listed first) when it has inputs, and otherwise the first that has both inputs and
/// outputs, so that recording works at once. Without one, the default output.
pub fn automatic(devices: &[Device]) -> String {
    let first = devices.first();
    first
        .filter(|d| !d.inputs.is_empty())
        .or_else(|| {
            devices
                .iter()
                .find(|d| !d.inputs.is_empty() && !d.outputs.is_empty())
        })
        .or(first)
        .map(|d| d.name.clone())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    pub device: String,
    pub levels: Vec<f32>,
    pub selected: Option<usize>,
    pub detected: Vec<usize>,
    pub latency: Option<Latency>,
    pub ports: Vec<String>,
    pub received: Vec<String>,
    /// The last note each MIDI input played (UI-263): it stays after the light goes out.
    pub notes: std::collections::BTreeMap<String, u8>,
    pub output: [usize; 2],
}

pub struct Io {
    windowed: bool,
    files: std::cell::RefCell<Option<routes::Files>>,
    proxy: Option<backend::Proxy>,
    pub view: Snapshot,
    backend: Box<dyn Backend>,
    configured: Option<(String, String, u32)>,
    detect: Option<(Instant, Vec<f32>)>,
    activity: std::collections::BTreeMap<String, Instant>,
}

impl Default for Io {
    fn default() -> Self {
        Self::new(Box::new(Unopened::default()))
    }
}

impl Io {
    pub fn is_windowed(&self) -> bool {
        self.windowed
    }
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            windowed: false,
            files: Default::default(),
            proxy: None,
            view: Snapshot {
                output: [0, 1],
                ..Snapshot::default()
            },
            backend,
            configured: None,
            detect: None,
            activity: Default::default(),
        }
    }
    pub fn replace(&mut self, backend: Box<dyn Backend>) {
        if self.windowed {
            match backend::Proxy::new(move || backend) {
                Ok(proxy) => {
                    self.backend = Box::new(proxy.clone());
                    self.proxy = Some(proxy);
                }
                Err(_) => return,
            }
        } else {
            self.backend = backend;
        }
        self.configured = None;
        self.view = Snapshot {
            output: [0, 1],
            ..Snapshot::default()
        };
        self.detect = None;
        self.activity.clear();
    }
    pub fn sync(&mut self, prefs: &xirius_prefs::Prefs) -> Result<(), String> {
        if self.windowed && self.proxy.is_none() {
            return Err("装置の監視を開始できません".into());
        }
        self.view.devices = self.backend.devices();
        let device = configured_device(prefs, &self.view.devices);
        let driver = prefs.text("audio.driver");
        let buffer = prefs.number("audio.buffer") as u32;
        let config = (driver.clone(), device.clone(), buffer);
        if self.configured.as_ref() != Some(&config) || self.proxy.is_some() {
            let changed_device = self.view.device != device;
            let latency = if device.is_empty() {
                None
            } else {
                self.backend.configure(&driver, &device, buffer)?
            };
            self.view.latency = latency;
            if changed_device {
                self.view.levels.clear();
                self.view.detected.clear();
                self.view.selected = None;
                self.detect = None;
                self.view.output = [0, 1];
            }
            self.view.device = device;
            self.configured = Some(config);
        }
        Ok(())
    }

    pub fn set_windowed(&mut self) -> Result<(), String> {
        self.windowed = true;
        if self.proxy.is_none() {
            let old = std::mem::replace(&mut self.backend, Box::new(Unopened::default()));
            let proxy = backend::Proxy::new(move || old)?;
            self.backend = Box::new(proxy.clone());
            self.proxy = Some(proxy);
        }
        if self.files.borrow().is_none() {
            *self.files.borrow_mut() = Some(routes::Files::new()?);
        }
        Ok(())
    }
    pub fn enumerate_later(&mut self) -> Result<(), String> {
        self.windowed = true;
        let proxy = backend::Proxy::new(|| Box::new(Unopened::enumerate()))?;
        self.backend = Box::new(proxy.clone());
        self.proxy = Some(proxy);
        self.configured = None;
        if self.files.borrow().is_none() {
            *self.files.borrow_mut() = Some(routes::Files::new()?);
        }
        Ok(())
    }
    fn worker_copy(&self) -> Option<Self> {
        let proxy = self.proxy.as_ref()?.clone();
        Some(Self {
            windowed: false,
            files: Default::default(),
            backend: Box::new(proxy),
            proxy: None,
            view: self.view.clone(),
            configured: self.configured.clone(),
            detect: self.detect.clone(),
            activity: self.activity.clone(),
        })
    }
    pub fn poll(&mut self, now: Instant) {
        if self.backend.can_measure() {
            self.view.levels = self.backend.levels();
        }
        if let Some((since, peaks)) = &mut self.detect {
            peaks.resize(self.view.levels.len(), 0.0);
            for (p, v) in peaks.iter_mut().zip(&self.view.levels) {
                *p = p.max(*v);
            }
            self.view.detected = peaks
                .iter()
                .enumerate()
                .filter(|(_, p)| **p >= 0.01)
                .map(|(i, _)| i)
                .collect();
            if now.saturating_duration_since(*since) >= Duration::from_millis(1600) {
                self.view.selected = loudest(peaks).or(self.view.selected);
                self.detect = None;
            }
        }
        self.activity
            .retain(|_, at| now.saturating_duration_since(*at) < Duration::from_millis(250));
        self.view.received = self.activity.keys().cloned().collect();
    }
    pub fn detect(&mut self, now: Instant) {
        // Drain observations from before the button press.
        let _ = self.backend.levels();
        self.view.detected.clear();
        self.detect = Some((now, Vec::new()));
    }
    pub fn observe(&mut self, mut levels: Vec<f32>, latency: Option<Latency>) {
        if self.backend.can_measure() {
            return;
        }
        let count = self
            .view
            .devices
            .iter()
            .find(|d| d.name == self.view.device)
            .map_or(0, |d| d.inputs.len());
        levels.truncate(count);
        self.view.levels = levels;
        if latency.is_some() {
            self.view.latency = latency;
        }
    }
    pub fn receiving(&mut self, port: &str, now: Instant) {
        self.activity.insert(port.into(), now);
    }
    /// `port` played `pitch`: its light comes on and the note is kept.
    pub fn heard(&mut self, port: &str, pitch: u8, now: Instant) {
        self.receiving(port, now);
        self.view.notes.insert(port.into(), pitch);
    }
    pub fn detecting(&self) -> bool {
        self.detect.is_some()
    }
    pub fn load_routes(&mut self, dir: Option<&std::path::Path>) -> Result<(), String> {
        if self.windowed && self.files.borrow().is_none() {
            return Err("入出力設定を読み込めません".into());
        }
        if let Some(dir) = dir
            && let Some(routes) = match self.files.borrow_mut().as_mut() {
                Some(files) => files.load(dir)?,
                None => crate::io_store::read(dir)?,
            }
            && routes.device == self.view.device
            && let Some(device) = self.view.devices.iter().find(|d| d.name == routes.device)
        {
            self.view.selected = routes.input.filter(|&n| n < device.inputs.len());
            if routes.output.iter().all(|&n| n < device.outputs.len()) {
                self.view.output = routes.output;
            }
        }
        Ok(())
    }
    pub fn save_routes(&self, dir: Option<&std::path::Path>) -> Result<(), String> {
        if self.windowed && self.files.borrow().is_none() {
            return Err("入出力設定を保存できません".into());
        }
        if let Some(dir) = dir {
            let routes = crate::io_store::Routes {
                device: self.view.device.clone(),
                input: self.view.selected,
                output: self.view.output,
            };
            if let Some(files) = self.files.borrow_mut().as_mut() {
                files.save(dir, routes)?;
            } else {
                crate::io_store::write(dir, &routes)?;
            }
        }
        Ok(())
    }
    pub fn input_name(&self) -> Option<String> {
        let d = self
            .view
            .devices
            .iter()
            .find(|d| d.name == self.view.device)?;
        d.inputs.get(self.view.selected?).cloned()
    }
}

pub fn loudest(levels: &[f32]) -> Option<usize> {
    levels
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_finite() && **p >= 0.01)
        .max_by(|(a, x), (b, y)| x.total_cmp(y).then_with(|| b.cmp(a)))
        .map(|(i, _)| i)
}

pub const NAMES: &[&str] = &[
    "audio.get",
    "audio.set",
    "audio.input.select",
    "audio.input.detect",
    "audio.output.set",
    "audio.devices",
    "midi.get",
    "midi.set",
];

/// The name the PC's keys go by in the list of MIDI inputs (UI-263): always first, so the list
/// is never empty.
pub const PC_KEYBOARD: &str = "PC のキーボード";

pub fn definitions() -> Vec<CommandDef> {
    NAMES
        .iter()
        .map(|&name| {
            let args = match name {
                "audio.set" | "midi.set" => vec![
                    ArgDef::text("key", "設定の名前").req(),
                    ArgDef::text("value", "値").req(),
                ],
                "audio.input.select" => vec![
                    ArgDef::int("channel", "入力（1 から）")
                        .req()
                        .range(1.0, 32.0),
                ],
                "audio.input.detect" => vec![ArgDef::flag("async", "検出の完了を待たずに戻る")],
                "audio.devices" => vec![ArgDef::flag("inputs", "入力のあるデバイスだけを表示する")],
                "audio.output.set" => vec![
                    ArgDef::int("left", "左のチャンネル（1 から）")
                        .req()
                        .range(1.0, 32.0),
                    ArgDef::int("right", "右のチャンネル（1 から）")
                        .req()
                        .range(1.0, 32.0),
                ],
                _ => vec![],
            };
            CommandDef {
                name,
                summary: if name == "audio.devices" {
                    "オーディオデバイスの一覧と、それぞれの入力と出力の数を表示します"
                } else {
                    "オーディオ・MIDI の設定"
                },
                args,
                history: false,
                risk: if name.ends_with("get") || name == "audio.devices" {
                    Risk::Read
                } else {
                    Risk::Write
                },
                handler: Handler::Session,
                screen_only: false,
            }
        })
        .collect()
}

pub struct IoExt {
    pub io: Shared,
    pub settings: xirius_prefs::Shared,
}

impl Extension for IoExt {
    fn answers(&self, name: &str) -> bool {
        NAMES.contains(&name)
    }
    fn run(
        &mut self,
        _: &ExtContext<'_>,
        def: &CommandDef,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        let (local, fence) = {
            let io = self.io.lock().unwrap_or_else(|e| e.into_inner());
            if io.windowed && crate::host::on_window_thread() {
                return Err(CmdError::conflict("装置の設定は裏で行う必要があります"));
            }
            if io.windowed && io.proxy.is_none() {
                return Err(CmdError::conflict("装置の監視を開始できません"));
            }
            let fence = io.files.borrow().as_ref().map(routes::Files::fence);
            (io.worker_copy(), fence)
        };
        if let Some(mut io) = local {
            if let Some(fence) = fence {
                fence.wait().map_err(CmdError::io)?;
            }
            let mut settings = xirius_prefs::Settings::command_snapshot(&self.settings);
            let result = Self::run_local(def, args, dry, &mut settings, &mut io)?;
            self.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .adopt_worker(settings);
            let mut current = self.io.lock().unwrap_or_else(|e| e.into_inner());
            current.view = io.view;
            current.configured = io.configured;
            current.detect = io.detect;
            if let Some(files) = current.files.borrow_mut().as_mut() {
                files.invalidate();
            }
            return Ok(result);
        }
        let mut settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        let mut io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        settings.reload_if_changed();
        Self::run_local(def, args, dry, &mut settings, &mut io)
    }
}
impl IoExt {
    fn run_local(
        def: &CommandDef,
        args: &Args,
        dry: bool,
        settings: &mut xirius_prefs::Settings,
        io: &mut Io,
    ) -> CmdResult<Output> {
        io.sync(&settings.prefs).map_err(CmdError::conflict)?;
        io.load_routes(settings.dir.as_deref())
            .map_err(CmdError::conflict)?;
        match def.name {
            "audio.set" | "midi.set" => {
                let prefix = def.name.split('.').next().unwrap();
                let key = args.str("key")?;
                let key = if key.starts_with(&format!("{prefix}.")) {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                let value = args.str("value")?;
                let mut next = settings.clone();
                next.prefs
                    .set(&key, &json!(value))
                    .map_err(CmdError::invalid)?;
                if key == "audio.buffer" && !BUFFERS.contains(&(next.prefs.number(&key) as u32)) {
                    return Err(CmdError::invalid(
                        "バッファサイズは 32・64・128・256・512・1024 から選択してください",
                    ));
                }
                if key == "audio.device" && !io.view.devices.iter().any(|d| d.name == value) {
                    return Err(CmdError::invalid("オーディオデバイスが見つかりません"));
                }
                if !dry {
                    io.sync(&next.prefs).map_err(CmdError::conflict)?;
                    if let Err(e) = next.save(xirius_prefs::File::Prefs) {
                        let _ = io.sync(&settings.prefs);
                        return Err(CmdError::conflict(e));
                    }
                    next.rev += 1;
                    *settings = next;
                }
            }
            "audio.input.select" => {
                let n = args.i64("channel")? as usize - 1;
                let count = io
                    .view
                    .devices
                    .iter()
                    .find(|d| d.name == io.view.device)
                    .map_or(0, |d| d.inputs.len());
                if n >= count {
                    return Err(CmdError::invalid("入力が見つかりません"));
                }
                if !dry {
                    let before = io.view.selected;
                    io.view.selected = Some(n);
                    if let Err(e) = io.save_routes(settings.dir.as_deref()) {
                        io.view.selected = before;
                        return Err(CmdError::conflict(e));
                    }
                }
            }
            "audio.output.set" => {
                let pair = [
                    args.i64("left")? as usize - 1,
                    args.i64("right")? as usize - 1,
                ];
                let count = io
                    .view
                    .devices
                    .iter()
                    .find(|d| d.name == io.view.device)
                    .map_or(0, |d| d.outputs.len());
                if pair.iter().any(|&n| n >= count) {
                    return Err(CmdError::invalid("出力が見つかりません"));
                }
                if !dry {
                    let before = io.view.output;
                    io.view.output = pair;
                    if let Err(e) = io.save_routes(settings.dir.as_deref()) {
                        io.view.output = before;
                        return Err(CmdError::conflict(e));
                    }
                }
            }
            "audio.input.detect" if !dry => {
                if !io.backend.can_measure() && !args.flag("async") {
                    return Err(CmdError::conflict(
                        "入力を検出するには設定画面を開いてください",
                    ));
                }
                if !io.backend.can_measure() && io.view.levels.is_empty() {
                    return Err(CmdError::conflict("オーディオ入力が開始されていません"));
                }
                io.detect(Instant::now());
                if !args.flag("async") {
                    while io.detecting() {
                        io.poll(Instant::now());
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    io.save_routes(settings.dir.as_deref())
                        .map_err(CmdError::conflict)?;
                }
            }
            _ => {}
        }
        let v = &io.view;
        if def.name == "audio.devices" {
            let only_inputs = args.flag("inputs");
            let rows: Vec<&Device> = v
                .devices
                .iter()
                .filter(|d| !only_inputs || !d.inputs.is_empty())
                .collect();
            let message = if rows.is_empty() {
                "オーディオデバイスなし".to_string()
            } else {
                rows.iter()
                    .map(|d| {
                        let mark = if d.name == v.device { "✓ " } else { "  " };
                        format!(
                            "{mark}{}（入力 {} · 出力 {}）",
                            d.name,
                            d.inputs.len(),
                            d.outputs.len()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let data = json!({
                "device": v.device,
                "devices": rows.iter().map(|d| json!({
                    "name": d.name,
                    "inputs": d.inputs.len(),
                    "outputs": d.outputs.len(),
                    "current": d.name == v.device,
                })).collect::<Vec<_>>(),
            });
            return Ok(Output {
                message,
                data,
                ..Output::default()
            });
        }
        let prefix = if def.name.starts_with("midi.") {
            "midi."
        } else {
            "audio."
        };
        let values: serde_json::Map<String, Value> = settings
            .prefs
            .list_all(Some(prefix))
            .into_iter()
            .map(|(key, value, _)| (key, value))
            .collect();
        let message = values
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut data: Value = json!({ "settings": values, "device": v.device, "devices": v.devices.iter().map(|d| json!({"name":d.name,"inputs":d.inputs,"outputs":d.outputs})).collect::<Vec<_>>(), "levels":v.levels, "input":v.selected.map(|n| n+1), "detected":v.detected.iter().map(|n| n+1).collect::<Vec<_>>(), "latency_ms":v.latency.map(|l|l.total_ms()), "parts_ms":v.latency.map(|l|l.parts_ms()), "output":v.output.map(|n|n+1), "ports":v.ports, "received":v.received });
        if def.name.starts_with("midi.") {
            // The PC's keys first, then the ports (UI-263).
            let mut inputs = vec![json!({ "name": PC_KEYBOARD, "kind": "keyboard" })];
            inputs.extend(
                v.ports
                    .iter()
                    .map(|p| json!({ "name": p, "kind": "port", "on": settings.prefs.get(&format!("midi.inputs.{p}")).and_then(|b| b.as_bool()).unwrap_or(true) })),
            );
            data["inputs"] = json!(inputs);
        }
        Ok(Output {
            message,
            data,
            ..Output::default()
        })
    }
}
