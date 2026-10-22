//! A small element tree that prints itself as XML.

use std::fmt::Write;

#[derive(Clone, Debug, Default)]
pub struct El {
    name: &'static str,
    attrs: Vec<(&'static str, String)>,
    kids: Vec<El>,
    text: Option<String>,
}

impl El {
    pub fn new(name: &'static str) -> El {
        El {
            name,
            ..El::default()
        }
    }

    /// Adds an attribute.
    pub fn a(mut self, key: &'static str, value: impl ToString) -> El {
        self.attrs.push((key, value.to_string()));
        self
    }

    /// Adds an attribute when there is a value.
    pub fn opt(self, key: &'static str, value: Option<impl ToString>) -> El {
        match value {
            Some(v) => self.a(key, v),
            None => self,
        }
    }

    /// Adds a child.
    pub fn k(mut self, kid: El) -> El {
        self.kids.push(kid);
        self
    }

    /// Sets the text inside.
    pub fn t(mut self, text: impl Into<String>) -> El {
        self.text = Some(text.into());
        self
    }

    pub fn push(&mut self, kid: El) {
        self.kids.push(kid);
    }

    pub fn is_empty(&self) -> bool {
        self.kids.is_empty()
    }

    /// The whole document, with the declaration DAWproject files start with.
    pub fn document(&self) -> String {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        self.print(&mut s, 0);
        s
    }

    fn print(&self, s: &mut String, depth: usize) {
        for _ in 0..depth {
            s.push_str("  ");
        }
        s.push('<');
        s.push_str(self.name);
        for (k, v) in &self.attrs {
            let _ = write!(s, " {k}=\"{}\"", escape(v));
        }
        if let Some(t) = &self.text {
            let _ = writeln!(s, ">{}</{}>", escape(t), self.name);
            return;
        }
        if self.kids.is_empty() {
            s.push_str("/>\n");
            return;
        }
        s.push_str(">\n");
        for k in &self.kids {
            k.print(s, depth + 1);
        }
        for _ in 0..depth {
            s.push_str("  ");
        }
        let _ = writeln!(s, "</{}>", self.name);
    }
}

fn escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// A number as the file writes it: no exponent and no `-0`.
pub fn num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let v = if v == 0.0 { 0.0 } else { v };
    let s = format!("{v}");
    if s.contains('e') {
        format!("{v:.9}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_values_are_escaped() {
        let doc = El::new("Track")
            .a("name", "A & \"B\" <C>")
            .k(El::new("Channel").a("solo", false))
            .document();
        assert!(
            doc.contains("name=\"A &amp; &quot;B&quot; &lt;C&gt;\""),
            "{doc}"
        );
        let parsed = roxmltree::Document::parse(&doc).unwrap();
        assert_eq!(
            parsed.root_element().attribute("name"),
            Some("A & \"B\" <C>")
        );
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(0.25), "0.25");
        assert_eq!(num(8.0), "8");
        assert!(!num(1e-12).contains('e'));
    }
}
