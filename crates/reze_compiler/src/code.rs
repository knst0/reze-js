//! Generated code plus the source positions it came from, for the source map.

use std::fmt;

use oxc_sourcemap::SourceMapBuilder;

/// A code fragment. `maps` holds `(offset in s, offset in the source)` pairs in `s` order.
#[derive(Default)]
pub struct Code {
    pub s: String,
    maps: Vec<(u32, u32)>,
}

impl Code {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, s: &str) {
        self.s.push_str(s);
    }

    /// Maps what is pushed next to `src`.
    pub fn mark(&mut self, src: u32) {
        let at = self.s.len() as u32;
        if self.maps.last().is_some_and(|&(g, _)| g == at) {
            self.maps.pop();
        }
        self.maps.push((at, src));
    }

    /// Copies `source[start..end]`, mapped at its start and at every line start inside it.
    pub fn src(&mut self, source: &str, start: u32, end: u32) {
        if start >= end {
            return;
        }
        let slice = &source[start as usize..end as usize];
        self.mark(start);
        let base = self.s.len();
        self.s.push_str(slice);
        for (i, b) in slice.bytes().enumerate() {
            if b == b'\n' && i + 1 < slice.len() {
                self.maps.push(((base + i + 1) as u32, start + i as u32 + 1));
            }
        }
    }

    pub fn append(&mut self, other: Code) {
        let offset = self.s.len() as u32;
        if other.maps.first().is_some_and(|&(g, _)| g == 0)
            && self.maps.last().is_some_and(|&(g, _)| g == offset)
        {
            self.maps.pop();
        }
        self.maps.extend(other.maps.into_iter().map(|(g, s)| (g + offset, s)));
        self.s.push_str(&other.s);
    }

    /// Serializes the mappings as a source map v3 JSON string.
    pub fn source_map(&self, filename: &str, source: &str) -> String {
        let mut builder = SourceMapBuilder::default();
        let src_id = builder.add_source_and_content(filename, source);
        let src_lines = LineIndex::new(source);
        let mut gen_lines = LineCursor::new(&self.s);
        for &(g, s) in &self.maps {
            let (dst_line, dst_col) = gen_lines.locate(g as usize);
            let (src_line, src_col) = src_lines.locate(source, s as usize);
            builder.add_token(dst_line, dst_col, src_line, src_col, Some(src_id), None);
        }
        builder.into_sourcemap().to_json_string()
    }
}

impl fmt::Write for Code {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.s.push_str(s);
        Ok(())
    }
}

/// UTF-16 length of `s`, as source maps count columns.
fn utf16_len(s: &str) -> u32 {
    if s.is_ascii() { s.len() as u32 } else { s.encode_utf16().count() as u32 }
}

/// Line starts of a text, for random-access `offset → (line, column)`.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(text.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1));
        Self { starts }
    }

    fn locate(&self, text: &str, offset: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        (line as u32, utf16_len(&text[self.starts[line]..offset]))
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
