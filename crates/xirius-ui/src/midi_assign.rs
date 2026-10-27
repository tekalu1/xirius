//! OPS-007: the knobs and pads of a controller that move a part's value or run a command.
//!
//! The learn state lives here: a part waits for a knob (`midimap.learn`), the first control change
//! that comes from the live input is given to it, and a knob that already moves another part asks
//! before it is taken over. The assigned messages never reach the notes of the live input; the
//! recording leaves them out too and tells what it left out (`Control::take`), so the part still
//! follows the knob while a take is being made.

use serde_json::{Map, Value, json};
use xirius_core::id::TrackId;
use xirius_prefs::Midimap;
use xirius_prefs::midimap::{Message, Row, Target};

use xirius_design::{FontSize, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::MixerHit;
use crate::studio::{Geometry, Rect, Studio};

/// The line that says a part waits for a knob: its height and the width of its button.
const PILL_H: f32 = 30.0;
const CANCEL_W: f32 = 84.0;

/// A knob that already moves another part: the answer is waited for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clash {
    pub port: Option<String>,
    pub message: Message,
    pub target: Target,
    /// What the knob moved until now.
    pub holder: Target,
    /// The port the row of the holder names.
    pub holder_port: Option<String>,
}

#[derive(Default)]
pub struct Assign {
    /// The part waiting for a knob.
    pub learning: Option<Target>,
    pub clash: Option<Clash>,
    /// What the recording left out of its input since the last poll: (port, message).
    pub pending: Vec<(String, [u8; 3])>,
    /// The commands the pads pressed: they run where the screen's geometry is known.
    pub commands: Vec<String>,
    /// The menu of the clash is to be opened (it needs the screen's geometry).
    pub ask: bool,
}

/// Whether the message is the one a part's value is learnt from: a control change.
fn is_cc(msg: [u8; 3]) -> bool {
    matches!(Message::of(msg), Some(Message::Cc(_)))
}

impl Studio {
    /// The part and the value a target stands for, as the screen says it.
    pub fn midi_target_name(&self, target: &Target) -> String {
        let name = |id: &str| {
            id.parse::<TrackId>()
                .ok()
                .and_then(|t| self.project().tracks.get(&t).map(|t| t.name.clone()))
                .unwrap_or_else(|| id.to_string())
        };
        match target {
            Target::Volume(id) => format!("{} · ボリューム", name(id)),
            Target::Pan(id) => format!("{} · パン", name(id)),
            Target::Command(c) => c.clone(),
        }
    }

    /// The assignments now (from the settings the song keeps).
    pub fn midimap(&self) -> Midimap {
        self.song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .midimap
            .clone()
    }

    /// The target the part waiting for a knob stands for, with its hit on the mixer.
    pub fn learning_hit(&self) -> Option<MixerHit> {
        match self.live_midi.assign.learning.as_ref()? {
            Target::Volume(id) => id.parse().ok().map(MixerHit::Fader),
            Target::Pan(id) => id.parse().ok().map(MixerHit::Pan),
            Target::Command(_) => None,
        }
    }

    /// `midimap.learn`: a part waits for a knob; `cancel` gives it up; `confirm` takes the knob
    /// over from the part that had it.
    pub fn midi_learn(&mut self, args: &Map<String, Value>) -> bool {
        let flag = |k: &str| args.get(k).and_then(Value::as_bool).unwrap_or(false);
        if flag("cancel") {
            let a = &mut self.live_midi.assign;
            let had = a.learning.take().is_some() | a.clash.take().is_some();
            return had;
        }
        if flag("confirm") {
            let Some(c) = self.live_midi.assign.clash.take() else {
                return false;
            };
            // The part that had the knob lets it go, whichever port its row names.
            let mut free = json!({"target": c.holder.to_string()});
            if c.holder.track().is_none() {
                free = json!({"message": c.message.to_string()});
                if let Some(p) = &c.holder_port {
                    free["port"] = json!(p);
                }
            }
            if let Err(e) = self.song.run("midimap.unset", free) {
                self.error = Some(e);
                return false;
            }
            return self.midi_set(c.port.as_deref(), c.message, &c.target);
        }
        let Some(target) = args
            .get("target")
            .and_then(Value::as_str)
            .and_then(|t| Target::parse(t).ok())
            .filter(|t| {
                t.track()
                    .and_then(|id| id.parse::<TrackId>().ok())
                    .is_some_and(|id| self.project().tracks.contains_key(&id))
            })
        else {
            self.error = Some("割り当てるパートが指定されていません（track.volume:<id>）".into());
            return false;
        };
        if self.live_midi.input.is_none() {
            self.error = Some("MIDI の入力が開かれていません".into());
            return false;
        }
        let a = &mut self.live_midi.assign;
        a.clash = None;
        a.learning = Some(target);
        true
    }

