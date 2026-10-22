//! Audio and MIDI panes, painted from the session's device observations.
use super::panes::{choices, flip, row, switch_row};
use super::parts;
use super::scene::{self, B};
use super::{Do, MenuKind, SettingsUi};
use crate::draw::Align;
use crate::studio::Rect;
use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::devices::BUFFERS;
use xirius_prefs::Settings;
use xirius_prefs::midimap::Target;
use xirius_session::audio_settings::PC_KEYBOARD;

/// The tip on the buffer sizes while the driver is WASAPI shared mode.
pub const SHARED_BUFFER: &str = "共有モードでは Windows がバッファサイズを決めます";

/// The same input rows as accessible radios; selection is shared with pointer and arrow input.
pub(crate) fn radios(
    s: &crate::studio::Studio,
    g: &crate::studio::Geometry,
) -> Vec<(usize, gpui::accesskit::Node)> {
    if !s
        .settings
        .as_ref()
        .is_some_and(|ui| ui.section == super::Section::Audio)
    {
        return Vec::new();
    }
    let io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
    let Some(device) = io.view.devices.iter().find(|d| d.name == io.view.device) else {
        return Vec::new();
    };
    let rows: Vec<_> = device
        .inputs
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, name)| (i, name, io.view.selected == Some(i)))
        .collect();
    drop(io);
    let scene = s.settings_scene(g);
    rows.into_iter()
        .filter_map(|(i, name, selected)| {
            let r = scene.find(&Do::Input(i))?.r;
            let mut node = gpui::accesskit::Node::new(gpui::Role::RadioButton);
            node.set_label(name);
            node.set_toggled(if selected {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            });
            node.set_bounds(gpui::accesskit::Rect::new(
                r.x as f64,
                (r.y + xirius_design::Layout::TOP_BAR_H) as f64,
                r.right() as f64,
                (r.bottom() + xirius_design::Layout::TOP_BAR_H) as f64,
            ));
            Some((i, node))
        })
        .collect()
}
pub fn audio(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let fc = super::faces(c);
    let mut y = top;
    for (label, key, fw) in [
        ("ドライバー", "audio.driver", 240.0),
        ("オーディオデバイス", "audio.device", 320.0),
    ] {
        if key == "audio.driver" && (s.os == xirius_prefs::Os::Mac || !cfg!(windows)) {
            continue;
        }
        let r = row(x, y, w);
        parts::label(
            b,
            Rect::new(r.x, r.y, parts::LABEL_W, r.h),
            None,
            label,
            None,
        );
        let shown = if key == "audio.device" {
            ui.io.device.clone()
        } else {
            let now = s.prefs.text(key);
            choices(key)
                .into_iter()
                .find(|(v, _)| *v == now)
                .map(|(_, l)| l)
                .unwrap_or(now)
        };
        let f = Rect::new(r.x + parts::LABEL_W, r.y + 6.0, fw, 28.0);
        b.field(f, &shown, true);
        b.item(f, Do::Menu(MenuKind::Pref(key.into())), None);
        y = r.bottom();
    }
    if ui.io.devices.iter().any(|d| d.name == ui.io.device) {
        y = tone_row(b, ui, x, y, w);
    }
    y += 6.0;
    // The inputs on the left, the latency on the right.
    let gap = 16.0;
    let lw = ((w - gap) * 1.3 / 2.3).floor();
    let rx = x + lw + gap;
    let rw = w - lw - gap;
    let head = Rect::new(x, y, lw, 36.0);
    b.text(
        "入力",
        FontSize::Caption,
        Weight::Semibold,
        c.ink,
        head,
        Align::Left,
    );
    let inputs = ui
        .io
        .devices
        .iter()
        .find(|d| d.name == ui.io.device)
        .map(|d| d.inputs.as_slice())
        .unwrap_or_default();
    // Without an input there is nothing to detect: the button is shown dim and does nothing.
    let can_detect = !inputs.is_empty();
    let lt = "入力を検出";
    let bw = 12.0 + 16.0 + 8.0 + scene::width(lt, FontSize::Small) + 12.0;
    let br = Rect::new(head.right() - bw, head.y, bw, 36.0);
    let hot = can_detect && b.hot(br);
    // While it listens the face pulses; the ink on the fill stays the fill's ink (UI-309).
    let pulse = ui.detect.is_some() && (ui.t() * 2.0).fract() < 0.5;
    let face = if !can_detect {
        c.surface_3
    } else if hot || pulse {
        c.fill_hover
    } else {
        c.fill
    };
    b.round(br, face, Radius::S);
    let ink = if can_detect { c.on_fill } else { c.ink };
    b.icon("mic", parts::icon_at(br.x + 12.0, br), ink);
    b.text(
        lt,
        FontSize::Small,
        Weight::Regular,
        ink,
        Rect::new(br.x + 36.0, br.y, bw - 40.0, br.h),
        Align::Left,
    );
    if can_detect {
        b.item(br, Do::Detect, None);
    }
    let mut iy = head.bottom() + 8.0;
    if inputs.is_empty() {
        none_row(b, Rect::new(x, iy, lw, 38.0));
    }
    for (i, name) in inputs.iter().enumerate() {
        let what = if name.contains('+') {
            "ステレオ"
        } else {
            "モノ"
        };
        let icon = "mic";
        let r = Rect::new(x, iy, lw, 38.0);
        if ui.io.selected == Some(i) {
            b.ring(r, c.ink_blue, fc.well, Radius::S, 1.5);
        } else {
            b.round(r, fc.well, Radius::S);
        }
        let mut cx = r.x + 8.0;
        b.icon(icon, Rect::new(cx + 2.0, r.y + 12.0, 14.0, 14.0), c.ink);
        cx += 18.0 + 8.0;
        b.text(
            name,
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            Rect::new(cx, r.y + 4.0, 96.0, 16.0),
            Align::Left,
        );
        b.text(
            what,
            FontSize::Micro,
            Weight::Regular,
            c.ink,
            Rect::new(cx, r.y + 20.0, 96.0, 14.0),
            Align::Left,
        );
        cx += 96.0 + 8.0;
        let mw = r.right() - 8.0 - 18.0 - 8.0 - 34.0 - 8.0 - cx;
        let m = Rect::new(cx, r.y + 15.0, mw.max(10.0), 8.0);
        let v = ui.io.levels.get(i).copied().unwrap_or(0.0) as f64;
        meter(b, m, v as f32);
        let db = if v > 0.0 {
            format!("{:.0}", 20.0 * v.log10())
        } else {
            "−∞".into()
        };
        b.digits(
            &db,
            FontSize::Micro,
            Weight::Regular,
            c.ink,
            Rect::new(m.right() + 8.0, r.y, 34.0, r.h),
            Align::Right,
        );
        if ui.io.selected == Some(i) {
            b.icon(
                "check",
                Rect::new(r.right() - 8.0 - 16.0, r.y + 12.0, 14.0, 14.0),
                c.ink,
            );
        }
        if ui.io.detected.contains(&i) {
            b.round(
                Rect::new(r.right() - 38.0, r.y + 16.0, 6.0, 6.0),
                c.ok,
                Radius::Pill,
            );
        }
        b.item(r, Do::Input(i), None);
        iy = r.bottom() + 2.0;
    }
    // レイテンシ.
    let buf = s.prefs.number("audio.buffer");
    let ms = ui.io.latency.map_or(0.0, |l| l.total_ms());
    let lat = Rect::new(
        rx,
        y + 6.0,
        rw,
        12.0 * 2.0 + 18.0 + 12.0 + 30.0 + 12.0 + 16.0 + 12.0 + 6.0 + 12.0 + 28.0,
    );
    b.round(lat, c.surface_2, Radius::S);
    let ix = lat.x + 12.0;
    let iw = lat.w - 24.0;
    let mut ly = lat.y + 12.0;
    let lab = Rect::new(ix, ly, iw, 18.0);
    b.icon("latency", parts::small_icon_at(ix, lab), c.ink);
    b.text(
        "レイテンシ",
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(ix + 20.0, ly, iw - 20.0, 18.0),
        Align::Left,
    );
    ly += 18.0 + 12.0;
    let num = ui
        .io
        .latency
        .map_or_else(|| "—".into(), |l| format!("{:.1}", l.total_ms()));
    let nw = b.digits(
        &num,
        FontSize::Readout,
        Weight::Semibold,
        c.ink_strong,
        Rect::new(ix, ly, iw, 30.0),
        Align::Left,
    );
    b.text(
        "ms",
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(ix + nw + 4.0, ly + 4.0, 40.0, 26.0),
        Align::Left,
    );
    ly += 30.0 + 12.0;
    // The scale: 0 to 40 ms, green to a quarter, amber to a half.
    let g = Rect::new(ix, ly + 3.0, iw, 10.0);
    let q = (g.w / 4.0).round();
    b.round(g, c.over, Radius::Pill);
    b.round(Rect::new(g.x, g.y, q * 2.0, g.h), c.hot, Radius::Pill);
    b.fill(Rect::new(g.x + q, g.y, q, g.h), c.hot);
    b.round(Rect::new(g.x, g.y, q, g.h), c.ok, Radius::Pill);
    b.fill(Rect::new(g.x + q - 5.0, g.y, 5.0, g.h), c.ok);
    if ui.io.latency.is_some() {
        let nx = g.x + (ms as f32 / 40.0).min(1.0) * g.w;
        b.round(
            Rect::new(nx - 4.0, g.y - 7.0, 8.0, 24.0),
            c.surface_2,
            Radius::Xs,
        );
        b.round(
            Rect::new(nx - 2.0, g.y - 5.0, 4.0, 20.0),
            c.ink_strong,
            Radius::Xs,
        );
    }
    ly += 16.0;
    let marks = ["0", "10", "20", "30", "40 ms"];
    for (i, m) in marks.iter().enumerate() {
        let align = match i {
            0 => Align::Left,
            4 => Align::Right,
            _ => Align::Center,
        };
        let cxm = ix + iw * i as f32 / 4.0;
        let r = match align {
            Align::Left => Rect::new(ix, ly, 40.0, 12.0),
            Align::Right => Rect::new(ix + iw - 60.0, ly, 60.0, 12.0),
            Align::Center => Rect::new(cxm - 20.0, ly, 40.0, 12.0),
        };
        b.digits(m, FontSize::Micro, Weight::Regular, c.ink, r, align);
    }
    ly += 12.0 + 12.0;
    // In, out and the processing.
    let [input, output, conversion] = ui
        .io
        .latency
        .map_or([0.0; 3], |l| l.parts_ms())
        .map(|v| v as f32);
    let total = (input + output + conversion).max(f32::EPSILON);
    let bw = iw - 4.0;
    let mut bx = ix;
    for (part, alpha) in [(input, 0.75), (output, 0.45), (conversion, 0.25)] {
        if part <= 0.0 {
            continue;
        }
        let pw = bw * part / total;
        let mut ink = c.ink_blue;
        ink.a *= alpha;
        b.round(Rect::new(bx, ly, pw, 6.0), ink, Radius::Xs);
        bx += pw + 2.0;
    }
    b.tip(Rect::new(ix, ly - 4.0, iw, 14.0), "入力 / 出力 / 変換");
    ly += 6.0 + 12.0;
    // In shared mode the Windows audio engine sets the buffer: the sizes are faint, none is on.
    let shared = s.prefs.text("audio.driver") == "wasapi-shared";
    let segs: Vec<String> = BUFFERS.iter().map(|n| n.to_string()).collect();
    let tips: Vec<String> = BUFFERS.iter().map(|n| format!("{n} サンプル")).collect();
    let items: Vec<parts::Seg> = segs
        .iter()
        .zip(&tips)
        .map(|(l, t)| parts::Seg {
            label: l,
            icon: None,
            act: Do::Pref("audio.buffer".into(), l.clone()),
            on: !shared && (buf as i64).to_string() == *l,
            tip: (!shared).then_some(t.as_str()),
        })
        .collect();
    b.dim = shared;
    let tray = parts::seg_with(b, ix, ly, &items, 24.0, 2.0, 0.0, 6.0);
    b.dim = false;
    if shared {
        // A press on the faint sizes does nothing (the settings stay open) and says why.
        b.item(tray, Do::Nothing, Some(SHARED_BUFFER.into()));
    }
    let mut ry = lat.bottom() + 8.0;
    let r = row(rx, ry, rw);
    parts::label(b, r, None, "録音オフセット", None);
    parts::number(
        b,
        ui,
        r.right(),
        r,
        "audio.record-offset",
        s.prefs.number("audio.record-offset"),
        Some("ms"),
        Some("録音したオーディオの位置をずらします"),
    );
    ry = r.bottom();
    ry.max(iy)
}

