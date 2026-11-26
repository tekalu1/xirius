use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::{SendId, TrackId};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn id(v: Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .to_string()
}

#[test]
fn pasted_track_retains_inherited_bus_and_original_note_timing() {
    let mut s = Session::new("Inherited routing");
    let bus = id(run(&mut s, "track.add.bus", json!({})));
    let folder = id(run(&mut s, "folder.add", json!({})));
    let t = id(run(&mut s, "track.add.instrument", json!({"into":folder})));
    run(&mut s, "folder.bus.set", json!({"id":folder,"bus":bus}));
    let c = id(run(
        &mut s,
        "clip.add.midi",
        json!({"track":t,"at":0,"bars":2}),
    ));
    run(
        &mut s,
        "note.add",
        json!({"clip":c,"at":110,"pitch":60,"length":240}),
    );
    run(&mut s, "note.quantize", json!({"ids":[c],"grid":"1/4"}));
    run(&mut s, "edit.copy", json!({"tracks":[t]}));
    let copied: TrackId = run(&mut s, "edit.paste", json!({}))["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        s.project().effective_output(copied).unwrap().to_string(),
        bus
    );
    let new = s.project().tracks[&copied].clips[0].id;
    assert_eq!(s.project().clip(new).unwrap().1.notes()[0].start, 0);
    run(
        &mut s,
        "note.quantize",
        json!({"ids":[new.to_string()],"restore":true}),
    );
    assert_eq!(s.project().clip(new).unwrap().1.notes()[0].start, 110);
    s.project().validate_deep().unwrap();
}

#[test]
fn removing_a_muted_send_or_its_bus_removes_its_lane_and_undo_restores_both() {
    for delete_bus in [false, true] {
        let mut s = Session::new("Send automation");
        let bus = id(run(&mut s, "track.add.bus", json!({})));
        let t = id(run(&mut s, "track.add.audio", json!({})));
        let send = id(run(&mut s, "send.add", json!({"ids":[t],"to":bus})));
        run(&mut s, "send.mute", json!({"id":send}));
        let target = format!("send:{send}");
        run(&mut s, "auto.lane.add", json!({"track":t,"target":target}));
        run(
            &mut s,
            "auto.point.add",
            json!({"track":t,"target":target,"at":0,"value":-6}),
        );
        let tid: TrackId = t.parse().unwrap();
        let sid: SendId = send.parse().unwrap();
        let lane = s.project().tracks[&tid]
            .automation
            .iter()
            .find(|l| l.target == AutoTarget::Send(sid))
            .unwrap();
        assert!(lane.suspended);
        assert_eq!(lane.value_at(0.0), None);
        let before = s.project().clone();
        if delete_bus {
            run(&mut s, "track.delete", json!({"ids":[bus]}));
        } else {
            run(&mut s, "send.remove", json!({"ids":[send]}));
        }
        assert!(s.project().tracks[&tid].sends.is_empty());
        assert!(s.project().tracks[&tid].automation.is_empty());
        run(&mut s, "history.undo", json!({}));
        assert_eq!(s.project(), &before);
    }
}
