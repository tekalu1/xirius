//! KEY-015: a macro run from the screen. Each step goes the way its key would, so a step that
//! works on the selection takes the one on the screen, and the steps are one step of the history.

use xirius_prefs::cmdline;

use crate::studio::{Geometry, Studio};

impl Studio {
    /// Runs the macro `name`. Stops at the first step that fails and says why.
    pub(crate) fn run_macro(&mut self, g: &Geometry, name: &str) -> bool {
        let steps = {
            let shared = self.song.settings().clone();
            let s = shared.lock().unwrap();
            s.macros.get(name).map(|m| m.steps.clone())
        };
        let Some(steps) = steps else {
            self.error = Some(format!("「{name}」というマクロはありません"));
            return true;
        };
        if steps.is_empty() {
            self.error = Some(format!("マクロ「{name}」に手順がありません"));
            return true;
        }
        self.error = None;
        // A wheel's step that is still open must not be closed with the macro's group (P3-7 of
        // review 15).
        self.close_wheel_group();
        self.song.session_mut().begin_group();
        for line in &steps {
            let (command, args) = match cmdline::parse(line, Some(self.song.registry())) {
                Ok(v) => v,
                Err(e) => {
                    self.error = Some(e);
                    break;
                }
            };
            let command = self.song.registry().canonical(&command).to_string();
            self.run_parsed(g, command, args);
            if self.error.is_some() {
                break;
            }
        }
        self.song.session_mut().end_group();
        true
    }
}
