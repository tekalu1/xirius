//! Track snapshots keep the source's routing and clip envelopes after it is cut.
use super::*;
use crate::clipboard::Clipboard;
use crate::id::TrackId;
use crate::model::TrackKind;

pub(super) fn gather(p: &Project, a: &Args) -> CmdResult<Clipboard> {
    let ids = a.ids::<TrackId>("ids")?;
    if ids.is_empty() {
        return Err(CmdError::invalid("トラックが選択されていません"));
    }
    for id in &ids {
        if p.track(*id)?.kind == TrackKind::Master {
            return Err(CmdError::invalid("マスターはコピーできません"));
        }
    }
    Ok(Clipboard::Tracks {
        source: Arc::new(p.clone()),
        ids,
    })
}

pub(super) fn put(p: &mut Project, source: &Project, ids: &[TrackId]) -> CmdResult<Output> {
    let mut snapshot = source.clone();
    snapshot.next_id = p.next_id;
    let mut copies = Vec::new();
    let mut map = std::collections::HashMap::new();
    for id in ids {
        let t = source.track(*id)?;
        let mut c = super::track::clone_track(&mut snapshot, t, false);
        c.output = source.effective_output(*id);
        for (old_clip, new_clip) in t.clips.iter().zip(&c.clips) {
            for (old, new) in old_clip.notes().iter().zip(new_clip.notes()) {
                if let Some(timing) = source.original_timing.get(&old.id) {
                    p.original_timing.insert(new.id, *timing);
                }
            }
        }
        map.insert(*id, c.id);
        copies.push(c);
    }
    p.next_id = snapshot.next_id;
    for c in &mut copies {
        if let Some(crate::model::Output::Bus(b)) = c.output {
            c.output = map
                .get(&b)
                .copied()
                .map(crate::model::Output::Bus)
                .or_else(|| {
                    p.tracks
                        .get(&b)
                        .filter(|t| t.kind == TrackKind::Bus)
                        .map(|_| crate::model::Output::Bus(b))
                });
        }
        c.sends.retain_mut(|s| {
            if let Some(to) = map.get(&s.to) {
                s.to = *to;
                true
            } else {
                p.tracks
                    .get(&s.to)
                    .is_some_and(|t| t.kind == TrackKind::Bus)
            }
        });
        c.automation.retain(|l| !matches!(&l.target, crate::auto::AutoTarget::Send(id) if !c.sends.iter().any(|s| s.id == *id)));
        for cl in &c.clips {
            if let Some(fx) = snapshot.clip_fx.get(&cl.id) {
                p.clip_fx.insert(cl.id, fx.clone());
            }
        }
    }
    let made: Vec<String> = copies.iter().map(|t| t.id.to_string()).collect();
    for c in copies {
        p.root.push(crate::id::Node::Track(c.id));
        p.tracks.insert(c.id, Arc::new(c));
    }
    Ok(Output::with(
        "トラックを貼り付けました",
        json!({"ids": made}),
    ))
}
