//! The controller assignments: `midimap.toml` (OPS-007).
//!
//! A knob (a control change) or a pad (a note) of a MIDI controller is assigned to one part's
//! volume or pan, or to a command. One knob has one target and one target has one knob: assigning
//! again replaces. The channel does not matter, so the same knob works whatever channel the
//! controller sends on. The assigned messages are not recorded as notes or controller events.

use serde_json::{Value, json};

use crate::tomlw;

/// What a controller sends, without its channel and value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Message {
    /// A control change of this number (a knob, a slider, a pedal).
    Cc(u8),
    /// A note of this pitch (a pad, a key).
    Note(u8),
}

impl Message {
    /// `cc:21`, `note:41`.
    pub fn parse(text: &str) -> Result<Message, String> {
        let bad = || format!("「{text}」はメッセージの書き方ではありません（cc:21 や note:41）");
        let (kind, n) = text.trim().split_once(':').ok_or_else(bad)?;
        let n: u8 = n.trim().parse().map_err(|_| bad())?;
        if n > 127 {
            return Err(format!("{text}: 番号は 0〜127 です"));
        }
        match kind.trim().to_ascii_lowercase().as_str() {
            "cc" => Ok(Message::Cc(n)),
            "note" => Ok(Message::Note(n)),
            _ => Err(bad()),
        }
    }

    /// The message of a MIDI message: a control change, or a note being played. `None` for
    /// everything else (pitch bend, a note being let go...).
    pub fn of(msg: [u8; 3]) -> Option<Message> {
        match msg[0] & 0xf0 {
            0xb0 if msg[1] < 128 => Some(Message::Cc(msg[1])),
            0x90 if msg[1] < 128 && msg[2] > 0 => Some(Message::Note(msg[1])),
            _ => None,
        }
    }

    /// Whether `msg` belongs to this assignment (a note's release too).
    pub fn matches(self, msg: [u8; 3]) -> bool {
        match (self, msg[0] & 0xf0) {
            (Message::Cc(n), 0xb0) => msg[1] == n,
            (Message::Note(n), 0x90 | 0x80) => msg[1] == n,
            _ => false,
        }
    }

    /// The label on the screen: `CC 21`, `ノート 41`.
    pub fn label(self) -> String {
        match self {
            Message::Cc(n) => format!("CC {n}"),
            Message::Note(n) => format!("ノート {n}"),
        }
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Cc(n) => write!(f, "cc:{n}"),
            Message::Note(n) => write!(f, "note:{n}"),
        }
    }
}

/// What a message moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The volume of a part (the track's id).
    Volume(String),
    /// The pan of a part.
    Pan(String),
    /// A command (a pad runs it).
    Command(String),
}

impl Target {
    /// `track.volume:<id>`, `track.pan:<id>`, or a command's name.
    pub fn parse(text: &str) -> Result<Target, String> {
        let text = text.trim();
        let part = |prefix: &str| {
            text.strip_prefix(prefix)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(String::from)
        };
        if let Some(id) = part("track.volume:") {
            return Ok(Target::Volume(id));
        }
        if let Some(id) = part("track.pan:") {
            return Ok(Target::Pan(id));
        }
        let first = text.split_whitespace().next().unwrap_or("");
        if first.is_empty() || first.contains(':') {
            return Err(format!(
                "「{text}」は割り当て先ではありません（track.volume:<id>・track.pan:<id>・コマンド名）"
            ));
        }
        Ok(Target::Command(text.to_string()))
    }

    /// The part this moves, `None` for a command.
    pub fn track(&self) -> Option<&str> {
        match self {
            Target::Volume(id) | Target::Pan(id) => Some(id),
            Target::Command(_) => None,
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Volume(id) => write!(f, "track.volume:{id}"),
            Target::Pan(id) => write!(f, "track.pan:{id}"),
            Target::Command(c) => f.write_str(c),
        }
    }
}

/// One knob or pad and what it moves. No port means any port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub port: Option<String>,
    pub message: Message,
    pub target: Target,
}

impl Row {
    pub fn to_json(&self) -> Value {
        json!({
            "port": self.port,
            "message": self.message.to_string(),
            "target": self.target.to_string(),
        })
    }