/// 「出力」: the test tone's button and the output's meter, left over right (UI-260).
fn tone_row(b: &mut B, ui: &SettingsUi, x: f32, y: f32, w: f32) -> f32 {
    use crate::start::rig::TONE;
    let c = b.c;
    let r = row(x, y, w);
    parts::label(
        b,
        Rect::new(r.x, r.y, parts::LABEL_W, r.h),
        None,
        "出力",
        None,
    );
    let bw = 10.0 + 16.0 + 6.0 + scene::width(TONE, FontSize::Small) + 10.0;
    let br = Rect::new(r.x + parts::LABEL_W, r.y + (r.h - 28.0) / 2.0, bw, 28.0);
    let (face, ink) = if ui.tone {
        (c.fill, c.on_fill)
    } else if b.hot(br) {
        (c.thumb, c.ink_strong)
    } else {
        (c.surface_3, c.ink_strong)
    };
    b.round(br, face, Radius::Clip);
    b.icon("tone", parts::icon_at(br.x + 10.0, br), ink);
    b.text(
        TONE,
        FontSize::Small,
        Weight::Medium,
        ink,
        Rect::new(br.x + 32.0, br.y, bw - 32.0, br.h),
        Align::Left,
    );
    b.item(br, Do::Tone, None);
    let mx = br.right() + 10.0;
    let my = r.y + (r.h - 11.0) / 2.0;
    for (i, a) in ui.out.iter().enumerate() {
        paint_meter(b, Rect::new(mx, my + i as f32 * 7.0, 90.0, 4.0), *a);
    }
    r.bottom()
}

