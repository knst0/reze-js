//! IR → client DOM code (SPEC §5). Emission takes no decisions: every choice is in the IR.

mod async_component;
mod component;
mod template;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write;

use oxc_allocator::Allocator;
use oxc_semantic::Scoping;

use crate::code::Code;
use crate::html::push_js_string;
use crate::ir::{Child, Embed, ExprChild, Getter, HoleKind, Jsx, Namespace, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Helper {
    Template,
    TemplateSvg,
    TemplateMathMl,
    Insert,
    Memo,
    Bind,
    CreateComponent,
    MergeProps,
    Spread,
    Use,
    AddEventListener,
    DelegateEvents,
    SetAttribute,
    SetAttributeNs,
    SetBoolAttribute,
    ClassName,
    Style,
    GetOwner,
    Signal,
    OnCleanup,
    Effect,
    TrackAsync,
    TrackPending,
}

const HELPER_COUNT: usize = Helper::TrackPending as usize + 1;

impl Helper {
    fn export(self) -> &'static str {
        match self {
            Helper::Template => "template",
            Helper::TemplateSvg => "templateSVG",
            Helper::TemplateMathMl => "templateMathML",
            Helper::Insert => "insert",
            Helper::Memo => "memo",
            Helper::Bind => "bind",
            Helper::CreateComponent => "createComponent",
            Helper::MergeProps => "mergeProps",
            Helper::Spread => "spread",
            Helper::Use => "use",
            Helper::AddEventListener => "addEventListener",
            Helper::DelegateEvents => "delegateEvents",
            Helper::SetAttribute => "setAttribute",
            Helper::SetAttributeNs => "setAttributeNS",
            Helper::SetBoolAttribute => "setBoolAttribute",
            Helper::ClassName => "className",
            Helper::Style => "style",
            Helper::GetOwner => "getOwner",
            Helper::Signal => "signal",
            Helper::OnCleanup => "onCleanup",
            Helper::Effect => "effect",
            Helper::TrackAsync => "trackAsync",
            Helper::TrackPending => "trackPending",
        }
    }

    fn alias_base(self) -> &'static str {
        match self {
            Helper::Template => "_$template",
            Helper::TemplateSvg => "_$templateSVG",
            Helper::TemplateMathMl => "_$templateMathML",
            Helper::Insert => "_$insert",
            Helper::Memo => "_$memo",
            Helper::Bind => "_$bind",
            Helper::CreateComponent => "_$createComponent",
            Helper::MergeProps => "_$mergeProps",
            Helper::Spread => "_$spread",
            Helper::Use => "_$use",
            Helper::AddEventListener => "_$addEventListener",
            Helper::DelegateEvents => "_$delegateEvents",
            Helper::SetAttribute => "_$setAttribute",
            Helper::SetAttributeNs => "_$setAttributeNS",
            Helper::SetBoolAttribute => "_$setBoolAttribute",
            Helper::ClassName => "_$className",
            Helper::Style => "_$style",
            Helper::GetOwner => "_$getOwner",
            Helper::Signal => "_$signal",
            Helper::OnCleanup => "_$onCleanup",
            Helper::Effect => "_$effect",
            Helper::TrackAsync => "_$trackAsync",
            Helper::TrackPending => "_$trackPending",
        }
    }
}

/// Fresh identifiers that avoid every name of the source.
struct Namer<'s> {
    reserved: HashSet<&'s str>,
    next_suffix: HashMap<&'static str, u32>,
}

impl<'s> Namer<'s> {
    fn new(scoping: &'s Scoping) -> Self {
        let mut reserved: HashSet<&'s str> = scoping.symbol_names().collect();
        reserved.extend(scoping.root_unresolved_references().keys().map(|name| name.as_str()));
        Self { reserved, next_suffix: HashMap::new() }
    }

    fn fresh(&mut self, base: &'static str, out: &mut String) {
        let suffix = self.next_suffix.entry(base).or_insert(1);
        loop {
            out.clear();
            out.push_str(base);
            if *suffix > 1 {
                let _ = write!(out, "{suffix}");
            }
            *suffix += 1;
            if !self.reserved.contains(out.as_str()) {
                return;
            }
        }
    }
}

