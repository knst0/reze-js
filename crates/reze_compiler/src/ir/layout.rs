use std::borrow::Cow;
use std::cell::{Ref, RefCell};

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, LocalName, Namespace, ParseOpts, QualName, parse_fragment};
use oxc_span::Span;

use crate::diagnostic::{Code, Report};
use crate::html::{escape_text, is_void, push_attribute_value, trim_trailing_end_tags};

use super::view::{
    Anchor, ElementView, InsertOp, Namespace as ViewNs, StaticNode, StaticNodeKind, StaticTree,
};

const HTML_NS: &str = "http://www.w3.org/1999/xhtml";
const SVG_NS: &str = "http://www.w3.org/2000/svg";
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

const TABLE_TOPOLOGY: [&str; 10] =
    ["table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "colgroup", "col"];
const RAWTEXT_TOPOLOGY: [&str; 9] =
    ["script", "style", "textarea", "title", "iframe", "noembed", "noframes", "xmp", "plaintext"];
const FOREIGN_TOPOLOGY: [&str; 5] = ["svg", "math", "foreignobject", "annotation-xml", "image"];
const SAME_NEST_TOPOLOGY: [&str; 29] = [
    "a", "button", "form", "li", "dt", "dd", "p", "option", "optgroup", "select", "h1", "h2", "h3",
    "h4", "h5", "h6", "nobr", "b", "big", "code", "em", "font", "i", "s", "small", "strike",
    "strong", "tt", "u",
];
const P_CLOSERS: [&str; 30] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "header",
    "hgroup",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "section",
    "summary",
    "ul",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
];
const RAWTEXT_TAGS: [&str; 7] =
    ["script", "style", "iframe", "noembed", "noframes", "xmp", "plaintext"];
const RCDATA_TAGS: [&str; 2] = ["textarea", "title"];
const LF_STRIP_TAGS: [&str; 3] = ["pre", "listing", "textarea"];
const COL_SECTIONS: [&str; 4] = ["thead", "tbody", "tfoot", "tr"];
const ROW_SECTIONS: [&str; 3] = ["thead", "tbody", "tfoot"];

