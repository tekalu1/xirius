//! What each place under the pointer does (UI-383): every kind of hit answers with a [`Want`],
//! and [`crate::pointer::show`] turns that into the shape. A match here may not end in `_`, so
//! a hit added without a shape does not build: the arrow is written as `Want::Plain`, on
//! purpose.

#![deny(clippy::wildcard_enum_match_arm)]

use crate::browser::{BrowserHit, RailTab};
use crate::mixer::MixerHit;
use crate::pointer::Shape;
use crate::roll::Cursor;
use crate::studio::Hit;
use crate::topbar::BarButton;

/// What a press or a drag does at a place; the pointer's shape follows from it alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Want {
    /// A press does nothing, or only selects: the arrow.
    Plain,
    /// A button or an item: the pointing hand.
    Press,
    /// A field to type in: the I-beam.
    Text,
    /// Dragged sideways (a level bar, an edge): ↔.
    DragH,
    /// Dragged up and down (a knob, a fader, a send): ↕.
    DragV,
    /// Picked up and moved (a loop, a section, a point): the open hand, closed while held.
    Grab,
    /// The corner that sizes a window.
    Corner,
    /// A place on the time line: the crosshair.
    Locate,
    /// Nothing can be done here.
    Blocked,
    /// A tool's picture (UI-184).
    Tool(Shape),
}

impl Want {
    /// What the arrangement's or the roll's shape asks for.
    pub fn of(cursor: Cursor) -> Want {
        match cursor {
            Cursor::Arrow => Want::Plain,
            Cursor::Press => Want::Press,
            Cursor::Move => Want::Grab,
            Cursor::ResizeH => Want::DragH,
            Cursor::ResizeV => Want::DragV,
            Cursor::Cross => Want::Locate,
            Cursor::Blocked => Want::Blocked,
            Cursor::Pencil => Want::Tool(Shape::Pencil),
            Cursor::Split => Want::Tool(Shape::Scissors),
            Cursor::Erase => Want::Tool(Shape::Eraser),
            Cursor::Mute => Want::Tool(Shape::Mute),
            Cursor::Audition => Want::Tool(Shape::Audition),
            Cursor::Range => Want::Tool(Shape::Range),
        }
    }
}

impl Hit {
    /// What a press here does; None leaves it to the arrangement or the roll under it, which
    /// answer by the tool held and the part of the clip or note.
    pub fn want(self) -> Option<Want> {
        Some(match self {
            Hit::None | Hit::Row(_) | Hit::Clip(..) | Hit::Ruler(_) => return None,
            Hit::PopupBody => Want::Plain,
            Hit::HeadFader(_) => Want::DragH,
            Hit::MidiCancel
            | Hit::Chevron(_)
            | Hit::Mute(_)
            | Hit::Solo(_)
            | Hit::Ara(..)
            | Hit::Tab(_)
            | Hit::RollView(_)
            | Hit::RollMap
            | Hit::RollLength(_)
            | Hit::Arm(_)
            | Hit::Monitor(_)
            | Hit::Keys(_)
            | Hit::TrackColor(_)
            | Hit::FolderColor(_)
            | Hit::TrackIcon(_)
            | Hit::TrackMark(_)
            | Hit::Widen(_)
            | Hit::Freeze(_)
            | Hit::Output(_)
            | Hit::FolderBus(_)
            | Hit::ToggleArrange
            | Hit::ToggleTempo
            | Hit::AddTrack
            | Hit::ZoomOut
            | Hit::ZoomIn
            | Hit::WaveZoom
            | Hit::Follow
            | Hit::Fold
            | Hit::Folded(_)
            | Hit::RailSettings
            | Hit::Item(_)
            | Hit::SearchClear
            | Hit::AllOff(..) => Want::Press,
            Hit::Rail(tab) => tab.want(),
            Hit::Browser(hit) => hit.want(),
            Hit::Inspector(part) => part.want(),
            Hit::Bar(button) => button.want(),
            Hit::Mixer(hit) => hit.want(),
            Hit::Start(entry) => entry.want(),
            Hit::Entrance(part) => part.want(),
            Hit::Home(spot) => spot.want(),
            Hit::Rec(part) => part.want(),
            Hit::Toast(part) => part.want(),
        })
    }
}

