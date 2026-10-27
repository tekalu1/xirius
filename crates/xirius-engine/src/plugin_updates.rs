//! Acceptance of plugin values from explicit intent, editors and state receipts.
//! Instance identity survives closing until acknowledgement; intent generations
//! advance for every explicit change, reset and undo, including an ABA restore.
pub fn new_instance() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.try_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |id| id.checked_add(1),
    )
    .expect("plugin instance identifiers exhausted")
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub instance: u64,
    pub generation: u64,
    /// Sequence at the owning endpoint. Commands and SDK observations carry
    /// their execution order; independent receipts do not advance this cutoff.
    pub revision: u64,
}
pub const STALE_UPDATE: &str = "プラグインの状態が変更されました。もう一度操作してください";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    Editor,
    Capture,
    /// A validated snapshot used to construct a successor; never a save receipt.
    Transfer,
}

#[derive(Clone, Copy)]
pub enum Field<'a> {
    State,
    Parameter {
        id: u32,
        title: &'a str,
    },
    /// Receipts and explicit owner commands require the exact intent generation.
    Receipt,
}
#[derive(Clone, Debug)]
struct Source {
    instance: u64,
    protected: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateGate {
    stamp: Stamp,
    sources: Vec<Source>,
    state_floor: u64,
    value_floor: u64,
    changed: Vec<(String, u64)>,
}
impl UpdateGate {
    pub fn new(stamp: Stamp) -> Self {
        Self {
            stamp,
            ..Self::default()
        }
    }
    pub fn stamp(&self) -> Stamp {
        self.stamp
    }
    pub fn rebind(&self, instance: u64) -> Self {
        let mut next = self.clone();
        if instance != next.stamp.instance {
            next.stamp.revision = 0;
        }
        next.stamp.instance = instance;
        next.sources.clear();
        next
    }
    pub fn successor(&self, instance: u64, protected: Vec<String>) -> Self {
        let mut next = self.clone();
        for source in &mut next.sources {
            source.protected.extend(protected.iter().cloned());
        }
        next.sources.push(Source {
            instance: self.stamp.instance,
            protected,
        });
        next.stamp.instance = instance;
        if instance != self.stamp.instance {
            next.stamp.revision = 0;
        }
        next
    }
    pub fn retain_sources(&mut self, mut alive: impl FnMut(u64) -> bool) {
        self.sources.retain(|source| alive(source.instance));
    }
    pub fn next_generation(&self) -> u64 {
        self.stamp
            .generation
            .checked_add(1)
            .expect("plugin intent generations exhausted")
    }
    pub fn next_revision(&mut self) -> Stamp {
        self.stamp.revision = self
            .stamp
            .revision
            .checked_add(1)
            .expect("plugin revision sequence exhausted");
        self.stamp
    }
    pub fn received(&mut self, source: Stamp) {
        if source.instance == self.stamp.instance {
            self.stamp.revision = self.stamp.revision.max(source.revision);
        }
    }
    /// Map model deltas to a monotonic intent boundary before sending SDK work.
    pub fn explicit(&mut self, generation: u64, replace: bool, keys: &[String]) {
        assert!(generation >= self.stamp.generation);
        self.stamp.generation = generation;
        if replace {
            self.state_floor = generation;
            self.value_floor = generation;
            self.changed.clear();
        }
        for key in keys {
            self.changed
                .retain(|(old, _)| !old.eq_ignore_ascii_case(key));
            self.changed.push((key.clone(), generation));
        }
    }
    pub fn command(
        &mut self,
        generation: u64,
        revision: u64,
        basis: u64,
        replace: bool,
        keys: &[String],
    ) -> Option<Stamp> {
        if generation < self.stamp.generation {
            return None;
        }
        if generation == self.stamp.generation && basis < self.stamp.revision {
            self.stamp.revision = self.stamp.revision.max(revision);
            return None;
        }
        self.explicit(generation, replace, keys);
        self.stamp.revision = self.stamp.revision.max(revision);
        Some(self.stamp)
    }
    /// The only stale-update decision, shared by remote/local and all origins.
    pub fn accepts(&self, source: Stamp, field: Field<'_>) -> bool {
        let own = source.instance == self.stamp.instance;
        let ancestor = self.sources.iter().find(|s| s.instance == source.instance);
        if !own && ancestor.is_none() {
            return false;
        }
        if source.generation > self.stamp.generation {
            return false;
        }
        match field {
            Field::Receipt => {
                own && source.generation == self.stamp.generation
                    && source.revision >= self.stamp.revision
            }
            Field::State => source.generation >= self.state_floor,
            Field::Parameter { id, title } => {
                source.generation >= self.value_floor
                    && self
                        .changed
                        .iter()
                        .all(|(key, since)| !names(key, id, title) || source.generation >= *since)
                    && ancestor.is_none_or(|source| {
                        !source.protected.iter().any(|key| names(key, id, title))
                    })
            }
        }
    }
    /// A newer instance's actual editor action takes precedence over its ancestors.
    pub fn observed(&mut self, source: Stamp, field: Field<'_>) {
        if !self.accepts(source, field) {
            return;
        }
        self.received(source);
        match field {
            Field::State => {
                self.state_floor = self.state_floor.max(source.generation);
                self.value_floor = self.value_floor.max(source.generation);
                self.sources
                    .retain(|ancestor| ancestor.instance >= source.instance);
            }
            Field::Parameter { id, title } => {
                for key in [id.to_string(), title.to_string()] {
                    if key.is_empty() {
                        continue;
                    }
                    self.changed
                        .retain(|(old, _)| !old.eq_ignore_ascii_case(&key));
                    self.changed.push((key, source.generation));
                }
                for ancestor in &mut self.sources {
                    if ancestor.instance >= source.instance {
                        continue;
                    }
                    for key in [id.to_string(), title.to_string()] {
                        if !key.is_empty()
                            && !ancestor
                                .protected
                                .iter()
                                .any(|old| old.eq_ignore_ascii_case(&key))
                        {
                            ancestor.protected.push(key);
                        }
                    }
                }
            }
            Field::Receipt => {}
        }
    }
}
fn names(key: &str, id: u32, title: &str) -> bool {
    key.parse::<u32>().ok() == Some(id) || (!title.is_empty() && key.eq_ignore_ascii_case(title))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_cannot_overwrite_an_unreported_current_editor_action_but_explicit_intent_wins() {
        let mut owner = UpdateGate::new(Stamp {
            instance: 2,
            generation: 0,
            revision: 10,
        });
        let editor = owner.next_revision();
        assert_eq!(editor.revision, 11);
        assert!(owner.command(0, 11, 10, false, &["1".into()]).is_none());
        assert!(owner.command(1, 12, 10, false, &["1".into()]).is_some());
        assert!(!owner.accepts(editor, Field::Receipt));
        assert!(owner.accepts(owner.stamp(), Field::Receipt));
    }
    #[test]
    fn a_new_owner_starts_its_own_sequence_and_old_receipts_cannot_use_it() {
        let old = Stamp {
            instance: 1,
            generation: 3,
            revision: 1000,
        };
        let next = UpdateGate::new(old).successor(2, Vec::new());
        assert_eq!(next.stamp().revision, 0);
        assert!(!next.accepts(old, Field::Receipt));
        assert!(next.accepts(next.stamp(), Field::Receipt));
    }
    #[test]
    fn explicit_undo_and_reset_share_the_editor_and_receipt_boundary() {
        let old = Stamp {
            instance: 1,
            generation: 0,
            revision: 0,
        };
        let mut next = UpdateGate::new(old).successor(2, Vec::new());
        let value = Field::Parameter {
            id: 1,
            title: "Gain",
        };
        assert!(next.accepts(old, value));
        next.explicit(1, false, &["gain".into()]);
        assert!(!next.accepts(old, value));
        assert!(
            next.accepts(old, Field::State),
            "opaque state survives with explicit-value replay"
        );
        assert!(!next.accepts(old, Field::Receipt));
        let current = next.stamp();
        assert!(next.accepts(current, value));
        next.explicit(2, true, &[]);
        next.explicit(3, true, &[]); // undo restores equal bytes, not the old authority
        assert!(!next.accepts(current, Field::State));
        assert!(!next.accepts(old, Field::State));
        assert!(next.accepts(next.stamp(), Field::State));
    }
    #[test]
    fn current_editor_and_successive_replacements_never_resurrect_old_authority() {
        let old = Stamp {
            instance: 1,
            generation: 0,
            revision: 0,
        };
        let mut current = UpdateGate::new(old).successor(2, Vec::new());
        current.observed(
            current.stamp(),
            Field::Parameter {
                id: 1,
                title: "Gain",
            },
        );
        let next = current.successor(3, Vec::new());
        assert!(!next.accepts(
            old,
            Field::Parameter {
                id: 1,
                title: "Gain"
            }
        ));
        assert!(next.accepts(
            old,
            Field::Parameter {
                id: 2,
                title: "Other"
            }
        ));
        current.observed(current.stamp(), Field::State);
        assert!(!current.accepts(old, Field::State));
        assert!(!current.successor(3, Vec::new()).accepts(old, Field::State));
    }
}
