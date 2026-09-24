use std::fmt;

use oxc_sourcemap::SourceMapBuilder;

use crate::diagnostic::{LineIndex, utf16_len};

/// Generated code plus `(offset in text, offset in the source)` marks in text order, for the
/// source map.
#[derive(Default)]
pub struct Code {
    pub text: String,
    marks: Vec<(u32, u32)>,
}

impl Code {
    pub fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Maps what is pushed next to source offset `src`.
    pub fn mark(&mut self, src: u32) {
        let at = self.text.len() as u32;
        if self.marks.last().is_some_and(|&(g, _)| g == at) {
            self.marks.pop();
        }
        self.marks.push((at, src));
    }

    /// Copies `source[start..end]`, mapped at its start and at every line start inside it.
    pub fn src(&mut self, source: &str, start: u32, end: u32) {
        if start >= end {
            return;
        }
        let slice = &source[start as usize..end as usize];
        self.mark(start);
        let base = self.text.len();
        self.text.push_str(slice);
        for (i, b) in slice.bytes().enumerate() {
            if b == b'\n' && i + 1 < slice.len() {
                self.marks.push(((base + i + 1) as u32, start + i as u32 + 1));
            }
        }
    }

    pub fn append(&mut self, other: Code) {
        let offset = self.text.len() as u32;
        if other.marks.first().is_some_and(|&(g, _)| g == 0)
            && self.marks.last().is_some_and(|&(g, _)| g == offset)
        {
            self.marks.pop();
        }
        self.marks.extend(other.marks.into_iter().map(|(g, s)| (g + offset, s)));
        self.text.push_str(&other.text);
    }

    /// Source map v3 JSON.
    pub fn source_map(&self, filename: &str, source: &str) -> String {
        let mut builder = SourceMapBuilder::default();
        let src_id = builder.add_source_and_content(filename, source);
        let src_lines = LineIndex::new(source);
        let mut gen_lines = LineCursor::new(&self.text);
        for &(g, s) in &self.marks {
            let (dst_line, dst_col) = gen_lines.locate(g as usize);
            let (src_line, src_col) = src_lines.locate(source, s as usize);
            builder.add_token(dst_line, dst_col, src_line, src_col, Some(src_id), None);
        }
        builder.into_sourcemap().to_json_string()
    }
}

impl fmt::Write for Code {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.text.push_str(s);
        Ok(())
    }
}

/// Forward-only `offset → (line, column)` for monotonically increasing offsets.
struct LineCursor<'t> {
    text: &'t str,
    line: u32,
    line_start: usize,
    scanned: usize,
}

impl<'t> LineCursor<'t> {
    fn new(text: &'t str) -> Self {
        Self { text, line: 0, line_start: 0, scanned: 0 }
    }

    fn locate(&mut self, offset: usize) -> (u32, u32) {
        for (i, b) in self.text.as_bytes()[self.scanned..offset].iter().enumerate() {
            if *b == b'\n' {
                self.line += 1;
                self.line_start = self.scanned + i + 1;
            }
        }
        self.scanned = offset;
        (self.line, utf16_len(&self.text[self.line_start..offset]))
    }
}