impl RailTab {
    pub fn want(self) -> Want {
        match self {
            RailTab::Instruments
            | RailTab::Effects
            | RailTab::Files
            | RailTab::SongFiles
            | RailTab::Inspector => Want::Press,
        }
    }
}

impl BrowserHit {
    pub fn want(self) -> Want {
        match self {
            BrowserHit::Search => Want::Text,
            BrowserHit::SearchClear
            | BrowserHit::ViewButton
            | BrowserHit::Group(_)
            | BrowserHit::Item(_)
            | BrowserHit::Fav(_)
            | BrowserHit::Disclose(_)
            | BrowserHit::Preset(_)
            | BrowserHit::Dir(_)
            | BrowserHit::File(_)
            | BrowserHit::Audition
            | BrowserHit::Loc(_)
            | BrowserHit::LocRemove(_)
            | BrowserHit::LocAdd
            | BrowserHit::Crumb(_)
            | BrowserHit::StopScan
            | BrowserHit::Retry(_)
            | BrowserHit::Pool(_)
            | BrowserHit::PoolCount(_)
            | BrowserHit::PoolMenu => Want::Press,
        }
    }
}

impl crate::inspector::Part {
    pub fn want(self) -> Want {
        match self {
            // A press picks the row and opens its value.
            crate::inspector::Part::Field(_) => Want::Press,
        }
    }
}

impl BarButton {
    pub fn want(self) -> Want {
        match self {
            // The tempo is dragged up and down (and scrolled, stepped or typed, UI-119).
            BarButton::Tempo => Want::DragV,
            BarButton::Logo
            | BarButton::SongName
            | BarButton::Saved
            | BarButton::Grid
            | BarButton::Snap
            | BarButton::History
            | BarButton::Palette
            | BarButton::Ai
            | BarButton::Export
            | BarButton::Tool(_)
            | BarButton::AltTool
            | BarButton::Shape
            | BarButton::DrawLength
            | BarButton::ToStart
            | BarButton::Play
            | BarButton::Stop
            | BarButton::Record
            | BarButton::Loop
            | BarButton::PreRoll
            | BarButton::Metronome
            | BarButton::Meter
            | BarButton::RecPanel
            | BarButton::Cpu
            | BarButton::Ripple => Want::Press,
        }
    }
}

impl MixerHit {
    pub fn want(self) -> Want {
        match self {
            // Only selects the strip; the heading is dragged to another channel as it is.
            MixerHit::Strip(_) | MixerHit::Heading(..) => Want::Plain,
            MixerHit::Fader(_) | MixerHit::Pan(_) | MixerHit::SendLevel(..) => Want::DragV,
            MixerHit::Control(_, control) if control.is_button() => Want::Press,
            MixerHit::Control(..) => Want::DragV,
            MixerHit::Level(..) | MixerHit::KeyLevel(..) | MixerHit::BranchPan(..) => Want::DragH,
            MixerHit::Mute(_)
            | MixerHit::Solo(_)
            | MixerHit::Arm(_)
            | MixerHit::Bypass(..)
            | MixerHit::Open(..)
            | MixerHit::AddInsert(_)
            | MixerHit::AddInstrument(_)
            | MixerHit::AddLayer(_)
            | MixerHit::Fold(..)
            | MixerHit::KeySend(..)
            | MixerHit::SendPre(..)
            | MixerHit::AddSend(_)
            | MixerHit::Output(_)
            | MixerHit::Input(_)
            | MixerHit::RowMute(..)
            | MixerHit::AddBranch(..)
            | MixerHit::Branch(..)
            | MixerHit::Seam(..)
            | MixerHit::Widen(_)
            | MixerHit::Color(_)
            | MixerHit::Editor(_) => Want::Press,
        }
    }
}

impl crate::start::Entry {
    pub fn want(self) -> Want {
        use crate::start::Entry;
        match self {
            Entry::Audio | Entry::Midi => Want::Press,
            // Only a label: files are dropped anywhere.
            Entry::Drop => Want::Plain,
        }
    }
}

impl crate::start::Part {
    pub fn want(self) -> Want {
        match self {
            crate::start::Part::NoDevice => Want::Press,
        }
    }
}

impl crate::home::Spot {
    pub fn want(self) -> Want {
        use crate::home::Spot;
        match self {
            Spot::Search => Want::Text,
            Spot::Button(_) | Spot::Device | Spot::SearchClear | Spot::Row(..) => Want::Press,
        }
    }
}