fn lowercase_tag(tag: &str) -> Cow<'_, str> {
    if tag.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(tag.to_ascii_lowercase())
    } else {
        Cow::Borrowed(tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStep {
    Index(u32),
    Content,
}

pub fn is_content_host(node: &StaticNode) -> bool {
    node.kind == StaticNodeKind::Element && node.tag == "template" && node.ns == ViewNs::Html
}

pub fn path_steps(tree: &StaticTree, ancestor: u32, node: u32) -> Vec<PathStep> {
    let mut current = node;
    let mut reversed = Vec::new();
    while current != ancestor {
        let parent = tree.nodes[current as usize].parent.expect("ancestor above every node");
        let index = tree.nodes[parent as usize]
            .children
            .iter()
            .position(|&child| child == current)
            .expect("static children contain every node") as u32;
        reversed.push(PathStep::Index(index));
        if is_content_host(&tree.nodes[parent as usize]) {
            reversed.push(PathStep::Content);
        }
        current = parent;
    }
    reversed.reverse();
    reversed
}

pub(crate) fn serialize_static(tree: &StaticTree) -> String {
    let mut html = String::new();
    push_static(&mut html, tree, 0, false, false, false);
    trim_trailing_end_tags(&mut html);
    html
}

pub(crate) fn needs_text_nodes(tree: &StaticTree, node: &StaticNode) -> bool {
    (is_rawtext_element(node) || is_rcdata_element(node))
        && (node.children.len() > 1
            || node.children.iter().any(|&id| {
                let child = &tree.nodes[id as usize];
                child.kind == StaticNodeKind::Marker
                    || (child.kind == StaticNodeKind::Text && child.text.is_empty())
            }))
}

pub(crate) fn serialize_client_static(tree: &StaticTree) -> String {
    let mut html = String::new();
    push_static(&mut html, tree, 0, false, false, true);
    trim_trailing_end_tags(&mut html);
    html
}

fn push_static(
    out: &mut String,
    tree: &StaticTree,
    id: u32,
    rawtext: bool,
    rcdata: bool,
    text_nodes: bool,
) {
    let node = &tree.nodes[id as usize];
    match node.kind {
        StaticNodeKind::Element => {
            out.push('<');
            out.push_str(&node.tag);
            for attr in &node.attrs {
                out.push(' ');
                out.push_str(&attr.name);
                if let Some(value) = &attr.value {
                    push_attribute_value(out, value);
                }
            }
            out.push('>');
            if node.ns != ViewNs::Html || !is_void(&node.tag) {
                let child_rawtext = is_rawtext_element(node);
                let child_rcdata = is_rcdata_element(node);
                let reconstruct = text_nodes && needs_text_nodes(tree, node);
                if !reconstruct
                    && node.ns == ViewNs::Html
                    && LF_STRIP_TAGS.contains(&node.tag.as_str())
                    && node
                        .children
                        .iter()
                        .map(|&child| &tree.nodes[child as usize])
                        .find(|child| !child_rcdata || child.kind != StaticNodeKind::Marker)
                        .is_some_and(|child| {
                            child.kind == StaticNodeKind::Text && child.text.starts_with('\n')
                        })
                {
                    out.push('\n');
                }
                if !reconstruct {
                    for &child in &node.children {
                        push_static(out, tree, child, child_rawtext, child_rcdata, text_nodes);
                    }
                }
                out.push_str("</");
                out.push_str(&node.tag);
                out.push('>');
            }
        }
        StaticNodeKind::Text => {
            if rawtext {
                out.push_str(&node.text);
            } else {
                escape_text(out, &node.text);
            }
        }
        StaticNodeKind::Marker => {
            if !rawtext && !rcdata {
                out.push_str("<!>");
            }
        }
    }
}

pub fn normalize(view: &mut ElementView, origin: Span, reports: &mut Vec<Report>) {
    if view.statics.nodes.is_empty() {
        return;
    }
    if let Some((detail, hint)) = rawtext_error(&view.statics) {
        reports.push(
            Report::new(Code::ParserRelocation, origin).arg("detail", detail).arg("hint", hint),
        );
        return;
    }
    if let Some((detail, hint)) = col_error(&view.statics) {
        reports.push(
            Report::new(Code::ParserRelocation, origin).arg("detail", detail).arg("hint", hint),
        );
        return;
    }
    if !needs_parse(&view.statics) {
        return;
    }
    let html = serialize_static(&view.statics);
    let parsed = parse_template_html(&html, &view.statics.nodes[0]);
    let failed = {
        let mut matcher = Matcher { tree: &mut view.statics, reports, origin, failed: false };
        matcher.match_roots(&parsed);
        matcher.failed
    };
    if !failed {
        retarget_anchors(&mut view.statics, &mut view.inserts);
    }
}

fn rawtext_error(tree: &StaticTree) -> Option<(String, String)> {
    for node in &tree.nodes {
        if node.kind != StaticNodeKind::Element || node.ns != ViewNs::Html {
            continue;
        }
        let tag = lowercase_tag(&node.tag);
        if tag == "plaintext" {
            let opaque = node
                .children
                .iter()
                .any(|&child| tree.nodes[child as usize].kind != StaticNodeKind::Marker);
            if opaque {
                return Some((
                    "renders content inside `<plaintext>`, which swallows the rest of the template as text".to_string(),
                    "render the text in a normal element instead".to_string(),
                ));
            }
        } else if RAWTEXT_TAGS.contains(&tag.as_ref()) {
            for &child in &node.children {
                let child = &tree.nodes[child as usize];
                if child.kind == StaticNodeKind::Text && closing_delimiter(&child.text, &tag) {
                    return Some((
                        format!("has text that closes `<{tag}>` early"),
                        "use a non-raw-text element or move the content into an external resource"
                            .to_string(),
                    ));
                }
            }
        }
    }
    None
}

fn closing_delimiter(text: &str, tag: &str) -> bool {
    let bytes = text.as_bytes();
    let tag = tag.as_bytes();
    let mut i = 0;
    while i + 2 + tag.len() <= bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2..i + 2 + tag.len()].eq_ignore_ascii_case(tag)
        {
            let after = i + 2 + tag.len();
            if after == bytes.len()
                || matches!(bytes[after], b'\t' | b'\n' | b'\x0C' | b' ' | b'/' | b'>')
            {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn col_error(tree: &StaticTree) -> Option<(String, String)> {
    for node in &tree.nodes {
        if node.kind != StaticNodeKind::Element
            || node.ns != ViewNs::Html
            || !node.tag.eq_ignore_ascii_case("col")
        {
            continue;
        }
        if let Some(parent) = node.parent {
            let parent = &tree.nodes[parent as usize];
            if parent.kind == StaticNodeKind::Element
                && parent.ns == ViewNs::Html
                && COL_SECTIONS.contains(&lowercase_tag(&parent.tag).as_ref())
            {
                return Some((
                    format!(
                        "places `<col>` directly inside `<{}>`, where the parser moves it out",
                        parent.tag.to_ascii_lowercase()
                    ),
                    "wrap `<col>` in an explicit `<colgroup>`".to_string(),
                ));
            }
        }
    }
    None
}

fn retarget_anchors(tree: &mut StaticTree, inserts: &mut [InsertOp]) {
    for insert in inserts {
        if let Anchor::Before(node) = insert.anchor {
            let parent = tree.nodes[node as usize].parent;
            if parent != Some(insert.parent) {
                if let Some(parent) = parent {
                    tree.nodes[parent as usize].referenced = true;
                    insert.parent = parent;
                }
            }
        }
    }
}

fn needs_parse(tree: &StaticTree) -> bool {
    let mut ancestors: Vec<&str> = Vec::new();
    scan_node(tree, 0, &mut ancestors)
}

fn scan_node<'t>(tree: &'t StaticTree, id: u32, ancestors: &mut Vec<&'t str>) -> bool {
    let node = &tree.nodes[id as usize];
    let lower = lowercase_tag(&node.tag);
    let tag = lower.as_ref();
    if TABLE_TOPOLOGY.contains(&tag)
        || RAWTEXT_TOPOLOGY.contains(&tag)
        || FOREIGN_TOPOLOGY.contains(&tag)
        || tag == "pre"
        || tag == "listing"
    {
        return true;
    }
    if tag == "p" {
        for &child in &node.children {
            let child = &tree.nodes[child as usize];
            if child.kind == StaticNodeKind::Element
                && P_CLOSERS.contains(&lowercase_tag(&child.tag).as_ref())
            {
                return true;
            }
        }
    }
    if tag == "option" {
        for &child in &node.children {
            let child = &tree.nodes[child as usize];
            if child.kind == StaticNodeKind::Element
                && matches!(lowercase_tag(&child.tag).as_ref(), "option" | "optgroup")
            {
                return true;
            }
        }
    }
    if SAME_NEST_TOPOLOGY.contains(&tag) && ancestors.contains(&tag) {
        return true;
    }
    ancestors.push(&node.tag);
    let hit = node.children.iter().any(|&child| scan_node(tree, child, ancestors));
    ancestors.pop();
    hit
}

enum PKind {
    Document,
    Fragment,
    Element { name: QualName, contents: Option<usize>, mathml_integration_point: bool },
    Text(String),
    Comment,
}

struct PNode {
    parent: Option<usize>,
    children: Vec<usize>,
    kind: PKind,
}

struct Parsed {
    nodes: Vec<PNode>,
}

struct LayoutSink {
    nodes: RefCell<Vec<PNode>>,
}

impl LayoutSink {
    fn push_child(nodes: &mut Vec<PNode>, parent: usize, child: usize) {
        nodes[child].parent = Some(parent);
        nodes[parent].children.push(child);
    }

    fn push_child_at(nodes: &mut Vec<PNode>, parent: usize, at: usize, id: usize) {
        nodes[id].parent = Some(parent);
        nodes[parent].children.insert(at, id);
    }

    fn push_text(nodes: &mut Vec<PNode>, parent: usize, text: &str) {
        if let Some(&last) = nodes[parent].children.last()
            && let PKind::Text(existing) = &mut nodes[last].kind
        {
            existing.push_str(text);
            return;
        }
        let id = nodes.len();
        nodes.push(PNode {
            parent: Some(parent),
            children: Vec::new(),
            kind: PKind::Text(text.to_string()),
        });
        nodes[parent].children.push(id);
    }

    fn detach(nodes: &mut Vec<PNode>, id: usize) {
        if let Some(parent) = nodes[id].parent.take() {
            nodes[parent].children.retain(|&child| child != id);
        }
    }

    fn is_html_template(name: &QualName) -> bool {
        name.ns.as_str() == HTML_NS && name.local.as_str() == "template"
    }
}

impl TreeSink for LayoutSink {
    type Handle = usize;
    type Output = Parsed;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Parsed {
        Parsed { nodes: self.nodes.into_inner() }
    }

    fn parse_error(&self, _msg: std::borrow::Cow<'static, str>) {}

    fn get_document(&self) -> usize {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a usize) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |nodes| match &nodes[*target].kind {
            PKind::Element { name, .. } => name,
            _ => panic!("element name of a non-element node"),
        })
    }

    fn create_element(&self, name: QualName, _attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len();
        let template = Self::is_html_template(&name);
        nodes.push(PNode {
            parent: None,
            children: Vec::new(),
            kind: PKind::Element {
                name,
                contents: None,
                mathml_integration_point: flags.mathml_annotation_xml_integration_point,
            },
        });
        if template {
            let contents = nodes.len();
            nodes.push(PNode { parent: Some(id), children: Vec::new(), kind: PKind::Fragment });
            if let PKind::Element { contents: slot, .. } = &mut nodes[id].kind {
                *slot = Some(contents);
            }
        }
        id
    }

    fn create_comment(&self, _text: StrTendril) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len();
        nodes.push(PNode { parent: None, children: Vec::new(), kind: PKind::Comment });
        id
    }

    fn create_pi(&self, _target: StrTendril, data: StrTendril) -> usize {
        self.create_comment(data)
    }

    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        let mut nodes = self.nodes.borrow_mut();
        match child {
            NodeOrText::AppendNode(id) => Self::push_child(&mut nodes, *parent, id),
            NodeOrText::AppendText(text) => Self::push_text(&mut nodes, *parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &usize,
        prev_element: &usize,
        child: NodeOrText<usize>,
    ) {
        let has_parent = self.nodes.borrow()[*element].parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }

    fn get_template_contents(&self, target: &usize) -> usize {
        match &self.nodes.borrow()[*target].kind {
            PKind::Element { contents: Some(contents), .. } => *contents,
            _ => panic!("template contents of a non-template element"),
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &usize) -> bool {
        matches!(
            self.nodes.borrow()[*handle].kind,
            PKind::Element { mathml_integration_point: true, .. }
        )
    }

    fn same_node(&self, x: &usize, y: &usize) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &usize, child: NodeOrText<usize>) {
        let mut nodes = self.nodes.borrow_mut();
        let parent = nodes[*sibling].parent.expect("sibling below the fragment root");
        let at = nodes[parent]
            .children
            .iter()
            .position(|&child| child == *sibling)
            .expect("sibling among its parent children");
        match child {
            NodeOrText::AppendNode(id) => {
                Self::detach(&mut nodes, id);
                Self::push_child_at(&mut nodes, parent, at, id);
            }
            NodeOrText::AppendText(text) => {
                if at > 0 {
                    let prev = nodes[parent].children[at - 1];
                    if let PKind::Text(existing) = &mut nodes[prev].kind {
                        existing.push_str(&text);
                        return;
                    }
                }
                let id = nodes.len();
                nodes.push(PNode {
                    parent: Some(parent),
                    children: Vec::new(),
                    kind: PKind::Text(text.to_string()),
                });
                nodes[parent].children.insert(at, id);
            }
        }
    }

    fn add_attrs_if_missing(&self, _target: &usize, _attrs: Vec<Attribute>) {}

    fn remove_from_parent(&self, target: &usize) {
        Self::detach(&mut self.nodes.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &usize, new_parent: &usize) {
        let mut nodes = self.nodes.borrow_mut();
        let moved = std::mem::take(&mut nodes[*node].children);
        for child in moved {
            nodes[child].parent = Some(*new_parent);
            nodes[*new_parent].children.push(child);
        }
    }
}

fn template_wrapper(root: &StaticNode) -> Option<&'static str> {
    match (root.ns, root.tag.as_str()) {
        (ViewNs::Html, _) | (ViewNs::Svg, "svg") | (ViewNs::MathMl, "math") => None,
        (ViewNs::Svg, _) => Some("<svg>"),
        (ViewNs::MathMl, _) => Some("<math>"),
    }
}

fn parse_template_html(source: &str, root: &StaticNode) -> Parsed {
    let sink = LayoutSink {
        nodes: RefCell::new(vec![PNode {
            parent: None,
            children: Vec::new(),
            kind: PKind::Document,
        }]),
    };
    let context = QualName::new(None, Namespace::from(HTML_NS), LocalName::from("template"));
    let mut input = StrTendril::new();
    if let Some(wrapper) = template_wrapper(root) {
        input.push_slice(wrapper);
    }
    input.push_slice(source);
    parse_fragment(sink, ParseOpts::default(), context, Vec::new(), false).one(input)
}

fn view_ns(ns: &Namespace) -> Option<ViewNs> {
    match ns.as_str() {
        HTML_NS => Some(ViewNs::Html),
        SVG_NS => Some(ViewNs::Svg),
        MATHML_NS => Some(ViewNs::MathMl),
        _ => None,
    }
}

fn expected_ns(tree: &StaticTree, sid: u32) -> ViewNs {
    tree.nodes[sid as usize].ns
}

fn is_rawtext_element(node: &StaticNode) -> bool {
    node.kind == StaticNodeKind::Element
        && node.ns == ViewNs::Html
        && RAWTEXT_TAGS.contains(&lowercase_tag(&node.tag).as_ref())
}

fn is_rcdata_element(node: &StaticNode) -> bool {
    node.kind == StaticNodeKind::Element
        && node.ns == ViewNs::Html
        && RCDATA_TAGS.contains(&lowercase_tag(&node.tag).as_ref())
}

fn parsed_text_equal(static_text: &str, parsed: &str, parent: &StaticNode) -> bool {
    if is_rawtext_element(parent) {
        return parsed == static_text;
    }
    let normalized;
    let mut current = static_text;
    if current.contains('\r') {
        normalized = current.replace("\r\n", "\n").replace('\r', "\n");
        current = &normalized;
    }
    current == parsed
}

#[derive(Clone, Copy)]
enum Implied {
    Tbody,
    Row,
    Colgroup,
}

struct Matcher<'t, 'r> {
    tree: &'t mut StaticTree,
    reports: &'r mut Vec<Report>,
    origin: Span,
    failed: bool,
}

impl Matcher<'_, '_> {
    fn fail(&mut self, detail: String, hint: String) {
        if !self.failed {
            self.failed = true;
            self.reports.push(
                Report::new(Code::ParserRelocation, self.origin)
                    .arg("detail", detail)
                    .arg("hint", hint),
            );
        }
    }

    fn match_roots(&mut self, parsed: &Parsed) {
        let fragment = parsed.nodes[0].children[0];
        let mut roots = &parsed.nodes[fragment].children;
        if template_wrapper(&self.tree.nodes[0]).is_some() && roots.len() == 1 {
            roots = &parsed.nodes[roots[0]].children;
        }
        if roots.len() != 1 {
            self.fail(
                "drops the root element or hoists content around it".to_string(),
                "use a root element the parser keeps as written (for example, table rows need an explicit `<table>` ancestor in the same template)".to_string(),
            );
            return;
        }
        let root = roots[0];
        self.match_element(parsed, 0, root);
    }

    fn match_element(&mut self, parsed: &Parsed, sid: u32, pid: usize) {
        if self.failed {
            return;
        }
        let (tag, raw, ns) = match &parsed.nodes[pid].kind {
            PKind::Element { name, .. } => {
                (name.local.as_str().to_ascii_lowercase(), name.local.as_str(), view_ns(&name.ns))
            }
            _ => {
                self.fail(
                    "replaces an element with text or a comment".to_string(),
                    "keep elements where the parser keeps elements".to_string(),
                );
                return;
            }
        };
        let expected = expected_ns(self.tree, sid);
        let (stag, s_kids, is_template) = {
            let node = &mut self.tree.nodes[sid as usize];
            if node.tag == "image" && node.ns == ViewNs::Html {
                node.tag = "img".to_string();
            }
            (
                node.tag.to_ascii_lowercase(),
                node.children.clone(),
                node.tag == "template" && node.ns == ViewNs::Html,
            )
        };
        let tags_equal = if expected == ViewNs::Html {
            tag == stag
        } else {
            raw == self.tree.nodes[sid as usize].tag.as_str()
        };
        if !tags_equal {
            self.fail(
                format!("parses `<{stag}>` as `<{tag}>` here"),
                "write the tag the parser produces".to_string(),
            );
            return;
        }
        if Some(expected) != ns {
            self.fail(
                format!("places `<{stag}>` in another namespace"),
                "move the element behind the matching integration point (`foreignObject` for HTML in SVG)".to_string(),
            );
            return;
        }
        let p_kids = match &parsed.nodes[pid].kind {
            PKind::Element { contents: Some(contents), .. } if is_template => {
                parsed.nodes[*contents].children.clone()
            }
            _ => parsed.nodes[pid].children.clone(),
        };
        self.match_kids(parsed, sid, &s_kids, &p_kids);
    }

    fn match_kids(&mut self, parsed: &Parsed, s_parent: u32, s_ids: &[u32], p_ids: &[usize]) {
        let parent = &self.tree.nodes[s_parent as usize];
        if is_rawtext_element(parent) || is_rcdata_element(parent) {
            let mut text = String::new();
            for &id in s_ids {
                let node = &self.tree.nodes[id as usize];
                match node.kind {
                    StaticNodeKind::Text => text.push_str(&node.text),
                    StaticNodeKind::Marker => {}
                    StaticNodeKind::Element => {
                        self.fail(
                            "parses an element inside raw text as text".to_string(),
                            "render scalar text in raw-text and RCDATA elements".to_string(),
                        );
                        return;
                    }
                }
            }
            let matches = match p_ids {
                [] => text.is_empty(),
                [id] => match &parsed.nodes[*id].kind {
                    PKind::Text(parsed_text) => parsed_text_equal(&text, parsed_text, parent),
                    _ => false,
                },
                _ => false,
            };
            if !matches {
                self.fail(
                    "changes the raw-text content".to_string(),
                    "keep raw text the parser preserves without closing its element".to_string(),
                );
            }
            return;
        }
        let mut si = 0;
        let mut pi = 0;
        while si < s_ids.len() && pi < p_ids.len() && !self.failed {
            let sid = s_ids[si];
            let pid = p_ids[pi];
            match self.tree.nodes[sid as usize].kind {
                StaticNodeKind::Element => {
                    let (p_tag, p_ns) = match &parsed.nodes[pid].kind {
                        PKind::Element { name, .. } => {
                            (name.local.as_str().to_ascii_lowercase(), view_ns(&name.ns))
                        }
                        _ => {
                            self.fail(
                                "replaces an element with text or a comment".to_string(),
                                "keep elements where the parser keeps elements".to_string(),
                            );
                            return;
                        }
                    };
                    let (s_tag, s_image) = {
                        let snode = &self.tree.nodes[sid as usize];
                        (
                            snode.tag.to_ascii_lowercase(),
                            snode.tag == "image" && snode.ns == ViewNs::Html,
                        )
                    };
                    let (parent_tag, parent_ns) = {
                        let parent = &self.tree.nodes[s_parent as usize];
                        (parent.tag.to_ascii_lowercase(), parent.ns)
                    };
                    if p_tag == s_tag || (s_image && p_tag == "img") {
                        self.match_element(parsed, sid, pid);
                        si += 1;
                        pi += 1;
                    } else if p_tag == "tbody"
                        && p_ns == Some(ViewNs::Html)
                        && parent_ns == ViewNs::Html
                        && parent_tag == "table"
                    {
                        match self.implied(parsed, s_parent, &s_ids[si..], pid, Implied::Tbody) {
                            Some(consumed) => {
                                si += consumed;
                                pi += 1;
                            }
                            None => {
                                self.fail(
                                    format!(
                                        "restructures `<{s_tag}>` next to an implied `<tbody>`"
                                    ),
                                    "wrap table rows in an explicit `<tbody>`".to_string(),
                                );
                                return;
                            }
                        }
                    } else if p_tag == "tr"
                        && p_ns == Some(ViewNs::Html)
                        && parent_ns == ViewNs::Html
                        && ROW_SECTIONS.contains(&parent_tag.as_str())
                    {
                        match self.implied(parsed, s_parent, &s_ids[si..], pid, Implied::Row) {
                            Some(consumed) => {
                                si += consumed;
                                pi += 1;
                            }
                            None => {
                                self.fail(
                                    format!("restructures `<{s_tag}>` next to an implied `<tr>`"),
                                    "wrap table cells in an explicit `<tr>`".to_string(),
                                );
                                return;
                            }
                        }
                    } else if p_tag == "colgroup"
                        && p_ns == Some(ViewNs::Html)
                        && parent_ns == ViewNs::Html
                        && parent_tag == "table"
                    {
                        match self.implied(parsed, s_parent, &s_ids[si..], pid, Implied::Colgroup) {
                            Some(consumed) => {
                                si += consumed;
                                pi += 1;
                            }
                            None => {
                                self.fail(
                                    format!(
                                        "restructures `<{s_tag}>` next to an implied `<colgroup>`"
                                    ),
                                    "wrap `<col>` elements in an explicit `<colgroup>`".to_string(),
                                );
                                return;
                            }
                        }
                    } else {
                        self.fail(
                            format!("parses `<{s_tag}>` as `<{p_tag}>` here"),
                            "restructure the markup so it parses as written".to_string(),
                        );
                        return;
                    }
                }
                StaticNodeKind::Text => {
                    let parsed_text = match &parsed.nodes[pid].kind {
                        PKind::Text(text) => Some(text.clone()),
                        _ => None,
                    };
                    let Some(parsed_text) = parsed_text else {
                        self.fail(
                            "drops text the template needs".to_string(),
                            "keep text where the parser keeps text".to_string(),
                        );
                        return;
                    };
                    let (ok, text) = {
                        let snode = &self.tree.nodes[sid as usize];
                        let parent = &self.tree.nodes[s_parent as usize];
                        (parsed_text_equal(&snode.text, &parsed_text, parent), snode.text.clone())
                    };
                    if !ok {
                        self.fail(
                            format!("changes text `{text}`"),
                            "keep text the parser keeps as written".to_string(),
                        );
                        return;
                    }
                    si += 1;
                    pi += 1;
                }
                StaticNodeKind::Marker => {
                    let parent = &self.tree.nodes[s_parent as usize];
                    if is_rawtext_element(parent) || is_rcdata_element(parent) {
                        si += 1;
                    } else {
                        match &parsed.nodes[pid].kind {
                            PKind::Comment => {
                                si += 1;
                                pi += 1;
                            }
                            _ => {
                                self.fail(
                                    "drops a marker the template needs".to_string(),
                                    "keep dynamic boundaries where the parser keeps nodes"
                                        .to_string(),
                                );
                                return;
                            }
                        }
                    }
                }
            }
        }
        if self.failed {
            return;
        }
        if si < s_ids.len() {
            let detail = {
                let snode = &self.tree.nodes[s_ids[si] as usize];
                match snode.kind {
                    StaticNodeKind::Element => {
                        format!("drops `<{}>` here", snode.tag.to_ascii_lowercase())
                    }
                    StaticNodeKind::Text => {
                        format!("drops the text `{}` here", snode.text)
                    }
                    StaticNodeKind::Marker => "drops a marker here".to_string(),
                }
            };
            self.fail(detail, "restructure the markup so the parser keeps every node".to_string());
        } else if pi < p_ids.len() {
            let extra = match &parsed.nodes[p_ids[pi]].kind {
                PKind::Element { name, .. } => {
                    format!("inserts `<{}>` here", name.local.as_str().to_ascii_lowercase())
                }
                PKind::Text(_) => "inserts text here".to_string(),
                _ => "inserts a node here".to_string(),
            };
            self.fail(
                format!("{extra} that the template does not have"),
                "restructure the markup so it parses as written".to_string(),
            );
        }
    }

    fn implied(
        &mut self,
        parsed: &Parsed,
        s_parent: u32,
        s_suffix: &[u32],
        pid: usize,
        kind: Implied,
    ) -> Option<usize> {
        let mut run = 0;
        for &sid in s_suffix {
            let snode = &self.tree.nodes[sid as usize];
            if run != 0
                && (snode.kind == StaticNodeKind::Marker
                    || (snode.kind == StaticNodeKind::Text
                        && snode
                            .text
                            .bytes()
                            .all(|byte| matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' '))))
            {
                run += 1;
                continue;
            }
            if snode.kind != StaticNodeKind::Element || snode.ns != ViewNs::Html {
                break;
            }
            let member = match kind {
                Implied::Tbody => {
                    snode.tag.eq_ignore_ascii_case("tr")
                        || snode.tag.eq_ignore_ascii_case("td")
                        || snode.tag.eq_ignore_ascii_case("th")
                }
                Implied::Row => {
                    snode.tag.eq_ignore_ascii_case("td") || snode.tag.eq_ignore_ascii_case("th")
                }
                Implied::Colgroup => snode.tag.eq_ignore_ascii_case("col"),
            };
            if !member {
                break;
            }
            run += 1;
        }
        if run == 0 {
            return None;
        }
        let tag = match kind {
            Implied::Tbody => "tbody",
            Implied::Row => "tr",
            Implied::Colgroup => "colgroup",
        };
        let id = self.tree.nodes.len() as u32;
        self.tree.nodes.push(StaticNode {
            parent: Some(s_parent),
            children: Vec::new(),
            kind: StaticNodeKind::Element,
            referenced: false,
            ns: ViewNs::Html,
            tag: tag.to_string(),
            text: String::new(),
            attrs: Vec::new(),
        });
        let mut effective = Vec::with_capacity(run);
        if matches!(kind, Implied::Tbody) {
            let mut group = Vec::new();
            for &sid in &s_suffix[..run] {
                let snode = &self.tree.nodes[sid as usize];
                let cell =
                    snode.tag.eq_ignore_ascii_case("td") || snode.tag.eq_ignore_ascii_case("th");
                if cell || (!group.is_empty() && snode.kind != StaticNodeKind::Element) {
                    group.push(sid);
                } else {
                    if !group.is_empty() {
                        effective.push(self.wrap_nodes(id, "tr", std::mem::take(&mut group)));
                    }
                    effective.push(sid);
                }
            }
            if !group.is_empty() {
                effective.push(self.wrap_nodes(id, "tr", group));
            }
            for &sid in &effective {
                self.tree.nodes[sid as usize].parent = Some(id);
            }
        } else {
            for &sid in &s_suffix[..run] {
                self.tree.nodes[sid as usize].parent = Some(id);
                effective.push(sid);
            }
        }
        self.tree.nodes[id as usize].children = effective.clone();
        let p_kids = parsed.nodes[pid].children.clone();
        self.match_kids(parsed, id, &effective, &p_kids);
        if self.failed {
            return None;
        }
        let siblings = &mut self.tree.nodes[s_parent as usize].children;
        let at = siblings
            .iter()
            .position(|&child| child == s_suffix[0])
            .expect("implicit rows stay among their siblings");
        siblings.splice(at..at + run, [id]);
        Some(run)
    }

    fn wrap_nodes(&mut self, parent: u32, tag: &str, members: Vec<u32>) -> u32 {
        let id = self.tree.nodes.len() as u32;
        for &sid in &members {
            self.tree.nodes[sid as usize].parent = Some(id);
        }
        self.tree.nodes.push(StaticNode {
            parent: Some(parent),
            children: members,
            kind: StaticNodeKind::Element,
            referenced: false,
            ns: ViewNs::Html,
            tag: tag.to_string(),
            text: String::new(),
            attrs: Vec::new(),
        });
        id
    }
}
