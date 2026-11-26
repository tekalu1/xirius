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
