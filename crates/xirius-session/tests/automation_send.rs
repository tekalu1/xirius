//! A send lane reaches both the offline mixer and the running audio callback.
use serde_json::{Value, json};
use xirius_core::{Session, id::TrackId, model::Project, session::RunOptions};
use xirius_session::{
    build::Builder,
    commands, export,
    graph::Select,
    host::{HostConfig, SessionHost},
};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run(name, args.as_object().unwrap(), &RunOptions::human())
        .unwrap()
        .data
}
fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}
fn render(s: &Session, select: Select) -> export::Rendered {
    export::render(
        &mut Builder::new(48000, None),
        s.project(),
        select,
        0,
        192000,
        0,
    )
    .unwrap()
}

#[test]
fn send_points_attenuate_only_the_send_and_follow_in_the_running_engine() {
    let mut s = Session::with_registry(Project::new("send lane"), commands::shared_registry());
    let track = run(&mut s, "track.add.instrument", json!({"name":"source"}))["id"]
        .as_str()
        .unwrap()
        .parse::<TrackId>()
        .unwrap();
    let clip = run(
        &mut s,
        "clip.add.midi",
        json!({"track":track,"at":0,"bars":2}),
    )["id"]
        .clone();
    run(
        &mut s,
        "note.add",
        json!({"clip":clip,"at":0,"length":7680,"pitch":60,"velocity":100}),
    );
    let bus = run(&mut s, "track.add.bus", json!({"name":"send"}))["id"]
        .as_str()
        .unwrap()
        .parse::<TrackId>()
        .unwrap();
    run(
        &mut s,
        "send.add",
        json!({"ids":[track],"to":bus,"level":0}),
    );
    let target = format!("send:{}", s.project().tracks[&track].sends[0].id);
    let reference = render(&s, Select::Bus(bus));
    for (at, value) in [(0, -40.0), (3839, -40.0), (3840, 0.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track":track,"target":target,"at":at,"value":value}),
        );
    }
    let send = render(&s, Select::Bus(bus));
    let ratio = |from, to| rms(&send.left[from..to]) / rms(&reference.left[from..to]);
    assert!((ratio(2400, 96000) - 0.01).abs() < 0.0001);
    assert!((ratio(98400, 192000) - 1.0).abs() < 0.0001);
    let mix = render(&s, Select::All);
    let (mut host, mut audio, _) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(std::sync::Arc::default()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(host.seek_frame(0) && host.play());
    let mut live = Vec::new();
    let mut buffer = [0.0; 512];
    while live.len() < 192000 {
        audio.render(&mut buffer, 2);
        live.extend(buffer.as_chunks::<2>().0.iter().map(|v| v[0]));
        host.poll();
    }
    for (from, to) in [(2400, 96000), (98400, 192000)] {
        let ratio = rms(&live[from..to]) / rms(&mix.left[from..to]);
        assert!((ratio - 1.0).abs() < 0.001, "{ratio}");
    }
    run(&mut s, "auto.mode", json!({"ids":[track],"mode":"off"}));
    assert!((rms(&render(&s, Select::Bus(bus)).left) / rms(&reference.left) - 1.0).abs() < 0.0001);
}