/// The test tone's thin meter: one colour for the whole bar, by how loud it is.
fn paint_meter(b: &mut B, m: Rect, a: f32) {
    use crate::start::rig::level_width;
    let c = b.c;
    b.round(m, c.well, Radius::Xs);
    let lw = m.w * level_width(a);
    if lw > 0.0 {
        let ink = if a >= 0.98 {
            c.over
        } else if a >= 0.5 {
            c.hot
        } else {
            c.ok
        };
        b.round(Rect::new(m.x, m.y, lw, m.h), ink, Radius::Xs);
    }
}

/// A level meter lying down: green, then amber, then red, as far as `v` (0 to 1).
fn meter(b: &mut B, r: Rect, v: f32) {
    let c = b.c;
    b.round(r, c.well, Radius::Clip);
    let v = v.clamp(0.0, 1.0);
    let bands = [(0.0, 0.5, c.ok), (0.5, 0.8, c.hot), (0.8, 1.0, c.over)];
    for (a, z, col) in bands {
        if v > a {
            let x0 = r.x + r.w * a;
            let x1 = r.x + r.w * v.min(z);
            b.fill(Rect::new(x0, r.y, x1 - x0, r.h), col);
        }
    }
    let mut peak = c.ink_strong;
    peak.a *= 0.75;
    let px = r.x + (r.w * (v + 0.03)).min(r.w * 0.99);
    b.fill(Rect::new(px, r.y, 1.0, r.h), peak);
}