struct TemplateDecl<'a> {
    name: &'a str,
    factory: &'a str,
    html: &'a str,
    namespace: Namespace,
}

pub struct Emitter<'a, 's> {
    alloc: &'a Allocator,
    source: &'a str,
    module_name: &'a str,
    is_typescript: bool,
    namer: Namer<'s>,
    aliases: [Option<&'a str>; HELPER_COUNT],
    helper_order: std::vec::Vec<Helper>,
    templates: std::vec::Vec<TemplateDecl<'a>>,
    template_names: HashMap<(&'a str, Namespace), &'a str>,
    events: BTreeSet<&'a str>,
    scratch: String,
}

impl<'a, 's> Emitter<'a, 's> {
    pub fn new(
        alloc: &'a Allocator,
        source: &'a str,
        module_name: &'a str,
        is_typescript: bool,
        scoping: &'s Scoping,
    ) -> Self {
        Self {
            alloc,
            source,
            module_name,
            is_typescript,
            namer: Namer::new(scoping),
            aliases: [None; HELPER_COUNT],
            helper_order: std::vec::Vec::new(),
            templates: std::vec::Vec::new(),
            template_names: HashMap::new(),
            events: BTreeSet::new(),
            scratch: String::new(),
        }
    }

    /// The whole module: `source[..header_at]`, the runtime header, the compiled `body`, and the
    /// event delegation trailer.
    pub fn module(mut self, body: &Embed<'a>, header_at: u32) -> Code {
        let mut compiled = Code::default();
        self.embed(&mut compiled, body);
        let delegate = (!self.events.is_empty()).then(|| self.helper(Helper::DelegateEvents));
        let header = self.header();

        let mut code = Code::default();
        code.src(self.source, 0, header_at);
        if header_at == 0 {
            code.push(&header);
            code.push("\n");
        } else {
            code.push("\n");
            code.push(&header);
        }
        code.append(compiled);
        if let Some(delegate) = delegate {
            code.push("\n");
            code.push(delegate);
            code.push("([");
            for (i, event) in self.events.iter().enumerate() {
                if i > 0 {
                    code.push(", ");
                }
                push_js_string(&mut code.text, event);
            }
            code.push("]);\n");
        }
        code
    }

    fn header(&mut self) -> String {
        let mut out = String::from("import { ");
        for (i, helper) in self.helper_order.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            let _ = write!(
                out,
                "{} as {}",
                helper.export(),
                self.aliases[*helper as usize].unwrap_or_default()
            );
        }
        out.push_str(" } from ");
        push_js_string(&mut out, self.module_name);
        out.push(';');
        for (i, template) in self.templates.iter().enumerate() {
            out.push_str(if i == 0 { "\nconst " } else { ",\n  " });
            let _ = write!(out, "{} = /*#__PURE__*/ {}(", template.name, template.factory);
            if template.namespace == Namespace::Svg {
                push_js_string(&mut out, &format!("<svg>{}</svg>", template.html));
            } else {
                push_js_string(&mut out, template.html);
            }
            out.push(')');
        }
        if !self.templates.is_empty() {
            out.push(';');
        }
        out
    }

    fn fresh(&mut self, base: &'static str) -> &'a str {
        let mut scratch = std::mem::take(&mut self.scratch);
        self.namer.fresh(base, &mut scratch);
        let name = self.alloc.alloc_str(&scratch);
        self.scratch = scratch;
        name
    }

    /// Local alias of a runtime export, imported on first use.
    fn helper(&mut self, helper: Helper) -> &'a str {
        if let Some(alias) = self.aliases[helper as usize] {
            return alias;
        }
        let alias = self.fresh(helper.alias_base());
        self.aliases[helper as usize] = Some(alias);
        self.helper_order.push(helper);
        alias
    }

    fn template_name(&mut self, html: &'a str, namespace: Namespace) -> &'a str {
        if let Some(name) = self.template_names.get(&(html, namespace)) {
            return name;
        }
        let factory = self.helper(match namespace {
            Namespace::Html => Helper::Template,
            Namespace::Svg => Helper::TemplateSvg,
            Namespace::MathMl => Helper::TemplateMathMl,
        });
        let name = self.fresh("_tmpl$");
        self.templates.push(TemplateDecl { name, factory, html, namespace });
        self.template_names.insert((html, namespace), name);
        name
    }

    fn src(&self, out: &mut Code, span: oxc_span::Span) {
        out.src(self.source, span.start, span.end);
    }

    /// The source slice of `embed` with its holes compiled.
    fn embed(&mut self, out: &mut Code, embed: &Embed<'a>) {
        let mut position = embed.span.start;
        for hole in &embed.holes {
            out.src(self.source, position, hole.span.start);
            out.mark(hole.span.start);
            match &hole.kind {
                HoleKind::Jsx(jsx) => self.jsx(out, jsx),
                HoleKind::AsyncComponent(component) => self.async_component(out, component),
                HoleKind::ConstSignalDecl { getter, init } => {
                    self.src(out, *getter);
                    out.push(" = ");
                    self.embed(out, init);
                }
                HoleKind::ConstSignalRead { getter } => self.src(out, *getter),
            }
            position = hole.span.end;
        }
        out.src(self.source, position, embed.span.end);
    }

    fn jsx(&mut self, out: &mut Code, jsx: &Jsx<'a>) {
        match jsx {
            Jsx::Template(template) => self.template(out, template),
            Jsx::Component(component) => self.component(out, component),
            Jsx::Fragment(children) => match children.as_slice() {
                [] => out.push("[]"),
                [child] => self.child(out, child, &[]),
                children => self.child_array(out, children),
            },
        }
    }

    fn child_array(&mut self, out: &mut Code, children: &[Child<'a>]) {
        out.push("[");
        for (i, child) in children.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            self.child(out, child, &[]);
        }
        out.push("]");
    }

    /// `memos` names the enclosing template's hoisted conditions.
    fn child(&mut self, out: &mut Code, child: &Child<'a>, memos: &[&'a str]) {
        match child {
            Child::Text(text) => push_js_string(&mut out.text, text),
            Child::Jsx(jsx) => self.jsx(out, jsx),
            Child::Expr(ExprChild::Static(embed)) => self.embed(out, embed),
            Child::Expr(ExprChild::Getter(getter)) => self.getter(out, getter),
            Child::Expr(ExprChild::Memo(getter)) => {
                let memo = self.helper(Helper::Memo);
                out.push(memo);
                out.push("(");
                self.getter(out, getter);
                out.push(")");
            }
            Child::Expr(ExprChild::Conditional(conditional)) => {
                out.push("() => ");
                out.push(memos[conditional.memo.0 as usize]);
                match &conditional.alternate {
                    Some(alternate) => {
                        out.push("() ? ");
                        self.embed(out, &conditional.consequent);
                        out.push(" : ");
                        self.embed(out, alternate);
                    }
                    None => {
                        out.push("() && ");
                        self.embed(out, &conditional.consequent);
                    }
                }
            }
        }
    }

    fn getter(&mut self, out: &mut Code, getter: &Getter<'a>) {
        match getter {
            Getter::Call(callee) => self.src(out, *callee),
            Getter::Thunk { body, parenthesize } => {
                out.push(if *parenthesize { "() => (" } else { "() => " });
                self.embed(out, body);
                if *parenthesize {
                    out.push(")");
                }
            }
        }
    }

    fn value(&mut self, out: &mut Code, value: &Value<'a>) {
        match value {
            Value::True => out.push("true"),
            Value::Str(s) => push_js_string(&mut out.text, s),
            Value::Expr(embed) => self.embed(out, embed),
            Value::Jsx(jsx) => self.jsx(out, jsx),
            Value::ClassParts(parts) => {
                out.push("[");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push(", ");
                    }
                    self.value(out, part);
                }
                out.push("]");
            }
        }
    }
}