impl crate::rec_panel::RecPart {
    pub fn want(self) -> Want {
        use crate::rec_panel::RecPart;
        match self {
            RecPart::PreRollBars | RecPart::PostRollBars | RecPart::CountBars => Want::Text,
            RecPart::Punch
            | RecPart::PreRoll
            | RecPart::PostRoll
            | RecPart::Count
            | RecPart::Quantize
            | RecPart::QuantizeValue => Want::Press,
        }
    }
}

impl crate::rec_area::RecPart {
    pub fn want(&self) -> Want {
        use crate::rec_area::RecPart;
        match self {
            RecPart::Gain(_) => Want::DragH,
            // A take's row plays it, and a drag along it comps the stretch.
            RecPart::Fold | RecPart::Bar { .. } => Want::Press,
        }
    }
}

impl crate::toast::ToastPart {
    pub fn want(self) -> Want {
        use crate::toast::ToastPart;
        match self {
            ToastPart::Body => Want::Plain,
            ToastPart::Button | ToastPart::Also | ToastPart::Close => Want::Press,
        }
    }
}

// ------------------------------------------------------------------ the windows of their own

impl crate::instrument::window::Part {
    /// A part of a device's window (UI-384). A knob's own look and its lock are
    /// [`Studio::float_want`]'s: a button among the knobs is pressed, a locked one refuses.
    pub fn want(self) -> Want {
        use crate::instrument::window::Part;
        match self {
            // The bar moves the window; a graph's plane away from its points does nothing.
            Part::Head | Part::Graph(_) | Part::Body => Want::Plain,
            Part::Power
            | Part::Preset
            | Part::PrevPreset
            | Part::NextPreset
            | Part::Sidechain
            | Part::Sources
            | Part::Channel
            | Part::Pin
            | Part::Close
            | Part::Tab(_)
            | Part::Spectrum
            | Part::Readout(_)
            | Part::Key(_)
            | Part::Pad(_)
            | Part::Spot(_) => Want::Press,
            Part::Resize => Want::Corner,
            Part::Knob(_) => Want::DragV,
            Part::Point(..) => Want::Grab,
        }
    }
}

impl crate::instrument::window::Grab {
    pub fn want(&self) -> Want {
        use crate::instrument::window::Grab;
        match self {
            Grab::Size { .. } => Want::Corner,
            Grab::Knob { .. } => Want::DragV,
            Grab::Point { .. } => Want::Grab,
            Grab::Note { .. } => Want::Press,
            Grab::Edge { .. } => Want::DragH,
        }
    }
}

impl crate::channel::Part {
    /// A part of the channel editor (UI-384).
    pub fn want(self) -> Want {
        use crate::channel::Part;
        match self {
            // A card and a layer's box only choose (a card is dragged to another place as it is).
            Part::Head | Part::Card(_) | Part::Range(_) | Part::Body => Want::Plain,
            Part::Pin
            | Part::Close
            | Part::Preset
            | Part::PrevPreset
            | Part::NextPreset
            | Part::Tool(_)
            | Part::Dot(_)
            | Part::More(_)
            | Part::Mode(_)
            | Part::Gap(_)
            | Part::Mute(_)
            | Part::Remove(_)
            | Part::AddBranch(_)
            | Part::MacroOut => Want::Press,
            Part::Level(_) | Part::Pan(_) | Part::Cross(..) | Part::Side(..) => Want::DragH,
            Part::Transpose(_) | Part::Macro(_) => Want::DragV,
        }
    }
}

impl crate::channel::Grab {
    pub fn want(&self) -> Want {
        use crate::channel::Grab;
        match self {
            Grab::Card { moved: true, .. } => Want::Grab,
            Grab::Card { moved: false, .. } => Want::Plain,
            Grab::Cross { .. } | Grab::Side { .. } => Want::DragH,
            Grab::Transpose { .. } => Want::DragV,
        }
    }
}

impl crate::macro_pad::Part {
    /// A part of the Macro Controls' window (UI-384).
    pub fn want(self) -> Want {
        use crate::macro_pad::Part;
        match self {
            Part::Head | Part::Body => Want::Plain,
            Part::Close | Part::Pin | Part::Pick(_) => Want::Press,
            Part::Knob(_) => Want::DragV,
            // The pad's point goes where the pointer is.
            Part::Pad => Want::Locate,
        }
    }
}

