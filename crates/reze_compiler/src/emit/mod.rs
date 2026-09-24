//! IR → code for one `Target` (SPEC §5, §14). Emission takes no decisions: every choice is in
//! the IR; the target only picks how a template is written out.

mod async_component;
mod component;
mod island;
mod props;
mod server;
mod store;
mod template;

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;

use oxc_allocator::Allocator;

use crate::Target;
use crate::code::Code;
use crate::html::push_js_string;
use crate::ir::{Child, Embed, ExprChild, Getter, HoleKind, Jsx, Namespace, Value};
use crate::namer::Namer;

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
    SplitProps,
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
    Untrack,
    OnCleanup,
    Effect,
    TrackAsync,
    TrackPending,
    Claim,
    ClaimChild,
    ClaimSibling,
    ClaimInsert,
    Ssr,
    SsrChild,
    SsrHydrationKey,
    SsrAttribute,
    SsrBoolAttribute,
    SsrClass,
    SsrStyle,
    SsrSpread,
    SsrRaw,
    SsrIsland,
    HydrateIslands,
}

const HELPER_COUNT: usize = Helper::HydrateIslands as usize + 1;

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
            Helper::SplitProps => "splitProps",
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
            Helper::Untrack => "untrack",
            Helper::OnCleanup => "onCleanup",
            Helper::Effect => "effect",
            Helper::TrackAsync => "trackAsync",
            Helper::TrackPending => "trackPending",
            Helper::Claim => "claim",
            Helper::ClaimChild => "claimChild",
            Helper::ClaimSibling => "claimSibling",
            Helper::ClaimInsert => "claimInsert",
            Helper::Ssr => "ssr",
            Helper::SsrChild => "ssrChild",
            Helper::SsrHydrationKey => "ssrHydrationKey",
            Helper::SsrAttribute => "ssrAttribute",
            Helper::SsrBoolAttribute => "ssrBoolAttribute",
            Helper::SsrClass => "ssrClass",
            Helper::SsrStyle => "ssrStyle",
            Helper::SsrSpread => "ssrSpread",
            Helper::SsrRaw => "ssrRaw",
            Helper::SsrIsland => "ssrIsland",
            Helper::HydrateIslands => "hydrateIslands",
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
            Helper::SplitProps => "_$splitProps",
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
            Helper::Untrack => "_$untrack",
            Helper::OnCleanup => "_$onCleanup",
            Helper::Effect => "_$effect",
            Helper::TrackAsync => "_$trackAsync",
            Helper::TrackPending => "_$trackPending",
            Helper::Claim => "_$claim",
            Helper::ClaimChild => "_$claimChild",
            Helper::ClaimSibling => "_$claimSibling",
            Helper::ClaimInsert => "_$claimInsert",
            Helper::Ssr => "_$ssr",
            Helper::SsrChild => "_$ssrChild",
            Helper::SsrHydrationKey => "_$ssrHydrationKey",
            Helper::SsrAttribute => "_$ssrAttribute",
            Helper::SsrBoolAttribute => "_$ssrBoolAttribute",
            Helper::SsrClass => "_$ssrClass",
            Helper::SsrStyle => "_$ssrStyle",
            Helper::SsrSpread => "_$ssrSpread",
            Helper::SsrRaw => "_$ssrRaw",
            Helper::SsrIsland => "_$ssrIsland",
            Helper::HydrateIslands => "_$hydrateIslands",
        }
    }
}

enum TemplateDecl<'a> {
    /// Client and hydrate: `factory(html)`.
    Factory { name: &'a str, factory: &'a str, html: &'a str, namespace: Namespace },
    /// Server: the static strings between the dynamic parts.
    Strings { name: &'a str, strings: std::vec::Vec<&'a str> },
}

