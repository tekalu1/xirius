use crate::mixer::{Output, Route};

/// Prepared processor delays before each automation stage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chain {
    pub input: u32,
    pub inserts: Vec<u32>,
}
impl Chain {
    pub fn fader(&self) -> u32 {
        self.input + self.inserts.iter().sum::<u32>()
    }
    pub fn insert(&self, index: usize) -> u32 {
        self.input + self.inserts.iter().take(index).sum::<u32>()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    pub tracks: Vec<Chain>,
    pub buses: Vec<Chain>,
}

/// The bus input delays used both by PDC and automation; buses feed later buses.
pub fn arrivals(tracks: &[Route], buses: &[Route]) -> (Vec<u32>, u32) {
    let mut arrive = vec![0u32; buses.len()];
    let mut master = 0;
    let reach = |to: Output, late: u32, arrive: &mut [u32], master: &mut u32| match to {
        Output::Master => *master = (*master).max(late),
        Output::Bus(b) => arrive[b] = arrive[b].max(late),
    };
    for t in tracks {
        reach(t.output, t.latency, &mut arrive, &mut master);
        for &b in &t.sends {
            reach(Output::Bus(b), t.latency, &mut arrive, &mut master);
        }
    }
    for (i, b) in buses.iter().enumerate() {
        let late = arrive[i] + b.latency;
        reach(b.output, late, &mut arrive, &mut master);
        for &to in &b.sends {
            reach(Output::Bus(to), late, &mut arrive, &mut master);
        }
    }
    (arrive, master)
}