/// The row of a list with nothing in it.
fn none_row(b: &mut B, r: Rect) {
    let c = b.c;
    b.text(
        "なし",
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(r.x + 2.0, r.y, r.w - 2.0, r.h),
        Align::Left,
    );
}

// ------------------------------------------------------------------- MIDI

pub fn midi(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let mut y = parts::gh(b, "入力", x, top);
    // The PC's keys first, so the list is never empty (UI-263), then the ports.
    let inputs = std::iter::once((PC_KEYBOARD, "piano")).chain(
        ui.io
            .ports
            .iter()
            .filter(|p| *p != PC_KEYBOARD)
            .map(|p| (p.as_str(), "midi")),
    );
    for (name, icon) in inputs {
        let r = row(x, y, w);
        let key = format!("midi.inputs.{name}");
        let on = s.prefs.bool(&key);
        let busy = on && ui.io.received.iter().any(|p| p == name);
        b.icon(icon, parts::icon_at(r.x + 2.0, r), c.ink);
        let dx = r.x + 2.0 + 16.0 + 8.0;
        let dot = Rect::new(dx, r.y + (r.h - 8.0) / 2.0, 8.0, 8.0);
        b.round(dot, if busy { c.ok } else { c.surface_3 }, Radius::Pill);
        b.tip(Rect::new(dx - 5.0, r.y + 10.0, 18.0, 20.0), "受信中");
        let nx = dx + 8.0 + 8.0;
        let nw = scene::width(name, FontSize::Small).min(r.right() - 80.0 - nx);
        b.text(
            name,
            FontSize::Small,
            Weight::Medium,
            if on { c.ink_strong } else { c.ink },
            Rect::new(nx, r.y, nw, r.h),
            Align::Left,
        );
        if on && let Some(pitch) = ui.io.notes.get(name) {
            b.digits(
                &crate::roll::pitch_name(*pitch),
                FontSize::Caption,
                Weight::Medium,
                c.ink_blue,
                Rect::new(nx + nw + 8.0, r.y, 40.0, r.h),
                Align::Left,
            );
        }
        parts::toggle(b, r.right(), r, on, Do::Pref(key, flip(on)), None);
        y = r.bottom();
    }
    y = parts::gh(b, "接続", x, y);
    y = switch_row(
        b,
        s,
        x,
        y,
        w,
        "piano",
        "インストゥルメントの入力を選択に合わせる",
        "midi.follow-selection",
        Some(
            "選んだインストゥルメントトラックを録音準備にし、ほかのインストゥルメントトラックの録音準備を外します",
        ),
    );
    let r = row(x, y, w);
    parts::label(b, r, Some("latency"), "MIDI 入力オフセット", None);
    parts::number(
        b,
        ui,
        r.right(),
        r,
        "midi.input-offset",
        s.prefs.number("midi.input-offset"),
        Some("ms"),
        None,
    );
    assignments(b, ui, s, x, r.bottom(), w)
}

