use std::any::Any;

use crate::html::{Html, escape_attr};
use crate::{Manifest, Params, RouteError, Table, Value};

/// The state of one render: the head, ids, the client state entries, and the status.
#[derive(Debug)]
pub struct Render {
    depth: usize,
    /// Keyed head elements in insertion order, with the depth that wrote each.
    keyed: Vec<(String, isize, String)>,
    unkeyed: Vec<String>,
    ids: usize,
    status: u16,
    entries: Vec<String>,
    events: Vec<String>,
    eager: bool,
}

/// What `<Errored>` restores when its children fail.
#[derive(Debug)]
pub struct Mark {
    entries: usize,
    unkeyed: usize,
    keyed: Vec<(String, isize, String)>,
    ids: usize,
    depth: usize,
}

impl Render {
    fn new(depth: usize) -> Self {
        Self {
            depth,
            keyed: vec![
                ("meta:charset".into(), -1, r#"<meta charset="utf-8">"#.into()),
                (
                    "meta:name=viewport".into(),
                    -1,
                    r#"<meta name="viewport" content="width=device-width, initial-scale=1">"#
                        .into(),
                ),
            ],
            unkeyed: Vec::new(),
            ids: 0,
            status: 200,
            entries: Vec::new(),
            events: Vec::new(),
            eager: false,
        }
    }

    /// A `<head>` element: a keyed one replaces the same key written at a lower depth.
    pub fn head(&mut self, key: Option<&str>, html: Html) {
        let html = html.into_string();
        let depth = self.depth as isize;
        match key {
            None => {
                if !self.unkeyed.contains(&html) {
                    self.unkeyed.push(html);
                }
            }
            Some(key) => match self.keyed.iter_mut().find(|(k, _, _)| k == key) {
                Some(entry) if entry.1 <= depth => {
                    entry.1 = depth;
                    entry.2 = html;
                }
                Some(_) => {}
                None => self.keyed.push((key.to_string(), depth, html)),
            },
        }
    }

    /// `createUniqueId()`.
    pub fn id(&mut self) -> String {
        self.ids += 1;
        format!("rz-{}", self.ids - 1)
    }

    /// Adds the state entry of a client component instance, of JSON `captures`, and returns its id.
    pub fn scope(
        &mut self,
        component: usize,
        events: &str,
        eager: bool,
        captures: Vec<String>,
    ) -> usize {
        for name in events.split(' ') {
            if !name.is_empty() && !self.events.iter().any(|event| event == name) {
                self.events.push(name.to_string());
            }
        }
        self.eager |= eager;
        self.entry(component.to_string(), captures)
    }

    /// Adds the state entry of a callback call that client code captures values of.
    pub fn row(&mut self, parent: usize, kind: usize, captures: Vec<String>) -> usize {
        self.entry(format!("{},{kind}", -(parent as i64) - 1), captures)
    }

    fn entry(&mut self, head: String, captures: Vec<String>) -> usize {
        let mut entry = format!("[{head}");
        for capture in captures {
            entry.push(',');
            entry.push_str(&capture);
        }
        entry.push(']');
        self.entries.push(entry);
        self.entries.len() - 1
    }

    pub fn mark(&self) -> Mark {
        Mark {
            entries: self.entries.len(),
            unkeyed: self.unkeyed.len(),
            keyed: self.keyed.clone(),
            ids: self.ids,
            depth: self.depth,
        }
    }

    pub fn restore(&mut self, mark: Mark) {
        self.entries.truncate(mark.entries);
        self.unkeyed.truncate(mark.unkeyed);
        self.keyed = mark.keyed;
        self.ids = mark.ids;
        self.depth = mark.depth;
    }

    /// Records the error an `<Errored>` caught: the page answers with its status.
    pub fn fail(&mut self, error: &RouteError) {
        self.status = error.status;
    }
}

/// What one render reads: the route, its params, loader data, and the first failed segment.
pub struct Page<'p> {
    pub(crate) table: &'static Table,
    pub(crate) chain: &'static [usize],
    pub(crate) params: &'p Params,
    pub(crate) data: &'p [Option<Box<dyn Value>>],
    pub(crate) error: Option<(usize, RouteError)>,
}

impl Page<'_> {
    /// The loader data of the segment at `depth`.
    pub fn data<T: Any>(&self, depth: usize) -> &T {
        self.data
            .get(depth)
            .and_then(Option::as_ref)
            .and_then(|value| value.any().downcast_ref())
            .expect("the loader of a rendered segment has run")
    }

    pub fn param(&self, name: &str) -> &str {
        self.params.get(name).unwrap_or_default()
    }

    pub fn param_opt(&self, name: &str) -> Option<&str> {
        self.params.get(name)
    }
}

fn level(out: &mut Html, cx: &mut Render, page: &Page<'_>, depth: usize) -> Result<(), RouteError> {
    if let Some((at, error)) = &page.error
        && *at == depth
    {
        return Err(error.clone());
    }
    (page.table.nodes[page.chain[depth]].render)(out, cx, page)
}

/// The `children` of a layout: the next segment, or nothing below the page.
pub fn children(out: &mut Html, cx: &mut Render, page: &Page<'_>) -> Result<(), RouteError> {
    let depth = cx.depth + 1;
    let failed = page.error.as_ref().is_some_and(|(at, _)| *at == depth);
    if depth >= page.chain.len() && !failed {
        return Ok(());
    }
    let mut inner = Html::new();
    let previous = cx.depth;
    cx.depth = depth;
    let result = level(&mut inner, cx, page, depth);
    cx.depth = previous;
    result?;
    out.raw("<!--s");
    out.display(depth);
    out.raw("-->");
    out.append(inner);
    out.raw("<!--/-->");
    Ok(())
}

/// Renders the page from segment `from` down: the whole document from `0`, or for a client
/// navigation the document whose body is that segment alone. Returns the document and its status.
pub(crate) fn render(
    page: &Page<'_>,
    chunk: Option<usize>,
    manifest: Option<&Manifest>,
    from: usize,
) -> Result<(String, u16), RouteError> {
    let mut cx = Render::new(from);
    let mut root = Html::new();
    level(&mut root, &mut cx, page, from)?;
    let mut document = String::from("<!doctype html><html><head>");
    for (_, _, html) in &cx.keyed {
        document.push_str(html);
    }
    for html in &cx.unkeyed {
        document.push_str(html);
    }
    if let Some(manifest) = manifest {
        document.push_str(&format!(
            r#"<script type="module" async src="{}"></script>"#,
            escape_attr(&manifest.dispatcher)
        ));
    }
    document.push_str("</head><body>");
    document.push_str(root.as_str());
    if let (false, Some(manifest), Some(chunk)) = (cx.entries.is_empty(), manifest, chunk) {
        let mut block = String::from("[");
        reze::js::push_json_string(&mut block, &cx.events.join(" "));
        block.push(',');
        match manifest.chunks.get(&chunk.to_string()) {
            Some(url) => reze::js::push_json_string(&mut block, url),
            None => block.push_str("null"),
        }
        block.push_str(if cx.eager { ",1" } else { ",0" });
        for entry in &cx.entries {
            block.push(',');
            block.push_str(entry);
        }
        block.push(']');
        document.push_str(r#"<script type="rz">"#);
        document.push_str(&block.replace('<', "\\u003c"));
        document.push_str("</script>");
    }
    document.push_str("</body></html>");
    Ok((document, cx.status))
}
