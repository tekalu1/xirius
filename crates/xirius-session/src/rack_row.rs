//! How a track's rack becomes its engine insert row (ENG-074): the plug-ins outside the fx tree
//! (a multi instrument's layer fx) first in a line, then the fx tree, where each splitter is one
//! insert holding its branches. The same layout serves the build (which fills it with
//! processors) and the automation timing (which needs each plug-in's place in the row).
//!
//! A branch's gain, pan and mute are values, not structure: [`structure`] blanks them so a change
//! goes to the running plan as `Command::BranchGain`, and [`values`] reads them for the mix.

use std::collections::HashSet;

use xirius_core::rack::{Rack, RackNode, SplitMode};

use crate::graph::db_to_gain;

/// One insert of the row, or of a branch's chain.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    /// A plug-in, by id.
    Plugin(u64),
    /// A splitter with two or more branches: (branch node id, its chain) each.
    Split {
        node: u64,
        mode: SplitMode,
        branches: Vec<(u64, Vec<Slot>)>,
    },
}

/// The row of a rack over the plug-ins that were built, in their built order. A plug-in that
/// was not built leaves no slot; a splitter whose branches are all empty still stands (its
/// branches are copies of the signal). A splitter with one branch is its branch in a line, and
/// one with none is left out.
pub fn layout(rack: &Rack, built: &[u64]) -> Vec<Slot> {
    let mut tree = HashSet::new();
    fx_ids(&rack.fx, &mut tree);
    let have: HashSet<u64> = built.iter().copied().collect();
    let mut row: Vec<Slot> = built
        .iter()
        .filter(|id| !tree.contains(id))
        .map(|&id| Slot::Plugin(id))
        .collect();
    chain(&rack.fx, &have, &mut row);
    row
}

fn chain(nodes: &[RackNode], have: &HashSet<u64>, out: &mut Vec<Slot>) {
    for n in nodes {
        match n {
            RackNode::Plugin(id) => {
                if have.contains(&id.0) {
                    out.push(Slot::Plugin(id.0));
                }
            }
            RackNode::Split(s) => match s.branches.len() {
                0 => {}
                1 => chain(&s.branches[0].fx, have, out),
                _ => out.push(Slot::Split {
                    node: s.id.0,
                    mode: s.mode,
                    branches: s
                        .branches
                        .iter()
                        .map(|b| {
                            let mut inner = Vec::new();
                            chain(&b.fx, have, &mut inner);
                            (b.id.0, inner)
                        })
                        .collect(),
                }),
            },
        }
    }
}

fn fx_ids(nodes: &[RackNode], out: &mut HashSet<u64>) {
    for n in nodes {
        match n {
            RackNode::Plugin(id) => {
                out.insert(id.0);
            }
            RackNode::Split(s) => {
                for b in &s.branches {
                    fx_ids(&b.fx, out);
                }
            }
        }
    }
}

/// The rack as structure: every branch's gain, pan and mute set to rest.
pub fn structure(rack: &Rack) -> Rack {
    let mut r = rack.clone();
    blank(&mut r.fx);
    r
}

fn blank(nodes: &mut [RackNode]) {
    for n in nodes {
        if let RackNode::Split(s) = n {
            for b in &mut s.branches {
                b.gain_db = 0.0;
                b.pan = 0.0;
                b.mute = false;
                blank(&mut b.fx);
            }
        }
    }
}

/// One branch's live values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BranchMix {
    /// The branch's node id.
    pub node: u64,
    /// Linear.
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
}

/// The values of every branch in the rack's fx tree, depth first.
pub fn values(rack: &Rack, out: &mut Vec<BranchMix>) {
    fn walk(nodes: &[RackNode], out: &mut Vec<BranchMix>) {
        for n in nodes {
            if let RackNode::Split(s) = n {
                for b in &s.branches {
                    out.push(BranchMix {
                        node: b.id.0,
                        gain: db_to_gain(b.gain_db),
                        pan: b.pan as f32,
                        mute: b.mute,
                    });
                    walk(&b.fx, out);
                }
            }
        }
    }
    walk(&rack.fx, out);
}
