use xirius_engine::{
    AudioIo, BusSpec, MixSpec, NoteEvent, Output, Plan, ProcessSetup, Processor, RenderMode,
    TrackSlot, TrackSpec, render_offline,
};

struct Constant;
impl Processor for Constant {
    fn name(&self) -> &str {
        "constant"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.left.fill(1.0);
        io.right.fill(1.0);
    }
}
struct Add;
impl Processor for Add {
    fn name(&self) -> &str {
        "add"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for v in io.left.iter_mut().chain(io.right.iter_mut()) {
            *v += 0.5;
        }
    }
}
struct Square;
impl Processor for Square {
    fn name(&self) -> &str {
        "square"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for v in io.left.iter_mut().chain(io.right.iter_mut()) {
            *v *= *v;
        }
    }
}
fn inserts() -> Vec<Box<dyn Processor>> {
    vec![Box::new(Add), Box::new(Square)]
}

#[test]
fn track_bus_and_master_run_inserts_in_order_before_the_fader() {
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 64,
        mode: RenderMode::Offline,
    };
    for channel in ["track", "bus", "master"] {
        let mut track = TrackSpec::new("input", Box::new(Constant));
        let mut spec = MixSpec::default();
        match channel {
            "track" => {
                track.inserts = inserts();
                track.gain = 0.5;
            }
            "bus" => {
                track.output = Output::Bus(0);
                let mut bus = BusSpec::new("bus");
                bus.inserts = inserts();
                bus.gain = 0.5;
                spec.buses.push(bus);
            }
            "master" => {
                spec.master_inserts = inserts();
                spec.master_gain = 0.5;
            }
            _ => unreachable!(),
        }
        spec.tracks.push(TrackSlot::New(track));
        let (left, right) = render_offline(Plan::build(spec, setup, None).unwrap(), 192);
        // Addition and squaring do not commute with each other or the fader.
        assert!(
            left.iter().chain(&right).all(|v| (*v - 1.125).abs() < 1e-6),
            "{channel}: {left:?}"
        );
    }
}
