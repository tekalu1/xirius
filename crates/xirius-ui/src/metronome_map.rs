use xirius_core::{model::Project, time::Meter};
use xirius_engine::click::Click;
use xirius_session::tempo::TempoMap;

pub(crate) fn meter_at(p: &Project, tick: i64) -> Meter {
    let bar = p.time_map().to_bbt(tick).0;
    meter_for_bar(p, bar)
}

fn meter_for_bar(p: &Project, bar: u32) -> Meter {
    p.timesig
        .iter()
        .rev()
        .find(|s| s.bar <= bar)
        .map_or(Meter { num: 4, den: 4 }, |s| Meter {
            num: s.num,
            den: s.den,
        })
}

pub(crate) fn click(p: &Project, tm: &TempoMap, rate: f64, end_frame: u64) -> Click {
    let bars = p.time_map();
    let last_bar = bars
        .to_bbt(tm.tick_of_frame(end_frame, rate))
        .0
        .saturating_add(8);
    let mut beats = Vec::new();
    for bar in 1..=last_bar {
        let tick = bars.bar_start(bar);
        let meter = meter_for_bar(p, bar);
        for place in 0..meter.num {
            let at = tick + i64::from(place) * meter.beat_ticks(p.ppq);
            beats.push((tm.frame_at(at, rate), place));
        }
    }
    let last = bars.bar_start(last_bar);
    let meter = meter_for_bar(p, last_bar);
    let beat = meter.beat_ticks(p.ppq);
    let end = last + meter.bar_ticks(p.ppq);
    let every = (tm.seconds_at(end) - tm.seconds_at(end - beat)) * rate;
    Click::new(rate, beats, every, meter.num, 0.3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn tempo_and_meter_changes_move_clicks_and_restart_the_downbeat() {
        let mut s = crate::studio::Studio::sample();
        assert!(s.run_command("tempo.set", json!({"at":0,"bpm":120})));
        assert!(s.run_command("tempo.set", json!({"at":19200,"bpm":180})));
        assert!(s.run_command("timesig.set", json!({"bar":6,"num":3,"den":8})));
        let p = s.project();
        let tm = TempoMap::from_project(&p);
        let bars = p.time_map();
        assert_eq!(
            crate::ruler::bars_in(&bars, 19200, 22080),
            vec![(6, 19200), (7, 20640), (8, 22080)]
        );
        let end = tm.frame_at(22080, 48000.0);
        let mut c = click(&p, &tm, 48000.0, end);
        let mut out = vec![0.0; end as usize + 2];
        c.render(&mut out, 0);
        for (tick, place) in [
            (18240, 3),
            (19200, 0),
            (19680, 1),
            (20160, 2),
            (20640, 0),
            (21120, 1),
        ] {
            let at = tm.frame_at(tick, 48000.0) as usize;
            assert_eq!(out[at], 0.0);
            assert!(out[at - 100..at].iter().all(|v| *v == 0.0));
            let hz = if place == 0 { 1760.0 } else { 1320.0 };
            let expected =
                0.3 * (std::f32::consts::TAU * hz / 48000.0).sin() * (-6.9f32 / 1680.0).exp();
            assert!(
                (out[at + 1] - expected).abs() < 1e-6,
                "{tick}: {}",
                out[at + 1]
            );
        }
        assert_eq!(
            tm.frame_at(19200, 48000.0) - tm.frame_at(18240, 48000.0),
            24000
        );
        assert_eq!(
            tm.frame_at(19680, 48000.0) - tm.frame_at(19200, 48000.0),
            8000
        );
        s.seek(18240);
        assert_eq!(s.bar_view().meter, "4/4");
        s.seek(19200);
        assert_eq!(s.bar_view().meter, "3/8");
        assert_eq!(s.bar_view().time, "6.1.000");
    }
}