    /// Writes one assignment through the settings command.
    fn midi_set(&mut self, port: Option<&str>, message: Message, target: &Target) -> bool {
        let mut args = json!({"message": message.to_string(), "target": target.to_string()});
        if let Some(p) = port {
            args["port"] = json!(p);
        }
        if self.transport.is_windowed() {
            if self.input_blocked() {
                return false;
            }
            self.io_command_on_worker("midimap.set", args.as_object().unwrap().clone());
            return true;
        }
        match self.song.run("midimap.set", args) {
            Ok(_) => {
                self.tell(format!(
                    "{} を {} に割り当てました",
                    self.midi_target_name(target),
                    message.label()
                ));
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// Takes a message of the live input or the recording's left-outs: true when it belongs to a
    /// knob or a pad (the notes do not get it).
    pub(crate) fn midi_take(&mut self, map: &Midimap, port: Option<&str>, msg: [u8; 3]) -> bool {
        if let Some(target) = self.live_midi.assign.learning.clone()
            && is_cc(msg)
            && let Some(message) = Message::of(msg)
        {
            let holder = map
                .find(port.unwrap_or(""), msg)
                .filter(|r| r.target != target)
                .map(|r| (r.target.clone(), r.port.clone()));
            self.live_midi.assign.learning = None;
            if let Some((holder, holder_port)) = holder {
                self.live_midi.assign.ask = true;
                self.live_midi.assign.clash = Some(Clash {
                    port: port.map(String::from),
                    message,
                    target,
                    holder,
                    holder_port,
                });
            } else {
                self.midi_set(port, message, &target);
            }
            return true;
        }
        let Some(row) = map.find(port.unwrap_or(""), msg).cloned() else {
            return false;
        };
        self.midi_move(&row, msg);
        true
    }

    /// A knob moves its part; a pad runs its command.
    fn midi_move(&mut self, row: &Row, msg: [u8; 3]) {
        match &row.target {
            Target::Volume(id) => {
                if let Ok(t) = id.parse() {
                    self.knob_move(MixerHit::Fader(t), msg[2]);
                }
            }
            Target::Pan(id) => {
                if let Ok(t) = id.parse() {
                    self.knob_move(MixerHit::Pan(t), msg[2]);
                }
            }
            Target::Command(c) => {
                let pressed = match msg[0] & 0xf0 {
                    0x90 => msg[2] > 0,
                    0xb0 => msg[2] >= 64,
                    _ => false,
                };
                if pressed {
                    self.live_midi.assign.commands.push(c.clone());
                }
            }
        }
    }

    /// Runs what the pads pressed. True when one ran.
    pub fn run_midi_commands(&mut self, g: &crate::studio::Geometry) -> bool {
        let commands = std::mem::take(&mut self.live_midi.assign.commands);
        let mut any = !commands.is_empty();
        if std::mem::take(&mut self.live_midi.assign.ask) {
            any |= self.ask_clash(g);
        }
        for c in commands {
            self.run_binding(g, &c);
        }
        any
    }

    /// Opens the menu of a knob that moves another part, at the part that asks for it.
    fn ask_clash(&mut self, g: &crate::studio::Geometry) -> bool {
        use crate::context::{Act, Item};
        let Some(c) = self.live_midi.assign.clash.clone() else {
            return false;
        };
        let hit = match &c.target {
            Target::Volume(id) => id.parse().ok().map(MixerHit::Fader),
            Target::Pan(id) => id.parse().ok().map(MixerHit::Pan),
            Target::Command(_) => None,
        };
        let (x, y) = hit
            .and_then(|h| self.mixer_rect(g, h))
            .map_or(self.input_map.pointer, |r| (r.x + r.w, r.y));
        let said = format!(
            "{} は {} に割り当て済みです",
            c.message.label(),
            self.midi_target_name(&c.holder)
        );
        let items = vec![
            Item::act(&said, "", Act::MidiLearnCancel).enabled(false),
            Item::gap(),
            Item::act("置き換える", "", Act::MidiLearnConfirm),
            Item::act("キャンセル", "", Act::MidiLearnCancel),
        ];
        self.open_context(items, x, y);
        true
    }

    /// The left-outs of the recording come to their parts.
    pub(crate) fn midi_pending(&mut self, map: &Midimap) -> bool {
        let pending = std::mem::take(&mut self.live_midi.assign.pending);
        let mut any = false;
        for (port, msg) in pending {
            any |= self.midi_take(map, Some(&port), msg);
        }
        any
    }
}

impl Studio {
    /// The line "MIDI 学習 · part" with its キャンセル button, while a part waits for a knob:
    /// the whole line and the button. Above the notice at the foot of the arrangement.
    pub fn midi_pill(&self, g: &Geometry) -> Option<(Rect, Rect)> {
        let target = self.live_midi.assign.learning.as_ref()?;
        let text = format!("MIDI 学習 · {}", self.midi_target_name(target));
        let w =
            (crate::browser::text_width(&text, FontSize::Small) + CANCEL_W + 40.0).min(g.lanes.w);
        let r = Rect::new(
            g.lanes.x + (g.lanes.w - w) / 2.0,
            g.drawer.y - 36.0 - PILL_H - 8.0,
            w,
            PILL_H,
        );
        let cancel = Rect::new(
            r.right() - CANCEL_W - 4.0,
            r.y + 3.0,
            CANCEL_W,
            PILL_H - 6.0,
        );
        Some((r, cancel))
    }
}

/// The line of a part that waits for a knob, drawn over the arrangement.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some((r, cancel)) = studio.midi_pill(g) else {
        return;
    };
    let Some(target) = studio.live_midi.assign.learning.as_ref() else {
        return;
    };
    let c = pen.c;
    pen.round(r, c.surface_3, Radius::Pill);
    let text = format!("MIDI 学習 · {}", studio.midi_target_name(target));
    let label = Rect::new(r.x + 14.0, r.y, r.w - CANCEL_W - 22.0, r.h);
    pen.mask(label, |pen| {
        pen.text(
            &text,
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            label,
            Align::Left,
        );
    });
    pen.round(cancel, c.surface_2, Radius::Pill);
    pen.text(
        "キャンセル",
        FontSize::Small,
        Weight::Medium,
        c.ink_strong,
        cancel,
        Align::Center,
    );
}

/// What a strip says of its knobs: a blue frame on the fader or the pan knob waiting for one,
/// a small mark on the ones that have one.
pub fn paint_strip(
    pen: &mut Pen,
    studio: &Studio,
    t: xirius_core::id::TrackId,
    pan: Option<Rect>,
    fader: Rect,
) {
    let waiting = studio.live_midi.assign.learning.as_ref();
    let map = &studio.transport.midimap;
    for (target, r) in [
        (Target::Volume(t.to_string()), Some(fader)),
        (Target::Pan(t.to_string()), pan),
    ] {
        let Some(r) = r else {
            continue;
        };
        if waiting == Some(&target) {
            let c = pen.c.ring;
            let e = 2.0;
            for edge in [
                Rect::new(r.x - e, r.y - e, r.w + e * 2.0, e),
                Rect::new(r.x - e, r.bottom(), r.w + e * 2.0, e),
                Rect::new(r.x - e, r.y, e, r.h),
                Rect::new(r.right(), r.y, e, r.h),
            ] {
                pen.fill(edge, c);
            }
        } else if map.of_target(&target).is_some() {
            pen.round(Rect::new(r.x, r.y, 5.0, 5.0), pen.c.ring, Radius::Pill);
        }
    }
}

#[cfg(test)]
mod tests;
