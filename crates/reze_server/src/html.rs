use std::fmt::{Display, Write};

/// Markup being rendered, with the escaping of the JS render.
#[derive(Clone, Debug, Default)]
pub struct Html(String);

impl Html {
    pub fn new() -> Self {
        Self(String::new())
    }

    pub fn raw(&mut self, html: &str) {
        self.0.push_str(html);
    }

    /// Text content: escapes `&` and `<`.
    pub fn text(&mut self, text: &str) {
        escape(&mut self.0, text, |c| c == '<');
    }

    /// ` name="value"`, which escapes `&` and `"`.
    pub fn attr(&mut self, name: &str, value: &str) {
        self.0.push(' ');
        self.0.push_str(name);
        self.0.push_str("=\"");
        escape(&mut self.0, value, |c| c == '"');
        self.0.push('"');
    }

    /// A number as `String(value)` writes it.
    pub fn number(&mut self, value: f64) {
        reze::js::push_number(&mut self.0, value);
    }

    pub fn display(&mut self, value: impl Display) {
        let _ = write!(self.0, "{value}");
    }

    /// The base-36 id of a state entry.
    pub fn id(&mut self, id: usize) {
        self.0.push_str(&base36(id));
    }

    pub fn append(&mut self, other: Html) {
        self.0.push_str(&other.0);
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

fn escape(out: &mut String, text: &str, other: impl Fn(char) -> bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' if other(c) => out.push_str("&lt;"),
            '"' if other(c) => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

pub(crate) fn escape_text(text: &str) -> String {
    let mut out = String::new();
    escape(&mut out, text, |c| c == '<');
    out
}

pub(crate) fn escape_attr(text: &str) -> String {
    let mut out = String::new();
    escape(&mut out, text, |c| c == '"');
    out
}

pub(crate) fn base36(mut value: usize) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((value % 36) as u32, 36).unwrap());
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}
