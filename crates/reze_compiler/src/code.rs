use oxc_sourcemap::SourceMapBuilder;

use crate::diagnostic::{LineIndex, utf16_len};

/// Generated text with `(generated offset, source offset)` marks in generated order.
#[derive(Default)]
pub struct Code {
    pub text: String,
    marks: Vec<(u32, u32)>,
}

impl Code {
    pub fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Maps the text pushed next to source offset `source`.
    pub fn mark(&mut self, source: u32) {
        let at = self.text.len() as u32;
        if self.marks.last().is_some_and(|&(generated, _)| generated == at) {
            self.marks.pop();
        }
        self.marks.push((at, source));
    }

    /// Copies `source[start..end]`, marked at its start and at every line start inside it.
    pub fn src(&mut self, source: &str, start: u32, end: u32) {
        if start >= end {
            return;
        }
        self.mark(start);
        let base = self.text.len() as u32;
        let slice = &source[start as usize..end as usize];
        self.text.push_str(slice);
        let last = slice.len() - 1;
        for (i, b) in slice.bytes().enumerate() {
            if b == b'\n' && i < last {
                let next = i as u32 + 1;
                self.marks.push((base + next, start + next));
            }
        }
    }

    pub fn append(&mut self, other: Code) {
        let offset = self.text.len() as u32;
        if other.marks.first().is_some_and(|&(generated, _)| generated == 0)
            && self.marks.last().is_some_and(|&(generated, _)| generated == offset)
        {
            self.marks.pop();
        }
        self.marks.extend(
            other.marks.into_iter().map(|(generated, source)| (generated + offset, source)),
        );
        self.text.push_str(&other.text);
    }

    /// Moves every source offset through `f`, for text compiled from a rewrite of the source.
    pub fn remap_marks(&mut self, f: impl Fn(u32) -> u32) {
        for (_, source) in &mut self.marks {
            *source = f(*source);
        }
    }

    /// Source map v3 JSON.
    pub fn source_map(&self, filename: &str, source: &str) -> String {
        let mut builder = SourceMapBuilder::default();
        let source_id = builder.add_source_and_content(filename, source);
        let source_lines = LineIndex::new(source);
        let mut generated = LineCursor { text: &self.text, line: 0, line_start: 0, scanned: 0 };
        for &(generated_offset, source_offset) in &self.marks {
            let (line, column) = generated.locate(generated_offset as usize);
            let (source_line, source_column) = source_lines.locate(source, source_offset as usize);
            builder.add_token(line, column, source_line, source_column, Some(source_id), None);
        }
        builder.into_sourcemap().to_json_string()
    }
}

impl std::fmt::Write for Code {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.text.push_str(s);
        Ok(())
    }
}

/// Forward-only `offset → (line, UTF-16 column)` for increasing offsets.
struct LineCursor<'t> {
    text: &'t str,
    line: u32,
    line_start: usize,
    scanned: usize,
}

impl LineCursor<'_> {
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
