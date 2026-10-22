//! The built-ins' parameters through the commands (PLG-045): every one is listed with its
//! spec, starts at its default, and can be set as 0..1 or in its unit.

use serde_json::{Value, json};
use xirius_core::error::ErrorCode;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_core::{CmdError, Registry};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"].as_str().expect("an id").to_string()
}

fn row<'a>(rows: &'a Value, id: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {rows}"))
}

#[test]
fn every_builtin_is_listed_with_its_parameters() {
    let mut s = Session::new("list");
    let r = run(&mut s, "plugin.builtin.list", json!({}));
    let list = r.data.as_array().unwrap();
    let names: Vec<&str> = list.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "Sampler",
            "Drum Kit",
            "EQ",
            "Compressor",
            "Reverb",
            "Delay",
            "Limiter"
        ]
    );
    let comp = &list[3];
    assert_eq!(comp["kind"], "effect");
    let ratio = row(&comp["params"], "ratio");
    assert_eq!(ratio["unit"], ":1");
    assert_eq!(ratio["min"], 1.0);
    assert_eq!(ratio["max"], 20.0);
    assert_eq!(ratio["default"], 4.0);
    let delay = &list[5];
    let division = row(&delay["params"], "division");
    assert_eq!(division["choices"][3], "1/4");
    assert_eq!(division["steps"], 6);
}

#[test]
fn a_new_builtin_holds_every_default_and_lists_them() {
    let mut s = Session::new("defaults");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    let r = run(&mut s, "plugin.param.list", json!({"id": eq}));
    assert_eq!(r.data.as_array().unwrap().len(), 17);
    let freq = row(&r.data, "mid1_freq");
    assert_eq!(freq["unit"], "Hz");
    assert!((freq["plain"].as_f64().unwrap() - 400.0).abs() < 1e-6);
    assert_eq!(freq["text"], "400 Hz");
    let gain = row(&r.data, "mid1_gain");
    assert!((gain["value"].as_f64().unwrap() - 0.5).abs() < 1e-9);

    let pid: xirius_core::id::PluginId = eq.parse().unwrap();
    let x = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.inserts.iter())
        .find(|x| x.id == pid)
        .unwrap();
    assert_eq!(
        x.params.len(),
        17,
        "every parameter can be automated at once"
    );

    // A plugin that is not a built-in starts empty and lists what the song holds.
    let other = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Thing", "format": "vst3", "uid": "00"}),
    ));
    let r = run(&mut s, "plugin.param.list", json!({"id": other}));
    assert_eq!(r.data, json!([]));
}

#[test]
fn a_parameter_is_set_in_its_unit_or_as_a_fraction() {
    let mut s = Session::new("set");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let comp = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Compressor"}),
    ));
    let r = run(
        &mut s,
        "plugin.param.set",
        json!({"id": comp, "param": "Threshold", "plain": "-30"}),
    );
    assert!(r.message.contains("-30.0 dB"), "{}", r.message);
    let list = run(&mut s, "plugin.param.list", json!({"id": comp}));
    let th = row(&list.data, "threshold");
    assert!((th["plain"].as_f64().unwrap() + 30.0).abs() < 1e-6);
    assert!((th["value"].as_f64().unwrap() - 0.5).abs() < 1e-9);

    run(
        &mut s,
        "plugin.param.set",
        json!({"id": comp, "param": "ratio", "value": 1.0}),
    );
    let list = run(&mut s, "plugin.param.list", json!({"id": comp}));
    assert!((row(&list.data, "ratio")["plain"].as_f64().unwrap() - 20.0).abs() < 1e-6);

    // Out of range, both or neither, and a unit for a parameter the built-in lacks.
    let range = |r: Result<RunResult, CmdError>| r.expect_err("should fail").code;
    assert_eq!(
        range(try_run(
            &mut s,
            "plugin.param.set",
            json!({"id": comp, "param": "threshold", "plain": "6"}),
        )),
        ErrorCode::OutOfRange
    );
    assert!(
        try_run(
            &mut s,
            "plugin.param.set",
            json!({"id": comp, "param": "threshold", "plain": "-6", "value": 0.5}),
        )
        .is_err()
    );
    assert!(
        try_run(
            &mut s,
            "plugin.param.set",
            json!({"id": comp, "param": "threshold"})
        )
        .is_err()
    );
    assert!(
        try_run(
            &mut s,
            "plugin.param.set",
            json!({"id": comp, "param": "nope", "plain": "1"}),
        )
        .is_err()
    );

    // A choice by its name, a switch by on/off.
    let delay = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Delay"}),
    ));
    run(
        &mut s,
        "plugin.param.set",
        json!({"id": delay, "param": "division", "plain": "1/4"}),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({"id": delay, "param": "sync", "plain": "off"}),
    );
    let list = run(&mut s, "plugin.param.list", json!({"id": delay}));
    assert_eq!(row(&list.data, "division")["text"], "1/4");
    assert_eq!(row(&list.data, "sync")["text"], "オフ");
}

#[test]
fn the_new_commands_are_reads_with_labels() {
    let reg = Registry::standard();
    for name in ["plugin.param.list", "plugin.builtin.list"] {
        let d = reg.get(name).unwrap_or_else(|| panic!("{name}"));
        assert!(!d.history, "{name} only looks");
    }
}

// Insert counts and instrument ordering are covered in many_inserts.rs and insert_ops.rs.

/// B-9: `--plain 0.25` on a 0〜100 % parameter sets 0.25 % (the unit's value) but says that
/// 0〜1 is a fraction for `--value`, so a caller does not silently get a hundredth.
#[test]
fn a_fraction_given_as_plain_to_a_percent_parameter_says_which_flag_is_the_fraction() {
    let mut s = Session::new("plain-fraction");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let delay = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Delay"}),
    ));
    let r = run(
        &mut s,
        "plugin.param.set",
        json!({"id": delay, "param": "mix", "plain": "0.25"}),
    );
    assert!(r.message.contains("0.25 %"), "{}", r.message);
    assert!(r.message.contains("--value"), "{}", r.message);
    assert_eq!(r.data["warnings"][0], "0〜1 の割合なら --value を使います");
    // The value is still read in the unit: 0.25 %.
    let list = run(&mut s, "plugin.param.list", json!({"id": delay}));
    let mix = row(&list.data, "mix");
    assert!(
        (mix["plain"].as_f64().unwrap() - 0.25).abs() < 1e-6,
        "{mix}"
    );
}
