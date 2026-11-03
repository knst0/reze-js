use std::any::Any;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reze::Issue;

use crate::html::{Html, escape_attr};
use crate::{Chunk, Manifest, Params, RouteError, Table, Value};

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
    eager: Vec<usize>,
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
            eager: Vec::new(),
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
        if eager && !self.eager.contains(&component) {
            self.eager.push(component);
        }
        self.entry(component.to_string(), captures)
    }

    /// The link of the scope being added to the instance `owner`, from its call site `site`.
    pub fn parent(&self, owner: usize, site: usize) -> String {
        format!("[{},{site}]", self.entries.len() - owner)
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

    /// Whether the render added state entries since `mark`.
    pub fn grew(&self, mark: &Mark) -> bool {
        self.entries.len() > mark.entries
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
    pub(crate) path: &'p str,
    /// The query without `_b` and `_s`, with its `?`, or empty.
    pub(crate) search: &'p str,
    pub(crate) query: &'p [(String, String)],
    pub(crate) params: &'p Params,
    pub(crate) data: &'p [Option<Box<dyn Value>>],
    pub(crate) error: Option<(usize, RouteError)>,
    pub(crate) submission: Option<&'p Submission>,
}

/// A form post whose input failed validation, which `useSubmission` reads.
pub(crate) struct Submission {
    pub(crate) id: &'static str,
    /// The raw form values, a repeated name joined with `,`.
    pub(crate) input: Vec<(String, String)>,
    pub(crate) issues: Vec<Issue>,
}

impl Submission {
    pub(crate) fn new(id: &'static str, form: &[(String, String)], issues: Vec<Issue>) -> Self {
        let mut input: Vec<(String, String)> = Vec::new();
        for (name, value) in form {
            match input.iter_mut().find(|(key, _)| key == name) {
                Some((_, joined)) => {
                    joined.push(',');
                    joined.push_str(value);
                }
                None => input.push((name.clone(), value.clone())),
            }
        }
        Self { id, input, issues }
    }
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

    pub fn pathname(&self) -> &str {
        self.path
    }

    pub fn search(&self) -> &str {
        self.search
    }

    /// The first value of a search param.
    pub fn query(&self, name: &str) -> Option<&str> {
        if name == "_b" || name == "_s" {
            return None;
        }
        self.query.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// The raw form value `name` of the failed submission of mutation `id`.
    pub fn submission_input(&self, id: &str, name: &str) -> Option<&str> {
        let submission = self.submission.filter(|submission| submission.id == id)?;
        submission.input.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// The first issue at `key`, the path joined with `.`, of the failed submission of mutation
    /// `id`.
    pub fn submission_issue(&self, id: &str, key: &str) -> Option<&Issue> {
        let submission = self.submission.filter(|submission| submission.id == id)?;
        submission.issues.iter().find(|issue| issue.key() == key)
    }

    /// The `action` of a form bound to mutation `id`: the page URL with the id in `_m`.
    pub fn form_action(&self, id: &str) -> String {
        let mut search = form_urlencoded::Serializer::new(String::new());
        search.extend_pairs(
            self.query.iter().filter(|(key, _)| !matches!(key.as_str(), "_b" | "_s" | "_m")),
        );
        search.append_pair("_m", id);
        format!("{}?{}", self.path, search.finish())
    }

    /// Whether an anchor's `href` points at the current path.
    pub fn active(&self, href: &str) -> bool {
        href.starts_with('/')
            && !href.starts_with("//")
            && href.split(['?', '#']).next() == Some(self.path)
    }
}

const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');
const SPLAT: &AsciiSet = &COMPONENT.remove(b'/');

/// Appends a param to a path as `encodeURIComponent` does, keeping each `/` of a `splat`.
pub fn push_segment(out: &mut String, value: &str, splat: bool) {
    out.extend(utf8_percent_encode(value, if splat { SPLAT } else { COMPONENT }));
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
    chunk: Option<Chunk>,
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
        let url = |block: &mut String, key: &str| match manifest.chunks.get(key) {
            Some(url) => reze::js::push_json_string(block, url),
            None => block.push_str("null"),
        };
        match chunk {
            Chunk::Route(key) => url(&mut block, key),
            Chunk::Factories(keys) => {
                block.push('{');
                for (index, (id, key)) in keys.iter().enumerate() {
                    if index > 0 {
                        block.push(',');
                    }
                    block.push_str(&format!("\"{id}\":"));
                    url(&mut block, key);
                }
                block.push('}');
            }
        }
        let eager: Vec<String> = cx.eager.iter().map(ToString::to_string).collect();
        block.push_str(&format!(",[{}]", eager.join(",")));
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