impl crate::macro_pad::Grab {
    pub fn want(&self) -> Want {
        match self {
            crate::macro_pad::Grab::Pad { .. } => Want::Locate,
        }
    }
}

/// A pop-up's cell under the pointer: a field types, an item is pressed.
fn popup_want(layout: &crate::popup::PopupLayout, x: f32, y: f32) -> Want {
    let Some(c) = layout.cells.iter().rev().find(|c| c.rect.contains(x, y)) else {
        return Want::Plain;
    };
    if c.clear_rect().is_some_and(|r| r.contains(x, y)) {
        Want::Press
    } else if !c.enabled {
        Want::Plain
    } else if matches!(
        c.look,
        crate::popup::Look::Input | crate::popup::Look::Field
    ) {
        Want::Text
    } else if c.action.is_some() {
        Want::Press
    } else {
        Want::Plain
    }
}

impl crate::studio::Studio {
    /// What the pointer shows over a window of its own (a device's, the channel editor, the
    /// Macro Controls, UI-384), and whether something is held. Called inside the window's
    /// scope, with its own geometry.
    pub fn float_want(&self, g: &crate::studio::Geometry, x: f32, y: f32) -> (Want, bool) {
        if let Some(p) = &self.popup {
            return (popup_want(&p.layout(g.size), x, y), false);
        }
        if self.mixer.level_drag.is_some() || self.mixer.key_drag.is_some() {
            return (Want::DragH, true);
        }
        if self.mixer.drag.is_some() {
            return (Want::DragV, true);
        }
        if self.instruments.detached_macros {
            if let Some(grab) = self.macro_pad.as_ref().and_then(|w| w.grab.as_ref()) {
                return (grab.want(), true);
            }
            let part = self
                .macro_view()
                .and_then(|v| crate::macro_pad::part_at(&v, x, y));
            return (part.map_or(Want::Plain, |p| p.want()), false);
        }
        if self.instruments.detached_channel {
            if let Some(w) = &self.channel {
                if let Some(grab) = &w.grab {
                    return (grab.want(), true);
                }
                if w.keyboard_drag.is_some() {
                    return (Want::DragH, true);
                }
            }
            let part = self
                .channel_view(g)
                .and_then(|v| crate::channel::part_at(&v, x, y));
            return (part.map_or(Want::Plain, |p| p.want()), false);
        }
        if let Some(grab) = &self.instruments.grab {
            return (grab.want(), true);
        }
        if self.instruments.keyboard_drag.is_some() {
            return (Want::DragH, true);
        }
        // What a drop on this window said has its button over the device (UI-353).
        if self.over_notice_button(g, x, y) {
            return (Want::Press, false);
        }
        let p = self.auto_shown();
        match self.instruments.at(&p, g, x, y) {
            Some((i, crate::instrument::window::Part::Knob(k))) => {
                (self.knob_want(&p, g, i, k), false)
            }
            Some((_, part)) => (part.want(), false),
            None => (Want::Plain, false),
        }
    }

    /// A device's control: a knob or a field is dragged up and down, the buttons and the
    /// choices among them are pressed, a locked one refuses (UI-384).
    fn knob_want(
        &self,
        p: &xirius_core::model::Project,
        g: &crate::studio::Geometry,
        i: usize,
        k: usize,
    ) -> Want {
        use crate::device::control::Look;
        let Some(w) = self.instruments.open.get(i).copied() else {
            return Want::Plain;
        };
        let Some((_, plugin, def)) = crate::instrument::window::live(p, &w) else {
            return Want::Plain;
        };
        if def
            .params
            .get(k)
            .is_some_and(|spec| self.param_locked(w.track, w.plugin, spec.id))
        {
            return Want::Blocked;
        }
        let look = self
            .instruments
            .slots(self.instruments.frame(i, g), plugin, def)
            .into_iter()
            .find(|s| s.k == k)
            .map(|s| s.look);
        match look {
            Some(Look::Toggle | Look::Dot | Look::Menu | Look::Segments) => Want::Press,
            Some(Look::Knob | Look::Field) | None => Want::DragV,
        }
    }
}

#[cfg(test)]
#[path = "want_tests.rs"]
mod tests;
