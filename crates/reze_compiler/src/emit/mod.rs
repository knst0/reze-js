mod component;
mod flow;
mod script;
mod template;

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;

use oxc_allocator::Allocator;
use oxc_span::Span;

use crate::RUNTIME_MODULE;
use crate::code::Code;
use crate::html::push_js_string;
use crate::ir::{Child, Embed, ExprChild, Getter, HoleKind, Jsx, Namespace, TextPart, Value};
use crate::namer::Namer;

macro_rules! helpers {
    ($($variant:ident => $export:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Helper {
            $($variant),*
        }

        const HELPER_COUNT: usize = [$(Helper::$variant),*].len();

        impl Helper {
            fn export(self) -> &'static str {
                match self {
                    $(Helper::$variant => $export),*
                }
            }

            fn alias_base(self) -> &'static str {
                match self {
                    $(Helper::$variant => concat!("_$", $export)),*
                }
            }
        }
    };
}

helpers! {
    Template => "template",
    TemplateSvg => "templateSVG",
    TemplateMathMl => "templateMathML",
    Insert => "insert",
    RenderEffect => "renderEffect",
    CreateComponent => "createComponent",
    MergeProps => "mergeProps",
    SplitProps => "splitProps",
    Spread => "spread",
    Use => "use",
    AddEventListener => "addEventListener",
    DelegateEvents => "delegateEvents",
    SetAttribute => "setAttribute",
    SetAttributeNs => "setAttributeNS",
    SetBoolAttribute => "setBoolAttribute",
    ClassName => "className",
    ToggleClass => "toggleClass",
    Style => "style",
    Branch => "branch",
    Computed => "computed",
    Choose => "choose",
    List => "list",
    Selector => "selector",
    HotComponent => "hotComponent",
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
    filename: &'a str,
    namer: Namer<'s>,
    aliases: [Option<&'a str>; HELPER_COUNT],
    helper_order: std::vec::Vec<Helper>,
    templates: std::vec::Vec<TemplateDecl<'a>>,
    template_names: HashMap<(&'a str, Namespace), &'a str>,
    events: BTreeSet<&'a str>,
}

impl<'a, 's> Emitter<'a, 's> {
    pub fn new(alloc: &'a Allocator, source: &'a str, filename: &'a str, namer: Namer<'s>) -> Self {
        Self {
            alloc,
            source,
            filename,
            namer,
            aliases: [None; HELPER_COUNT],
            helper_order: std::vec::Vec::new(),
            templates: std::vec::Vec::new(),
            template_names: HashMap::new(),
            events: BTreeSet::new(),
        }
    }

    /// The compiled `head`, the runtime header, the compiled `body` and the event delegation.
    pub fn module(mut self, head: &Embed<'a>, body: &Embed<'a>) -> Code {
        let mut code = Code::default();
        self.embed(&mut code, head);
        let mut compiled = Code::default();
        self.embed(&mut compiled, body);
        let delegate = (!self.events.is_empty()).then(|| self.helper(Helper::DelegateEvents));
        let header = self.header();
        if !header.is_empty() {
            if head.span.end == 0 {
                code.push(&header);
                code.push("\n");
            } else {
                code.push("\n");
                code.push(&header);
            }
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

    /// Runtime imports and template declarations; empty when there are none.
    fn header(&self) -> String {
        let mut out = String::new();
        if !self.helper_order.is_empty() {
            out.push_str("import { ");
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
            push_js_string(&mut out, RUNTIME_MODULE);
            out.push(';');
        }
        for (i, template) in self.templates.iter().enumerate() {
            if i == 0 {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("const ");
            } else {
                out.push_str(",\n  ");
            }
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

    fn fresh(&mut self, base: &str) -> &'a str {
        let name = self.namer.fresh(base);
        self.alloc.alloc_str(&name)
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

    fn src(&self, out: &mut Code, span: Span) {
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
                HoleKind::Script(edit) => self.script(out, hole.span, edit),
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
                [child] => self.child(out, child),
                children => self.child_array(out, children),
            },
            Jsx::Flow(flow) => self.flow(out, flow),
        }
    }

    fn child_array(&mut self, out: &mut Code, children: &[Child<'a>]) {
        out.push("[");
        for (i, child) in children.iter().enumerate() {
            if i > 0 {
                out.push(", ");
            }
            self.child(out, child);
        }
        out.push("]");
    }

    fn child(&mut self, out: &mut Code, child: &Child<'a>) {
        match child {
            Child::Text(text) => push_js_string(&mut out.text, text),
            Child::Jsx(jsx) => self.jsx(out, jsx),
            Child::Expr(ExprChild::Static(embed)) => self.embed(out, embed),
            Child::Expr(ExprChild::Getter(getter)) => self.getter(out, getter),
            Child::Expr(ExprChild::Conditional(_)) => {
                unreachable!("a conditional is only an insert value")
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
            Value::Truthy(embed) => {
                out.push("!!(");
                self.embed(out, embed);
                out.push(")");
            }
            Value::Text(parts) => {
                let starts_static = matches!(parts.first(), Some(TextPart::Static(_)));
                if !starts_static {
                    out.push("\"\"");
                }
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 || !starts_static {
                        out.push(" + ");
                    }
                    match part {
                        TextPart::Static(text) => push_js_string(&mut out.text, text),
                        TextPart::Dynamic(value) => {
                            out.push("(");
                            self.embed(out, value);
                            out.push(")");
                        }
                    }
                }
            }
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