pub struct Emitter<'a, 's> {
    alloc: &'a Allocator,
    source: &'a str,
    module_name: &'a str,
    is_typescript: bool,
    target: Target,
    namer: Namer<'s>,
    aliases: [Option<&'a str>; HELPER_COUNT],
    helper_order: std::vec::Vec<Helper>,
    templates: std::vec::Vec<TemplateDecl<'a>>,
    template_names: HashMap<(&'a str, Namespace), &'a str>,
    string_template_names: HashMap<std::vec::Vec<&'a str>, &'a str>,
    events: BTreeSet<&'a str>,
    /// `import { export as alias } from specifier` of island components, in first-use order.
    island_imports: std::vec::Vec<(&'a str, &'a str, &'a str)>,
}

impl<'a, 's> Emitter<'a, 's> {
    pub fn new(
        alloc: &'a Allocator,
        source: &'a str,
        module_name: &'a str,
        is_typescript: bool,
        target: Target,
        namer: Namer<'s>,
    ) -> Self {
        Self {
            alloc,
            source,
            module_name,
            is_typescript,
            target,
            namer,
            aliases: [None; HELPER_COUNT],
            helper_order: std::vec::Vec::new(),
            templates: std::vec::Vec::new(),
            template_names: HashMap::new(),
            string_template_names: HashMap::new(),
            events: BTreeSet::new(),
            island_imports: std::vec::Vec::new(),
        }
    }

    /// The whole module: the compiled `head` (hashbang, directives, leading imports), the runtime
    /// header, the compiled `body`, and the event delegation trailer.
    pub fn module(mut self, head: &Embed<'a>, body: &Embed<'a>) -> Code {
        let mut code = Code::default();
        self.embed(&mut code, head);
        let header_at = head.span.end;
        let mut compiled = Code::default();
        self.embed(&mut compiled, body);
        let delegate = (!self.events.is_empty()).then(|| self.helper(Helper::DelegateEvents));
        let header = self.header();

        if !header.is_empty() {
            if header_at == 0 {
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

    /// The runtime exports compiling `embed` imports, in first-use order (SPEC §15.11).
    pub fn runtime_exports(mut self, embed: &Embed<'a>) -> std::vec::Vec<&'static str> {
        let mut scratch = Code::default();
        self.embed(&mut scratch, embed);
        if !self.events.is_empty() {
            self.helper(Helper::DelegateEvents);
        }
        self.helper_order.iter().map(|helper| helper.export()).collect()
    }

    /// Runtime imports and template declarations, one per line; empty when there are none.
    fn header(&mut self) -> String {
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
            push_js_string(&mut out, self.module_name);
            out.push(';');
        }
        for (export, alias, specifier) in &self.island_imports {
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = write!(out, "import {{ {export} as {alias} }} from ");
            push_js_string(&mut out, specifier);
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
            match template {
                TemplateDecl::Factory { name, factory, html, namespace } => {
                    let _ = write!(out, "{name} = /*#__PURE__*/ {factory}(");
                    if *namespace == Namespace::Svg {
                        push_js_string(&mut out, &format!("<svg>{html}</svg>"));
                    } else {
                        push_js_string(&mut out, html);
                    }
                    out.push(')');
                }
                TemplateDecl::Strings { name, strings } => {
                    let _ = write!(out, "{name} = [");
                    for (i, string) in strings.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        push_js_string(&mut out, string);
                    }
                    out.push(']');
                }
            }
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
        self.templates.push(TemplateDecl::Factory { name, factory, html, namespace });
        self.template_names.insert((html, namespace), name);
        name
    }

    fn string_template_name(&mut self, strings: std::vec::Vec<&'a str>) -> &'a str {
        if let Some(name) = self.string_template_names.get(&strings) {
            return name;
        }
        let name = self.fresh("_tmpl$");
        self.string_template_names.insert(strings.clone(), name);
        self.templates.push(TemplateDecl::Strings { name, strings });
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
                HoleKind::ComputedInline { body } => {
                    out.push("(");
                    self.embed(out, body);
                    out.push(")");
                }
                HoleKind::Remove => {}
                HoleKind::IslandRoot { kind, callee, code, element, islands } => {
                    self.island_root(out, *kind, *callee, code, element.as_ref(), islands);
                }
                HoleKind::PropsParam { name } => out.push(name),
                HoleKind::PropsRead { props, path, default, shorthand } => {
                    if *shorthand {
                        self.src(out, hole.span);
                        out.push(": ");
                    }
                    self.props_read(out, props, path, default.as_ref());
                }
                HoleKind::PropsRest { props, binding, keys, body } => {
                    self.props_rest(out, props, *binding, keys, body.as_ref());
                }
                HoleKind::StoreDecl { leaves } => self.store_declaration(out, leaves),
                HoleKind::StoreExport { declaration, specifiers } => {
                    self.embed(out, declaration);
                    if !specifiers.is_empty() {
                        out.push("\nexport { ");
                        self.specifiers(out, specifiers);
                        out.push(" };");
                    }
                }
                HoleKind::StoreRead { getter } => {
                    out.push(getter);
                    out.push("()");
                }
                HoleKind::StoreSet { body } => {
                    let untrack = self.helper(Helper::Untrack);
                    out.push("void ");
                    out.push(untrack);
                    out.push("(() => ");
                    self.embed(out, body);
                    out.push(")");
                }
                HoleKind::StoreWrite { setter, write } => self.store_write(out, setter, write),
                HoleKind::Specifiers { specifiers } => self.specifiers(out, specifiers),
            }
            position = hole.span.end;
        }
        out.src(self.source, position, embed.span.end);
    }

    fn jsx(&mut self, out: &mut Code, jsx: &Jsx<'a>) {
        match jsx {
            Jsx::Template(template) => match self.target {
                Target::Client | Target::Hydrate => self.template(out, template),
                Target::Server => self.server_template(out, template),
            },
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
