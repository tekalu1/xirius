//! A per-track ring of pure timeline audio (ENG-015). Stateful instruments and inserts stay
//! synchronous; their state cannot be rewound after a speculative block is discarded.
use crate::timeline::Timeline;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AheadStatus {
    pub cached_blocks: usize,
    pub rendered_frames: u64,
    pub generation: u64,
}
struct Block {
    at: u64,
    frames: usize,
    valid: bool,
    left: Vec<f32>,
    right: Vec<f32>,
}
pub(crate) struct Ahead {
    blocks: Vec<Block>,
    cursor: usize,
    rendered: u64,
    generation: u64,
}

impl Ahead {
    pub fn new(max_block: usize) -> Self {
        Self {
            blocks: (0..3)
                .map(|_| Block {
                    at: 0,
                    frames: 0,
                    valid: false,
                    left: vec![0.0; max_block],
                    right: vec![0.0; max_block],
                })
                .collect(),
            cursor: 0,
            rendered: 0,
            generation: 0,
        }
    }
    pub fn invalidate(&mut self) {
        for b in &mut self.blocks {
            b.valid = false;
        }
        self.generation += 1;
    }
    pub fn status(&self) -> AheadStatus {
        AheadStatus {
            cached_blocks: self.blocks.iter().filter(|b| b.valid).count(),
            rendered_frames: self.rendered,
            generation: self.generation,
        }
    }
    fn find(&self, at: u64, frames: usize) -> Option<usize> {
        self.blocks
            .iter()
            .position(|b| b.valid && b.at == at && b.frames == frames)
    }
    fn fill(&mut self, timeline: &Timeline, at: u64, frames: usize) -> usize {
        let i = self.cursor;
        let block = &mut self.blocks[i];
        block.left[..frames].fill(0.0);
        block.right[..frames].fill(0.0);
        timeline.mix_audio(at, &mut block.left[..frames], &mut block.right[..frames]);
        block.at = at;
        block.frames = frames;
        block.valid = true;
        self.cursor = (i + 1) % self.blocks.len();
        self.rendered += frames as u64;
        i
    }
    pub fn mix(&mut self, timeline: &Timeline, at: u64, left: &mut [f32], right: &mut [f32]) {
        // Stream pages may arrive between callbacks. Never cache a temporary disk miss.
        if !timeline.streams().is_empty() {
            self.invalidate();
            timeline.mix_audio(at, left, right);
            return;
        }
        let frames = left.len().min(right.len());
        let i = match self.find(at, frames) {
            Some(i) => i,
            None => {
                self.invalidate();
                self.cursor = 0;
                self.fill(timeline, at, frames)
            }
        };
        left[..frames].copy_from_slice(&self.blocks[i].left[..frames]);
        right[..frames].copy_from_slice(&self.blocks[i].right[..frames]);
        self.blocks[i].valid = false;
        // The next two blocks are made before they are needed and held at their song positions.
        for block in 1..=2 {
            let next = at + (frames * block) as u64;
            if self.find(next, frames).is_none() {
                self.fill(timeline, next, frames);
            }
        }
    }
}