    fn hears(&self, port: &str, msg: [u8; 3]) -> bool {
        self.port.as_deref().is_none_or(|p| p == port) && self.message.matches(msg)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Midimap {
    rows: Vec<Row>,
}

impl Midimap {
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Assigns `message` of `port` to `target`. The knob's earlier target and the target's
    /// earlier knob are let go. Returns the rows it replaced.
    pub fn assign(&mut self, port: Option<&str>, message: Message, target: Target) -> Vec<Row> {
        let port = port.map(str::trim).filter(|p| !p.is_empty());
        let mut gone = Vec::new();
        self.rows.retain(|r| {
            let same_knob = r.port.as_deref() == port && r.message == message;
            // A command can have several pads; a part's value only one knob.
            let same_target = r.target.track().is_some() && r.target == target;
            if same_knob || same_target {
                gone.push(r.clone());
            }
            !(same_knob || same_target)
        });
        self.rows.push(Row {
            port: port.map(String::from),
            message,
            target,
        });
        gone
    }

    /// Lets the knob go. False when it had nothing.
    pub fn unassign(&mut self, port: Option<&str>, message: Message) -> bool {
        let port = port.map(str::trim).filter(|p| !p.is_empty());
        let before = self.rows.len();
        self.rows
            .retain(|r| !(r.port.as_deref() == port && r.message == message));
        self.rows.len() != before
    }

    /// Lets the knob go whatever its port.
    pub fn unassign_any(&mut self, message: Message) -> usize {
        let before = self.rows.len();
        self.rows.retain(|r| r.message != message);
        before - self.rows.len()
    }

    /// Lets the knob of a part's value (or the pads of a command) go.
    pub fn unassign_target(&mut self, target: &Target) -> usize {
        let before = self.rows.len();
        self.rows.retain(|r| &r.target != target);
        before - self.rows.len()
    }

    /// Lets every knob of `port` go.
    pub fn unassign_port(&mut self, port: &str) -> usize {
        let before = self.rows.len();
        self.rows.retain(|r| r.port.as_deref() != Some(port));
        before - self.rows.len()
    }

    pub fn clear(&mut self) -> usize {
        std::mem::take(&mut self.rows).len()
    }

    /// The row that takes `msg` from `port` (one that names the port before one for any port).
    pub fn find(&self, port: &str, msg: [u8; 3]) -> Option<&Row> {
        let mut found: Option<&Row> = None;
        for r in &self.rows {
            if r.hears(port, msg) && (found.is_none() || r.port.is_some()) {
                found = Some(r);
            }
        }
        found
    }

    /// The knob of a part's value.
    pub fn of_target(&self, target: &Target) -> Option<&Row> {
        self.rows.iter().find(|r| &r.target == target)
    }

    /// The rows that move a part not in `alive` (the part was deleted).
    pub fn orphans<'a>(&'a self, alive: &dyn Fn(&str) -> bool) -> Vec<&'a Row> {
        self.rows
            .iter()
            .filter(|r| r.target.track().is_some_and(|t| !alive(t)))
            .collect()
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.rows.iter().map(Row::to_json).collect())
    }

    pub fn from_toml(t: &toml::Table, problems: &mut Vec<String>) -> Midimap {
        let mut out = Midimap::default();
        let Some(list) = t.get("assign") else {
            return out;
        };
        let Some(list) = list.as_array() else {
            problems.push("midimap.toml: assign は [[assign]] の並びです".into());
            return out;
        };
        for (i, item) in list.iter().enumerate() {
            let text = |key: &str| item.get(key).and_then(toml::Value::as_str);
            let row = (|| {
                let message = Message::parse(text("message").ok_or("message がありません")?)?;
                let target = Target::parse(text("target").ok_or("target がありません")?)?;
                Ok::<_, String>((text("port"), message, target))
            })();
            match row {
                Ok((port, message, target)) => {
                    out.assign(port, message, target);
                }
                Err(e) => problems.push(format!("midimap.toml: {} 番目の割り当て: {e}", i + 1)),
            }
        }
        out
    }

    pub fn to_toml(&self) -> String {
        let mut s = String::from("# midimap.toml（コントローラーの割り当て）\n");
        for r in &self.rows {
            s.push_str("\n[[assign]]\n");
            if let Some(p) = &r.port {
                s.push_str(&format!("port = {}\n", tomlw::string(p)));
            }
            s.push_str(&format!(
                "message = {}\ntarget = {}\n",
                tomlw::string(&r.message.to_string()),
                tomlw::string(&r.target.to_string())
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(id: &str) -> Target {
        Target::Volume(id.into())
    }

    #[test]
    fn a_message_is_written_and_read_the_same() {
        for text in ["cc:0", "cc:127", "note:41"] {
            assert_eq!(Message::parse(text).unwrap().to_string(), text);
        }
        assert!(Message::parse("cc:128").is_err());
        assert!(Message::parse("pc:3").is_err());
        assert!(Message::parse("21").is_err());
        assert_eq!(Message::parse(" CC : 7 ").unwrap(), Message::Cc(7));
    }

    #[test]
    fn a_knob_is_told_from_the_message_of_any_channel() {
        assert_eq!(Message::of([0xb0, 21, 64]), Some(Message::Cc(21)));
        assert_eq!(Message::of([0xb5, 21, 0]), Some(Message::Cc(21)));
        assert_eq!(Message::of([0x90, 41, 100]), Some(Message::Note(41)));
        assert_eq!(
            Message::of([0x90, 41, 0]),
            None,
            "a note on with no force is off"
        );
        assert_eq!(Message::of([0x80, 41, 0]), None);
        assert_eq!(Message::of([0xe0, 0, 64]), None);
        assert!(
            Message::Note(41).matches([0x80, 41, 0]),
            "the release is the pad's too"
        );
        assert!(!Message::Cc(41).matches([0x90, 41, 100]));
    }

    #[test]
    fn a_target_is_a_part_s_value_or_a_command() {
        assert_eq!(Target::parse("track.volume:t3").unwrap(), vol("t3"));
        assert_eq!(
            Target::parse("track.pan:t3").unwrap(),
            Target::Pan("t3".into())
        );
        assert_eq!(
            Target::parse("transport.play").unwrap(),
            Target::Command("transport.play".into())
        );
        assert!(Target::parse("track.volume:").is_err());
        assert!(Target::parse("send:1").is_err());
        assert!(Target::parse("").is_err());
        assert_eq!(vol("t3").to_string(), "track.volume:t3");
    }

    #[test]
    fn one_knob_has_one_part_and_one_part_one_knob() {
        let mut m = Midimap::default();
        m.assign(Some("K49"), Message::Cc(21), vol("t1"));
        // The knob moves to another part: the first is let go.
        let gone = m.assign(Some("K49"), Message::Cc(21), vol("t2"));
        assert_eq!(gone.len(), 1);
        assert_eq!(m.rows().len(), 1);
        assert_eq!(m.rows()[0].target, vol("t2"));
        // The part gets another knob: its old knob is let go.
        let gone = m.assign(Some("K49"), Message::Cc(22), vol("t2"));
        assert_eq!(gone[0].message, Message::Cc(21));
        assert_eq!(m.rows().len(), 1);
        // The pan of the same part is another value.
        m.assign(Some("K49"), Message::Cc(23), Target::Pan("t2".into()));
        assert_eq!(m.rows().len(), 2);
        // A command may have several pads.
        m.assign(
            None,
            Message::Note(41),
            Target::Command("transport.play".into()),
        );
        m.assign(
            None,
            Message::Note(42),
            Target::Command("transport.play".into()),
        );
        assert_eq!(m.rows().len(), 4);
    }

    #[test]
    fn a_message_is_found_by_its_port_and_a_named_port_comes_first() {
        let mut m = Midimap::default();
        m.assign(None, Message::Cc(21), vol("any"));
        assert_eq!(m.find("K49", [0xb0, 21, 5]).unwrap().target, vol("any"));
        m.assign(Some("K49"), Message::Cc(21), vol("k49"));
        assert_eq!(m.find("K49", [0xb2, 21, 5]).unwrap().target, vol("k49"));
        assert_eq!(m.find("Other", [0xb0, 21, 5]).unwrap().target, vol("any"));
        assert!(m.find("K49", [0xb0, 22, 5]).is_none());
        assert!(m.find("K49", [0x90, 21, 5]).is_none());
        assert!(m.unassign(Some("K49"), Message::Cc(21)));
        assert!(!m.unassign(Some("K49"), Message::Cc(21)));
        assert_eq!(m.find("K49", [0xb0, 21, 5]).unwrap().target, vol("any"));
    }

    #[test]
    fn the_file_reads_back_and_a_bad_row_is_skipped_and_named() {
        let mut m = Midimap::default();
        m.assign(Some("Launchkey \"49\""), Message::Cc(21), vol("t1"));
        m.assign(
            None,
            Message::Note(41),
            Target::Command("transport.play".into()),
        );
        let text = m.to_toml();
        let mut problems = Vec::new();
        let back = Midimap::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, m);
        assert_eq!(back.to_toml(), text);

        let t: toml::Table = "[[assign]]\nmessage = \"cc:200\"\ntarget = \"track.volume:t1\"\n\n[[assign]]\nmessage = \"cc:7\"\ntarget = \"track.pan:t2\"\n"
            .parse()
            .unwrap();
        let mut problems = Vec::new();
        let m = Midimap::from_toml(&t, &mut problems);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("1 番目"), "{problems:?}");
        assert_eq!(m.rows().len(), 1);
    }

    #[test]
    fn a_part_that_is_gone_leaves_an_orphan() {
        let mut m = Midimap::default();
        m.assign(None, Message::Cc(1), vol("t1"));
        m.assign(None, Message::Cc(2), vol("t2"));
        m.assign(
            None,
            Message::Note(3),
            Target::Command("transport.play".into()),
        );
        let orphans = m.orphans(&|id| id == "t1");
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].target, vol("t2"));
    }
}