/// コントローラーの割り当て: one row per knob or pad, with the part it moves.
fn assignments(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let mut y = parts::gh(b, "コントローラーの割り当て", x, top);
    if s.midimap.is_empty() {
        let r = row(x, y, w);
        b.text(
            "未設定",
            FontSize::Small,
            Weight::Regular,
            c.ink,
            r,
            Align::Left,
        );
        return r.bottom();
    }
    for a in s.midimap.rows() {
        let r = row(x, y, w);
        let name = |id: &str| ui.parts.get(id).cloned().unwrap_or_else(|| id.to_string());
        let target = match &a.target {
            Target::Volume(id) => format!("{} · ボリューム", name(id)),
            Target::Pan(id) => format!("{} · パン", name(id)),
            Target::Command(cmd) => cmd.clone(),
        };
        b.text(
            &a.message.label(),
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            Rect::new(r.x + 2.0, r.y, 96.0, r.h),
            Align::Left,
        );
        b.push_mask(Rect::new(r.x + 104.0, r.y, r.w - 104.0 - 36.0, r.h));
        b.text(
            &target,
            FontSize::Small,
            Weight::Regular,
            c.ink_strong,
            Rect::new(r.x + 104.0, r.y, r.w - 104.0 - 40.0, r.h),
            Align::Left,
        );
        b.pop_mask();
        if let Some(port) = &a.port {
            b.tip(Rect::new(r.x, r.y + 8.0, r.w - 40.0, r.h - 16.0), port);
        }
        let end = Rect::new(r.right() - 26.0, r.y + (r.h - 22.0) / 2.0, 22.0, 22.0);
        parts::icon_button(
            b,
            end,
            "trash",
            Do::MidiUnset(a.target.to_string()),
            Some("解除"),
        );
        y = r.bottom();
    }
    y
}
