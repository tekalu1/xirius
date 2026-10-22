use xirius_engine::{
    BusSpec, EngineConfig, MixSpec, Output, Plan, ProcessSetup, RenderMode, TrackSlot, TrackSpec,
    builtin::{Delay, Sine},
    engine::create,
};
const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};
struct PreparedSource(u32);
impl xirius_engine::Processor for PreparedSource {
    fn name(&self) -> &str {
        "prepared source"
    }
    fn prepare(&mut self, _: &ProcessSetup) {
        self.0 = 31;
    }
    fn process(&mut self, io: &mut xirius_engine::AudioIo<'_>, _: &[xirius_engine::NoteEvent]) {
        io.clear();
    }
    fn latency(&self) -> u32 {
        self.0
    }
}
#[test]
fn prepared_pdc_offsets_include_bus_arrival_and_survive_a_kept_track() {
    let mut track = TrackSpec::new("source", Box::new(PreparedSource(0)));
    track.inserts = vec![Box::new(Delay::new(777)), Box::new(Delay::new(19))];
    track.output = Output::Bus(0);
    let direct = TrackSpec::new("direct", Box::new(Sine::new(220.0, 0.0)));
    let mut bus = BusSpec::new("bus");
    bus.inserts = vec![Box::new(Delay::new(47))];
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track), TrackSlot::New(direct)],
            buses: vec![bus],
            ..Default::default()
        },
        SETUP,
        None,
    )
    .unwrap();
    assert_eq!(plan.latency(), 874);
    let shape = plan.shape();
    assert_eq!(shape.automation.tracks[0].input, 31);
    assert_eq!(shape.automation.tracks[0].insert(0), 31);
    assert_eq!(shape.automation.tracks[0].fader(), 827);
    assert_eq!(shape.automation.tracks[0].insert(1), 808);
    assert_eq!(shape.automation.tracks[1].fader(), 0);
    assert_eq!(shape.automation.buses[0].input, 827);
    assert_eq!(shape.automation.buses[0].fader(), 874);
    let (mut audio, mut ctl) = create(plan, EngineConfig::default());
    let mut bus = BusSpec::new("bus");
    bus.inserts = vec![Box::new(Delay::new(47))];
    ctl.swap(MixSpec {
        tracks: vec![TrackSlot::Keep(0), TrackSlot::Keep(1)],
        buses: vec![bus],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(ctl.shape().automation, shape.automation);
    audio.render(&mut [0.0; 128], 2);
    assert_eq!(audio.latency(), 874);
}
