//! MIX-044: splitters nested in splitters on the strip. At 116 the nested one starts shut with
//! its count of branches, a press opens one level, the strip widened to 252 shows them all, and
//! at 116 again what the person opened stays open; through the offscreen window's pointer
//! handlers.
use crate::arrange::probe::track;
use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::rack::RackNode;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "n01-nested-shut-116",
        acts: &[stage(0)],
    },
    Shot {
        name: "n02-one-level-open-116",
        acts: &[stage(1)],
    },
    Shot {
        name: "n03-all-open-252",
        acts: &[stage(2)],
    },
    Shot {
        name: "n04-kept-116",
        acts: &[stage(3)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn press(p: (f32, f32), window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: at(p),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: at(p),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn center(studio: &Rc<RefCell<Studio>>, hit: MixerHit, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    s.mixer_center(&g, hit).expect("on screen")
}

fn add(s: &mut Studio, t: TrackId, effect: &str) -> PluginId {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": effect, "kind": "effect", "format": "builtin", "uid": effect }),
    ));
    s.project().tracks[&t].inserts.last().unwrap().id
}

/// The splitters of `t`'s rack, each with its branches, outermost first.
fn splits(s: &Studio, t: TrackId) -> Vec<(NodeId, Vec<NodeId>)> {
    fn walk(nodes: &[RackNode], out: &mut Vec<(NodeId, Vec<NodeId>)>) {
        for n in nodes {
            if let RackNode::Split(x) = n {
                out.push((x.id, x.branches.iter().map(|b| b.id).collect()));
                for b in &x.branches {
                    walk(&b.fx, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Some(rack) = &s.project().tracks[&t].rack {
        walk(&rack.fx, &mut out);
    }
    out
}

/// A splitter put at `at`; its id and branches.
fn split(s: &mut Studio, t: TrackId, at: &str) -> (NodeId, Vec<NodeId>) {
    let before = splits(s, t);
    assert!(s.run_command(
        "rack.split.add",
        json!({ "track": t.to_string(), "at": at })
    ));
    splits(s, t)
        .into_iter()
        .find(|x| !before.contains(x))
        .expect("the new splitter")
}

fn moved(s: &mut Studio, id: PluginId, to: NodeId) {
    assert!(s.run_command(
        "rack.node.move",
        json!({ "id": id.to_string(), "to": to.to_string() })
    ));
}

/// The splitter in the first branch of Vocal's splitter.
fn inner(s: &Studio, t: TrackId) -> NodeId {
    splits(s, t)[1].0
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let vocal = track(&studio.borrow(), "Vocal");
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            s.drawer_size.mixer = Some(520.0);
            // EQ, then a splitter: Compressor and a splitter in one branch (Delay and one more
            // splitter in its first), Delay in the other.
            add(&mut s, vocal, "EQ");
            let comp = add(&mut s, vocal, "Compressor");
            let delay = add(&mut s, vocal, "Delay");
            let (_, outer) = split(&mut s, vocal, "track");
            moved(&mut s, comp, outer[0]);
            moved(&mut s, delay, outer[1]);
            let (_, mid) = split(&mut s, vocal, &format!("branch:{}:1", outer[0]));
            let echo = add(&mut s, vocal, "Delay");
            moved(&mut s, echo, mid[0]);
            split(&mut s, vocal, &format!("branch:{}:1", mid[0]));
            s.hover = crate::studio::Hit::None;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => {
            let id = inner(&studio.borrow(), vocal);
            let p = center(studio, MixerHit::Fold(vocal, id), window);
            press(p, window, cx);
        }
        _ => {
            let p = center(studio, MixerHit::Widen(vocal), window);
            press(p, window, cx);
            studio.borrow_mut().hover = crate::studio::Hit::None;
        }
    }
}
