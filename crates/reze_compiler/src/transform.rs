//! JSX → DOM code for the client target: one `template()` per static HTML shape, node paths as
//! `firstChild`/`nextSibling` chains, and runtime calls from `@rezejs/dom` for everything dynamic.
//!
//! The transform splices generated code into the original source text, so everything outside
//! JSX (including TypeScript syntax) is kept verbatim.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::code::Code;
use crate::html::{
    attribute_namespace, clean_jsx_text, decode_entities, escape_attribute, escape_text,
    is_delegated_event, is_property, is_svg_element, is_void, js_string, member, property_key,
};

pub struct Transformer<'s> {
    source: &'s str,
    module_name: &'s str,
    /// Identifiers written in the source; generated names avoid them.
    taken: HashSet<String>,
    next_suffix: HashMap<String, u32>,
    /// Runtime imports in first-use order: (export, local alias).
    helpers: Vec<(&'static str, String)>,
    /// (variable, `template*` factory, factory arguments), deduplicated by HTML and namespace.
    templates: Vec<(String, &'static str, String)>,
    /// Template variables by HTML, then namespace slot (`svg + 2*math`): the hot
    /// lookup borrows `html` instead of cloning it for the probe (C01).
    template_ids: HashMap<String, [Option<String>; 4]>,
    events: Vec<String>,
    /// Non-fatal diagnostics: (source offset, message), surfaced as `Warning`s (D03/D04/D05).
    warnings: Vec<(u32, String)>,
}

impl<'s> Transformer<'s> {
    pub fn new(source: &'s str, module_name: &'s str, program: &Program<'_>) -> Self {
        let mut names = NameCollector::default();
        names.visit_program(program);
        Self {
            source,
            module_name,
            taken: names.names,
            next_suffix: HashMap::new(),
            helpers: Vec::new(),
            templates: Vec::new(),
            template_ids: HashMap::new(),
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Compiles every JSX expression in `program`: the code plus `(offset, message)`
    /// diagnostics. `None` when there is no JSX.
    pub fn program(mut self, program: &Program<'_>) -> Option<(Code, Vec<(u32, String)>)> {
        let mut finder = Finder { t: &mut self, found: Vec::new() };
        finder.visit_program(program);
        let found = finder.found;
        if found.is_empty() {
            return None;
        }

        // Runtime imports and templates go after the leading imports.
        let mut at = program.hashbang.as_ref().map_or(0, |h| h.span.end);
        if let Some(d) = program.directives.last() {
            at = d.span.end;
        }
        for stmt in &program.body {
            match stmt {
                Statement::ImportDeclaration(i) => at = i.span.end,
                _ => break,
            }
        }
        let split = found.partition_point(|(span, _)| span.start < at);
        let mut found = found;
        let after = found.split_off(split);
        let end = self.source.len() as u32;

        let mut code = self.splice(0, at, found);
        let delegate = (!self.events.is_empty()).then(|| self.helper("delegateEvents"));
        let header = self.header();
        if at == 0 {
            code.push(&header);
            code.push("\n");
        } else {
            code.push("\n");
            code.push(&header);
        }
        code.append(self.splice(at, end, after));
        if let Some(delegate) = delegate {
            // Canonical order: identical event sets emit identical trailers (C12).
            let mut names: Vec<String> = self.events.iter().map(|e| js_string(e)).collect();
            names.sort();
            let _ = write!(code, "\n{delegate}([{}]);\n", names.join(", "));
        }
        Some((code, std::mem::take(&mut self.warnings)))
    }

    /// Records a non-fatal diagnostic at a source offset (D03/D04/D05).
    fn warn(&mut self, offset: u32, message: impl Into<String>) {
        self.warnings.push((offset, message.into()));
    }

    fn header(&mut self) -> String {
        let mut out = String::new();
        let templates = std::mem::take(&mut self.templates);
        let mut decls = Vec::new();
        for (var, factory, args) in &templates {
            let alias = self.helper(factory);
            decls.push(format!("{var} = /*#__PURE__*/ {alias}({args})"));
        }
        let specifiers: Vec<String> =
            self.helpers.iter().map(|(name, alias)| format!("{name} as {alias}")).collect();
        let _ = write!(
            out,
            "import {{ {} }} from {};",
            specifiers.join(", "),
            js_string(self.module_name)
        );
        if !decls.is_empty() {
            let _ = write!(out, "\nconst {};", decls.join(",\n  "));
        }
        out
    }

    // -----------------------------------------------------------------------------------------
    // Names

    fn uid(&mut self, base: &str) -> String {
        let n = self.next_suffix.entry(base.to_string()).or_insert(1);
        loop {
            let name = if *n == 1 { base.to_string() } else { format!("{base}{n}") };
            *n += 1;
            if !self.taken.contains(&name) {
                return name;
            }
        }
    }

    /// Local alias of a runtime export, imported on first use.
    fn helper(&mut self, name: &'static str) -> String {
        if let Some((_, alias)) = self.helpers.iter().find(|(n, _)| *n == name) {
            return alias.clone();
        }
        let alias = self.uid(&format!("_${name}"));
        self.helpers.push((name, alias.clone()));
        alias
    }

    fn template(&mut self, html: String, svg: bool, math: bool) -> String {
        let slot = usize::from(svg) + 2 * usize::from(math);
        if let Some(var) = self.template_ids.get(&html).and_then(|v| v[slot].as_ref()) {
            return var.clone();
        }
        let var = self.uid("_tmpl$");
        // One factory per namespace (B02): HTML-only apps never import the
        // SVG/MathML parser branches.
        let (factory, args) = if svg {
            ("templateSVG", js_string(&format!("<svg>{html}</svg>")))
        } else if math {
            ("templateMathML", js_string(&html))
        } else {
            ("template", js_string(&html))
        };
        self.templates.push((var.clone(), factory, args));
        self.template_ids.entry(html).or_default()[slot] = Some(var.clone());
        var
    }

    // -----------------------------------------------------------------------------------------
    // Source splicing

    /// `source[start..end]` with each outermost JSX expression in `found` replaced.
    fn splice(&self, start: u32, end: u32, found: Vec<(Span, Code)>) -> Code {
        let mut code = Code::new();
        let mut pos = start;
        for (span, compiled) in found {
            code.src(self.source, pos, span.start);
            code.mark(span.start);
            code.append(compiled);
            pos = span.end;
        }
        code.src(self.source, pos, end);
        code
    }

    /// The source of `e` with nested JSX compiled.
    fn expr<'a>(&mut self, e: &Expression<'a>) -> Code {
        let span = e.span();
        let mut finder = Finder { t: self, found: Vec::new() };
        finder.visit_expression(e);
        let found = finder.found;
        self.splice(span.start, span.end, found)
    }

    /// A statement slice with nested JSX compiled and nested single-await
    /// `async` functions rewritten.
    fn stmt_code<'a>(&mut self, stmt: &Statement<'a>) -> Code {
        let span = stmt.span();
        let mut finder = Finder { t: self, found: Vec::new() };
        finder.visit_statement(stmt);
        let found = finder.found;
        self.splice(span.start, span.end, found)
    }

    /// Parameter list with nested JSX compiled (defaults may hold elements).
    fn params_code<'a>(&mut self, params: &FormalParameters<'a>) -> Code {
        let span = params.span;
        let mut finder = Finder { t: self, found: Vec::new() };
        finder.visit_formal_parameters(params);
        let found = finder.found;
        self.splice(span.start, span.end, found)
    }

    // -----------------------------------------------------------------------------------------
    // Async components

    /// Rewrites `async function` with a single top-level `await` into its sync shape;
    /// `None` leaves the function for the ordinary JSX walk (warning on D06 when the
    /// function is component-like but outside first-support scope).
    fn async_function<'a>(&mut self, func: &Function<'a>) -> Option<Code> {
        if !func.r#async || func.generator {
            return None;
        }
        let Some(body) = func.body.as_ref() else { return None };
        let plan = match async_plan(body) {
            Ok(plan) => plan,
            Err(reason) => {
                if reason != NO_TOP_AWAIT && has_jsx(body) {
                    self.warn(
                        func.span.start,
                        format!(
                            "async components support one top-level `const x = await …` \
                             followed by `return …` ({reason}); leaving `async` as written."
                        ),
                    );
                }
                return None;
            }
        };
        let mut code = Code::new();
        code.push("function ");
        if let Some(id) = &func.id {
            code.mark(id.span.start);
            code.src(self.source, id.span.start, id.span.end);
        }
        if let Some(params) = &func.type_parameters {
            code.src(self.source, params.span.start, params.span.end);
        }
        code.append(self.params_code(&func.params));
        if let Some(ret) = &func.return_type {
            code.append(self.return_type_code(ret));
        }
        code.push(" ");
        code.append(self.async_body_code(body, &plan));
        Some(code)
    }

    /// Same rewrite for `async (params) => { … }`; concise bodies have no statements.
    fn async_arrow<'a>(&mut self, arrow: &ArrowFunctionExpression<'a>) -> Option<Code> {
        if !arrow.r#async {
            return None;
        }
        let ArrowFunctionBody::FunctionBody(body) = &arrow.body else { return None };
        let plan = match async_plan(body) {
            Ok(plan) => plan,
            Err(reason) => {
                if reason != NO_TOP_AWAIT && has_jsx(body) {
                    self.warn(
                        arrow.span.start,
                        format!(
                            "async components support one top-level `const x = await …` \
                             followed by `return …` ({reason}); leaving `async` as written."
                        ),
                    );
                }
                return None;
            }
        };
        let mut code = Code::new();
        if let Some(params) = &arrow.type_parameters {
            code.src(self.source, params.span.start, params.span.end);
        }
        code.append(self.params_code(&arrow.params));
        if let Some(ret) = &arrow.return_type {
            code.append(self.return_type_code(ret));
        }
        code.push(" => ");
        code.append(self.async_body_code(body, &plan));
        Some(code)
    }

    /// Emitted return type of an async rewrite: `Promise<X>` unwraps to `X` because the
    /// output is sync, anything else passes through verbatim. An unrecognised `Promise`
    /// shape warns; other annotations are the author's own and stay silent.
    fn return_type_code(&mut self, ret: &TSTypeAnnotation<'_>) -> Code {
        let mut code = Code::new();
        if let Some(inner) = promise_inner(ret) {
            code.push(": ");
            code.mark(inner.start);
            code.src(self.source, inner.start, inner.end);
            return code;
        }
        code.push(" ");
        code.src(self.source, ret.span.start, ret.span.end);
        if is_promise_ref(ret) {
            self.warn(ret.span.start, "could not unwrap `Promise<…>` return type; kept as written.");
        }
        code
    }

    /// Sync body shared by both function shapes: one fetch `effect` per `await` plus a `memo`
    /// running the suffix once every step settles.
    fn async_body_code<'a>(&mut self, body: &FunctionBody<'a>, plan: &AsyncPlan<'a>) -> Code {
        let get_owner = self.helper("getOwner");
        let signal = self.helper("signal");
        let on_cleanup = self.helper("onCleanup");
        let effect = self.helper("effect");
        let memo = self.helper("memo");
        let track_async = self.helper("trackAsync");
        let track_pending = self.helper("trackPending");
        let owner = self.uid("_owner$");
        let err = self.uid("_err$");
        let set_err = self.uid("_setErr$");
        let epoch = self.uid("_epoch$");
        let prom = self.uid("_p$");
        let my = self.uid("_my$");
        let ex = self.uid("_ex$");
        let mut steps = Vec::with_capacity(plan.awaits.len());
        for (step, _) in plan.awaits.iter().enumerate() {
            steps.push(AsyncStep {
                val: self.uid(&format!("_val{step}$")),
                set_val: self.uid(&format!("_setVal{step}$")),
                settled: self.uid(&format!("_settled{step}$")),
                set_settled: self.uid(&format!("_setSettled{step}$")),
                tmp: self.uid(&format!("_t{step}$")),
            });
        }

        let mut code = Code::new();
        code.push("{\n");
        let _ = write!(code, "const {owner} = {get_owner}();\n");
        for step in &steps {
            let _ = write!(code, "const [{}, {}] = {signal}(undefined as any);\n", step.val, step.set_val);
            let _ = write!(code, "const [{}, {}] = {signal}(false);\n", step.settled, step.set_settled);
        }
        let _ = write!(code, "const [{err}, {set_err}] = {signal}(undefined as any);\n");
        let _ = write!(code, "let {epoch} = 0;\n");
        let _ = write!(code, "{on_cleanup}(() => {{ {epoch}++; }});\n");
        let last = steps.last().unwrap();
        let _ = write!(code, "{track_pending}(() => !{}() && {err}() === undefined);\n", last.settled);
        let mut seg_start = 0;
        for (step, awaited) in plan.awaits.iter().enumerate() {
            let _ = write!(code, "{effect}(() => {{\n");
            if step > 0 {
                self.await_prelude(&mut code, plan, &steps, step, "return;");
            }
            for stmt in &body.statements[seg_start..awaited.stmt_index] {
                code.mark(stmt.span().start);
                code.append(self.stmt_code(stmt));
                code.push("\n");
            }
            let _ = write!(code, "{set_err}(undefined);\n");
            let _ = write!(code, "{}({});\n", steps[step].set_settled, "false");
            let _ = write!(code, "const {prom} = Promise.resolve(");
            code.append(self.expr(awaited.argument));
            code.push(");\n");
            let _ = write!(code, "const {my} = ++{epoch};\n");
            let _ = write!(
                code,
                "{track_async}({owner}, {prom}, () => {my} === {epoch}, (_v) => {{ {}(_v); {}(true); }}, {set_err});\n",
                steps[step].set_val,
                steps[step].set_settled,
            );
            code.push("});\n");
            seg_start = awaited.stmt_index + 1;
        }
        let _ = write!(code, "return {memo}(() => {{\n");
        let _ = write!(code, "const {ex} = {err}();\n");
        let _ = write!(code, "if ({ex} !== undefined) throw {ex};\n");
        self.await_prelude(&mut code, plan, &steps, plan.awaits.len(), "return undefined;");
        for stmt in &body.statements[seg_start..plan.ret_index] {
            code.mark(stmt.span().start);
            code.append(self.stmt_code(stmt));
            code.push("\n");
        }
        let Statement::ReturnStatement(ret) = &body.statements[plan.ret_index] else {
            unreachable!("planned return statement")
        };
        code.push("return ");
        code.append(self.expr(ret.argument.as_ref().unwrap()));
        code.push(";\n");
        code.push("});\n}");
        code
    }

    fn await_prelude(
        &mut self,
        code: &mut Code,
        plan: &AsyncPlan<'_>,
        steps: &[AsyncStep],
        upto: usize,
        exit: &str,
    ) {
        let mut gate = String::new();
        for (step, _) in plan.awaits[..upto].iter().enumerate() {
            if step > 0 {
                gate.push_str(" || ");
            }
            gate.push('!');
            gate.push_str(&steps[step].settled);
            gate.push_str("()");
        }
        let _ = write!(code, "if ({gate}) {exit}\n");
        for (step, awaited) in plan.awaits[..upto].iter().enumerate() {
            let _ = write!(code, "const {} = {}();\n", steps[step].tmp, steps[step].val);
            let _ = write!(code, "{} ", awaited.kind);
            code.mark(awaited.pattern.start);
            code.src(self.source, awaited.pattern.start, awaited.pattern.end);
            if let Some(ann) = awaited.annotation {
                code.src(self.source, ann.start, ann.end);
            }
            let _ = write!(code, " = {};\n", steps[step].tmp);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Elements

    fn element<'a>(&mut self, el: &JSXElement<'a>) -> Code {
        match tag_of(&el.opening_element.name) {
            Tag::Component(span) => self.component(el, span),
            Tag::Native(tag) => self.template_root(el, &tag),
        }
    }

    fn fragment<'a>(&mut self, frag: &JSXFragment<'a>) -> Code {
        let items = self.child_items(&frag.children, false);
        let mut code = Code::new();
        match items.len() {
            0 => code.push("[]"),
            1 => {
                let item = items.into_iter().next().unwrap();
                code.append(self.list_item(item));
            }
            _ => {
                code.push("[");
                for (i, item) in items.into_iter().enumerate() {
                    if i > 0 {
                        code.push(", ");
                    }
                    code.append(self.list_item(item));
                }
                code.push("]");
            }
        }
        code
    }

    /// An entry of a children array: dynamic expressions become memos so they stay reactive.
    fn list_item<'a>(&mut self, item: Item<'_, 'a>) -> Code {
        match item {
            Item::Text(s) => code_of(&js_string(&s)),
            Item::Element(e) => self.element(e),
            Item::Fragment(f) => self.fragment(f),
            Item::Expr(e) => {
                if !is_function(e) && is_dynamic(e, true, false) {
                    let memo = self.helper("memo");
                    let mut code = code_of(&memo);
                    code.push("(");
                    code.append(self.getter(e));
                    code.push(")");
                    code
                } else {
                    self.expr(e)
                }
            }
        }
    }
    fn template_root<'a>(&mut self, el: &JSXElement<'a>, tag: &str) -> Code {
        let svg_root = is_svg_element(tag);
        // A `<math>` root needs the MathML-namespaced parser (runtime `isMathML`);
        // nested `<math>` is already handled by the HTML parser itself (C23).
        let math_root = tag == "math";
        let mut tpl = Template::default();
        let root_var = self.uid("_el$");
        let root = self.native(&mut tpl, el, tag, root_var.clone(), svg_root);
        let tmpl = self.template(tpl.html, svg_root, math_root);

        let mut decls = vec![format!("{root_var} = {tmpl}()")];
        collect_decls(&root_var, &root.children, &mut decls);
        if decls.len() == 1 && tpl.stmts.is_empty() && tpl.binds.is_empty() {
            return code_of(&format!("{tmpl}()"));
        }

        let mut code = Code::new();
        let _ = write!(code, "(() => {{\n  var {};\n", decls.join(",\n    "));
        for stmt in tpl.stmts {
            code.push("  ");
            code.append(stmt);
            code.push(";\n");
        }
        if !tpl.binds.is_empty() {
            code.push("  ");
            code.append(self.binds(tpl.binds));
            code.push(";\n");
        }
        let _ = write!(code, "  return {root_var};\n}})()");
        code
    }

    /// Writes `el` into the template and queues its runtime work.
    fn native<'a>(
        &mut self,
        tpl: &mut Template,
        el: &JSXElement<'a>,
        tag: &str,
        var: String,
        in_svg: bool,
    ) -> Node {
        let svg = in_svg || tag == "svg";
        let attrs = &el.opening_element.attributes;
        let mut items = self.child_items(&el.children, true);
        // `children` as an attribute stands in for JSX children — but when both are
        // present the nested children win and the attribute is ignored (D05).
        let children_attr = attrs.iter().find_map(|a| match a {
            JSXAttributeItem::Attribute(a) if attribute_name(a) == "children" => {
                match &a.value {
                    Some(JSXAttributeValue::ExpressionContainer(c)) => {
                        c.expression.as_expression().map(|e| (e, a.span))
                    }
                    _ => None,
                }
            }
            _ => None,
        });
        if items.is_empty() {
            if let Some((e, _)) = children_attr {
                items.push(Item::Expr(e));
            }
        } else if let Some((_, span)) = children_attr {
            self.warn(
                span.start,
                "`children` attribute is ignored because the element also has nested children.",
            );
        }

        let mut needs_ref = false;
        let _ = write!(tpl.html, "<{tag}");
        if attrs.iter().any(|a| matches!(a, JSXAttributeItem::SpreadAttribute(_))) {
            self.spread(tpl, &var, attrs, svg, !items.is_empty());
            needs_ref = true;
        } else {
            for attr in attrs {
                if let JSXAttributeItem::Attribute(attr) = attr {
                    needs_ref |= self.attribute(tpl, &var, attr, svg);
                }
            }
        }
        tpl.html.push('>');
        if is_void(tag) {
            return Node { var: Some(var), needs_ref, children: Vec::new() };
        }

        let child_svg = svg && tag != "foreignObject";
        let children = self.native_children(tpl, &var, items, child_svg, &mut needs_ref);
        let _ = write!(tpl.html, "</{tag}>");
        needs_ref |= children.iter().any(|c| c.needs_ref);
        Node { var: Some(var), needs_ref, children }
    }

    /// Static children go into the template; dynamic ones become `insert` calls positioned
    /// before the next static node (a `<!>` marker when they sit between two texts, which the
    /// HTML parser would merge).
    fn native_children<'a>(
        &mut self,
        tpl: &mut Template,
        parent: &str,
        items: Vec<Item<'_, 'a>>,
        svg: bool,
        needs_ref: &mut bool,
    ) -> Vec<Node> {
        let sole = items.len() == 1;
        let mut nodes: Vec<Node> = Vec::new();
        let mut inserts: Vec<(Code, usize)> = Vec::new();
        let mut last_is_text = false;
        let mut after_dynamic = false;
        for item in items {
            match item {
                Item::Text(s) => {
                    if after_dynamic && last_is_text {
                        tpl.html.push_str("<!>");
                        nodes.push(Node::leaf());
                    }
                    escape_text(&mut tpl.html, &s);
                    nodes.push(Node::leaf());
                    last_is_text = true;
                    after_dynamic = false;
                }
                Item::Element(e) => match tag_of(&e.opening_element.name) {
                    Tag::Native(tag) => {
                        let var = self.uid("_el$");
                        nodes.push(self.native(tpl, e, &tag, var, svg));
                        last_is_text = false;
                        after_dynamic = false;
                    }
                    Tag::Component(span) => {
                        inserts.push((self.component(e, span), nodes.len()));
                        after_dynamic = true;
                    }
                },
                Item::Fragment(f) => {
                    inserts.push((self.fragment(f), nodes.len()));
                    after_dynamic = true;
                }
                Item::Expr(e) => {
                    inserts.push((self.insert_value(e), nodes.len()));
                    after_dynamic = true;
                }
            }
        }

        if inserts.is_empty() {
            return nodes;
        }
        *needs_ref = true;
        let insert = self.helper("insert");
        for (value, next) in inserts {
            let marker = if sole {
                None
            } else if let Some(node) = nodes.get_mut(next) {
                node.needs_ref = true;
                if node.var.is_none() {
                    node.var = Some(self.uid("_el$"));
                }
                node.var.clone()
            } else {
                Some("null".to_string())
            };
            let mut stmt = code_of(&format!("{insert}({parent}, "));
            stmt.append(value);
            if let Some(marker) = marker {
                let _ = write!(stmt, ", {marker}");
            }
            stmt.push(")");
            tpl.stmts.push(stmt);
        }
        nodes
    }

    /// The value handed to `insert` for an expression child.
    fn insert_value<'a>(&mut self, e: &Expression<'a>) -> Code {
        if let Some(code) = self.conditional(unparen(e)) {
            return code;
        }
        if is_dynamic(e, true, false) { self.getter(e) } else { self.expr(e) }
    }

    /// `cond() ? <A/> : <B/>` and `cond() && <A/>`: the branch is rebuilt only when the
    /// condition's truthiness changes.
    fn conditional<'a>(&mut self, e: &Expression<'a>) -> Option<Code> {
        let (test, rest): (&Expression<'a>, Vec<&Expression<'a>>) = match e {
            Expression::ConditionalExpression(c) => (&c.test, vec![&c.consequent, &c.alternate]),
            Expression::LogicalExpression(l) if l.operator == LogicalOperator::And => {
                (&l.left, vec![&l.right])
            }
            _ => return None,
        };
        if !is_dynamic(test, true, false) || !rest.iter().any(|b| is_dynamic(b, true, true)) {
            return None;
        }
        let memo = self.helper("memo");
        let c = self.uid("_c$");
        let mut code = code_of(&format!("(() => {{ var {c} = {memo}(() => !!("));
        code.append(self.expr(test));
        let _ = write!(code, ")); return () => {c}()");
        if let [consequent, alternate] = rest[..] {
            code.push(" ? ");
            code.append(self.expr(consequent));
            code.push(" : ");
            code.append(self.expr(alternate));
        } else {
            code.push(" && ");
            code.append(self.expr(rest[0]));
        }
        code.push("; })()");
        Some(code)
    }

    /// `() => e`, or `f` for a bare `f()`.
    fn getter<'a>(&mut self, e: &Expression<'a>) -> Code {
        if let Expression::CallExpression(call) = unparen(e)
            && let Expression::Identifier(id) = &call.callee
            && call.arguments.is_empty()
            && !call.optional
            && call.type_arguments.is_none()
        {
            let mut code = Code::new();
            code.src(self.source, id.span.start, id.span.end);
            return code;
        }
        let body = self.expr(e);
        let mut code = code_of("() => ");
        if body.s.starts_with('{') {
            code.push("(");
            code.append(body);
            code.push(")");
        } else {
            code.append(body);
        }
        code
    }

    // -----------------------------------------------------------------------------------------
    // Attributes

    /// Handles one attribute of a non-spread element. Returns whether the element is referenced.
    fn attribute<'a>(
        &mut self,
        tpl: &mut Template,
        var: &str,
        attr: &JSXAttribute<'a>,
        svg: bool,
    ) -> bool {
        let name = attribute_name(attr);
        let value = match &attr.value {
            None => Value::Bare,
            Some(JSXAttributeValue::StringLiteral(s)) => {
                Value::Str(decode_entities(s.value.as_str()))
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(e) => Value::Expr(e),
                None => return false,
            },
            Some(JSXAttributeValue::Element(e)) => Value::Code(self.element(e)),
            Some(JSXAttributeValue::Fragment(f)) => Value::Code(self.fragment(f)),
        };

        let kind = if name == "ref" {
            let Value::Expr(e) = value else { return false };
            let stmt = self.element_ref(var, e);
            tpl.stmts.push(stmt);
            return true;
        } else if name == "children" {
            return false;
        } else if let Some(event) = name.strip_prefix("on:") {
            return self.event(tpl, var, event, false, value);
        } else if name.len() > 2
            && name.starts_with("on")
            && name.as_bytes()[2].is_ascii_uppercase()
            && !name.contains(':')
        {
            return self.event(tpl, var, &name[2..].to_ascii_lowercase(), true, value);
        } else if name == "class" || name == "className" {
            Kind::Class
        } else if name == "classList" {
            Kind::ClassList
        } else if name == "style" {
            Kind::Style
        } else if let Some(n) = name.strip_prefix("prop:") {
            Kind::Prop(n.to_string())
        } else if let Some(n) = name.strip_prefix("attr:") {
            Kind::Attr(n.to_string())
        } else if let Some(n) = name.strip_prefix("bool:") {
            Kind::Bool(n.to_string())
        } else if let Some(ns) = name.split_once(':').and_then(|(p, _)| attribute_namespace(p)) {
            Kind::AttrNs(ns, name.clone())
        } else if !svg && is_property(&name) {
            match name.as_str() {
                "value" | "checked" | "selected" => Kind::InlineProp(name.clone()),
                _ => Kind::Prop(name.clone()),
            }
        } else if name == "key" {
            // `key` only identifies `<For>` rows and component instances (D03).
            self.warn(
                attr.span().start,
                "`key` does nothing on a plain element; it renders as a plain attribute.",
            );
            Kind::Attr(name.clone())
        } else {
            // Possible typo of a known attribute (D02); namespaced, `data-*`,
            // `aria-*` and hyphenated names are custom by design and stay quiet.
            if !name.contains(['-', ':']) && let Some(suggestion) = suggest_attribute(&name) {
                self.warn(
                    attr.span().start,
                    format!("unknown attribute `{name}`; did you mean `{suggestion}`?"),
                );
            }
            Kind::Attr(name.clone())
        };

        // All-literal `style`/`classList` objects fold to static attributes (C09/C10),
        // dropping the runtime `style`/`classList` import when nothing else needs it.
        if matches!(kind, Kind::Style)
            && let Value::Expr(e) = &value
            && let Some(s) = static_style_object(e)
        {
            let _ = write!(tpl.html, " style=\"");
            escape_attribute(&mut tpl.html, &s);
            tpl.html.push('"');
            return false;
        }
        if matches!(kind, Kind::ClassList)
            && let Value::Expr(e) = &value
            && let Some(c) = static_class_list(e)
        {
            if !c.is_empty() {
                let _ = write!(tpl.html, " class=\"");
                escape_attribute(&mut tpl.html, &c);
                tpl.html.push('"');
            }
            return false;
        }

        // Literal values go straight into the template.
        let literal = match &value {
            Value::Bare => Some(Lit::Bare),
            Value::Str(s) => Some(Lit::Str(s.to_string())),
            Value::Expr(e) => literal(e),
            Value::Code(_) => None,
        };
        if let Some(lit) = literal {
            let html_name = match &kind {
                Kind::Attr(n) | Kind::AttrNs(_, n) | Kind::InlineProp(n) => Some(n.as_str()),
                Kind::Bool(n) => Some(n.as_str()),
                Kind::Class => Some("class"),
                Kind::Style => Some("style"),
                Kind::ClassList | Kind::Prop(_) => None,
            };
            if let Some(n) = html_name {
                let is_bool = matches!(kind, Kind::Bool(_));
                match lit {
                    Lit::Bare | Lit::Bool(true)
                        if is_bool || matches!(kind, Kind::InlineProp(_)) =>
                    {
                        let _ = write!(tpl.html, " {n}");
                    }
                    Lit::Bare => {
                        let _ = write!(tpl.html, " {n}");
                    }
                    Lit::Bool(true) => {
                        let _ = write!(tpl.html, " {n}=\"true\"");
                    }
                    Lit::Str(s) if is_bool => {
                        if !s.is_empty() {
                            let _ = write!(tpl.html, " {n}");
                        }
                    }
                    Lit::Str(s) => {
                        let _ = write!(tpl.html, " {n}=\"");
                        escape_attribute(&mut tpl.html, &s);
                        tpl.html.push('"');
                    }
                    Lit::Bool(false) | Lit::Null => {}
                }
                return false;
            }
            if matches!(lit, Lit::Null) && matches!(kind, Kind::ClassList) {
                return false;
            }
        }

        let setter = match kind {
            Kind::Attr(n) => Setter::Attr(var.to_string(), n),
            Kind::AttrNs(ns, n) => Setter::AttrNs(var.to_string(), ns, n),
            Kind::Bool(n) => Setter::Bool(var.to_string(), n),
            Kind::Prop(n) | Kind::InlineProp(n) => Setter::Prop(var.to_string(), n),
            Kind::Class => Setter::Class(var.to_string()),
            Kind::ClassList => Setter::ClassList(var.to_string()),
            Kind::Style => Setter::Style(var.to_string()),
        };
        match value {
            Value::Expr(e) if is_dynamic(e, true, false) => {
                let value = self.expr(e);
                tpl.binds.push(Bind { setter, value });
            }
            value => {
                let value = match value {
                    Value::Bare => code_of("true"),
                    Value::Str(s) => code_of(&js_string(&s)),
                    Value::Expr(e) => self.expr(e),
                    Value::Code(code) => code,
                };
                tpl.stmts.push(self.set(&setter, value, None));
            }
        }
        true
    }

    fn event<'a>(
        &mut self,
        tpl: &mut Template,
        var: &str,
        event: &str,
        delegatable: bool,
        value: Value<'_, 'a>,
    ) -> bool {
        let handler = match value {
            Value::Expr(e) => e,
            // `onclick="…"` and friends stay plain attributes.
            Value::Str(s) => {
                let _ = write!(tpl.html, " on{event}=\"");
                escape_attribute(&mut tpl.html, &s);
                tpl.html.push('"');
                return false;
            }
            Value::Bare | Value::Code(_) => return false,
        };
        let listen = self.helper("addEventListener");
        let name = js_string(event);
        if !(delegatable && is_delegated_event(event)) {
            let mut stmt = code_of(&format!("{listen}({var}, {name}, "));
            stmt.append(self.expr(handler));
            stmt.push(")");
            tpl.stmts.push(stmt);
            return true;
        }

        if !self.events.iter().any(|e| e == event) {
            self.events.push(event.to_string());
        }
        let key = format!("{var}.$${event}");
        let inner = unparen(handler);
        let stmt = if is_function(inner) {
            let mut stmt = code_of(&format!("{key} = "));
            stmt.append(self.expr(handler));
            stmt
        } else if let Expression::ArrayExpression(a) = inner
            && a.elements.len() == 2
            && let (Some(f), Some(data)) =
                (a.elements[0].as_expression(), a.elements[1].as_expression())
        {
            // `[handler, data]`: the handler receives `data` first.
            let mut stmt = code_of(&format!("{key} = "));
            stmt.append(self.expr(f));
            let _ = write!(stmt, ";\n  {key}Data = ");
            stmt.append(self.expr(data));
            stmt
        } else {
            let mut stmt = code_of(&format!("{listen}({var}, {name}, "));
            stmt.append(self.expr(handler));
            stmt.push(", true)");
            stmt
        };
        tpl.stmts.push(stmt);
        true
    }

    /// `ref={fn}` calls `fn(el)`; `ref={variable}` assigns the element unless it holds a function.
    fn element_ref<'a>(&mut self, var: &str, e: &Expression<'a>) -> Code {
        let use_ = self.helper("use");
        let inner = unparen(e);
        if is_function(inner) {
            let mut code = code_of(&format!("{use_}("));
            code.append(self.expr(e));
            let _ = write!(code, ", {var})");
            return code;
        }
        let r = self.uid("_ref$");
        let mut code = code_of(&format!("var {r} = "));
        code.append(self.expr(e));
        let _ = write!(code, ";\n  typeof {r} === \"function\" ");
        if is_assignable(inner) {
            let _ = write!(code, "? {use_}({r}, {var}) : ");
            code.append(self.expr(e));
            let _ = write!(code, " = {var}");
        } else {
            let _ = write!(code, "&& {use_}({r}, {var})");
        }
        code
    }

    /// An element with a spread: every attribute goes through `spread`, children stay compiled.
    fn spread<'a>(
        &mut self,
        tpl: &mut Template,
        var: &str,
        attrs: &[JSXAttributeItem<'a>],
        svg: bool,
        has_children: bool,
    ) {
        let mut props = Props::default();
        for attr in attrs {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    let source = self.spread_source(&s.argument);
                    props.spread(source);
                }
                JSXAttributeItem::Attribute(a) => {
                    let name = attribute_name(a);
                    if name == "ref" {
                        if let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                            && let Some(e) = c.expression.as_expression()
                        {
                            let stmt = self.element_ref(var, e);
                            tpl.stmts.push(stmt);
                        }
                    } else if !(name == "children" && has_children) {
                        let prop = self.prop(&name, a, false);
                        props.push(prop);
                    }
                }
            }
        }
        let spread = self.helper("spread");
        let mut stmt = code_of(&format!("{spread}({var}, "));
        stmt.append(self.props(props));
        let _ = write!(stmt, ", {svg}, {has_children})");
        tpl.stmts.push(stmt);
    }

    /// Merges the dynamic attribute updates of a template into one `bind`.
    fn binds(&mut self, binds: Vec<Bind>) -> Code {
        let bind = self.helper("bind");
        if binds.len() == 1 {
            let b = binds.into_iter().next().unwrap();
            let mut code = Code::new();
            if b.setter.threads_prev() {
                let p = self.uid("_p$");
                let _ = write!(code, "{bind}({p} => ");
                code.append(self.set(&b.setter, b.value, Some(&p)));
            } else {
                let _ = write!(code, "{bind}(() => ");
                code.append(self.set(&b.setter, b.value, None));
            }
            code.push(")");
            return code;
        }

        let p = self.uid("_p$");
        let mut code = code_of(&format!("{bind}({p} => {{\n    var "));
        let mut vars = Vec::with_capacity(binds.len());
        let mut setters = Vec::with_capacity(binds.len());
        for (i, b) in binds.into_iter().enumerate() {
            let v = self.uid("_v$");
            if i > 0 {
                code.push(",\n      ");
            }
            let _ = write!(code, "{v} = ");
            code.append(b.value);
            vars.push(v);
            setters.push(b.setter);
        }
        code.push(";\n");
        for (i, (v, setter)) in vars.iter().zip(&setters).enumerate() {
            let slot = format!("{p}[{i}]");
            let _ = write!(code, "    {v} !== {slot} && (");
            if setter.threads_prev() {
                let _ = write!(code, "{slot} = ");
                code.append(self.set(setter, code_of(v), Some(&slot)));
            } else {
                code.append(self.set(setter, code_of(&format!("{slot} = {v}")), None));
            }
            code.push(");\n");
        }
        let _ = write!(code, "    return {p};\n  }}, [])");
        code
    }

    fn set(&mut self, setter: &Setter, value: Code, prev: Option<&str>) -> Code {
        let mut code = match setter {
            Setter::Attr(el, n) => {
                code_of(&format!("{}({el}, {}, ", self.helper("setAttribute"), js_string(n)))
            }
            Setter::AttrNs(el, ns, n) => code_of(&format!(
                "{}({el}, {}, {}, ",
                self.helper("setAttributeNS"),
                js_string(ns),
                js_string(n)
            )),
            Setter::Bool(el, n) => {
                code_of(&format!("{}({el}, {}, ", self.helper("setBoolAttribute"), js_string(n)))
            }
            Setter::Prop(el, n) => {
                let mut code = code_of(&format!("{} = ", member(el, n)));
                code.append(value);
                return code;
            }
            Setter::Class(el) => code_of(&format!("{}({el}, ", self.helper("className"))),
            Setter::Style(el) => code_of(&format!("{}({el}, ", self.helper("style"))),
            Setter::ClassList(el) => code_of(&format!("{}({el}, ", self.helper("classList"))),
        };
        code.append(value);
        if let Some(prev) = prev {
            let _ = write!(code, ", {prev}");
        }
        code.push(")");
        code
    }

    // -----------------------------------------------------------------------------------------
    // Components

    fn component<'a>(&mut self, el: &JSXElement<'a>, tag: Span) -> Code {
        // An inline array literal as `each` gets a fresh identity on every evaluation,
        // rebuilding all rows each time; hoist it or memoize it (D04).
        if &self.source[tag.start as usize..tag.end as usize] == "For" {
            for attr in &el.opening_element.attributes {
                if let JSXAttributeItem::Attribute(a) = attr
                    && attribute_name(a) == "each"
                    && let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value
                    && let Some(Expression::ArrayExpression(_)) = c.expression.as_expression()
                {
                    self.warn(
                        a.span.start,
                        "`<For each={...}>` holds an inline array literal; hoist it so rows survive re-renders.",
                    );
                }
            }
        }

        let create = self.helper("createComponent");
        let items = self.child_items(&el.children, false);
        let mut props = Props::default();
        for attr in &el.opening_element.attributes {
            match attr {
                JSXAttributeItem::SpreadAttribute(s) => {
                    let source = self.spread_source(&s.argument);
                    props.spread(source);
                }
                JSXAttributeItem::Attribute(a) => {
                    let name = attribute_name(a);
                    if !(name == "children" && !items.is_empty()) {
                        let prop = self.prop(&name, a, true);
                        props.push(prop);
                    }
                }
            }
        }
        if let Some(children) = self.component_children(items) {
            props.push(children);
        }
        let mut code = code_of(&format!("{create}("));
        code.src(self.source, tag.start, tag.end);
        code.push(", ");
        code.append(self.props(props));
        code.push(")");
        code
    }

    /// One `key: value` or `get key() { … }` entry of a props object.
    fn prop<'a>(&mut self, name: &str, attr: &JSXAttribute<'a>, component: bool) -> Code {
        let key = property_key(name);
        match &attr.value {
            None => code_of(&format!("{key}: true")),
            Some(JSXAttributeValue::StringLiteral(s)) => {
                code_of(&format!("{key}: {}", js_string(&decode_entities(s.value.as_str()))))
            }
            Some(JSXAttributeValue::Element(e)) => {
                let value = self.element(e);
                self.getter_prop(&key, value)
            }
            Some(JSXAttributeValue::Fragment(f)) => {
                let value = self.fragment(f);
                self.getter_prop(&key, value)
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                let Some(e) = c.expression.as_expression() else {
                    return code_of(&format!("{key}: undefined"));
                };
                if component && name == "ref" && is_assignable(unparen(e)) {
                    // Forwarded to the element the component puts it on.
                    let (r, arg) = (self.uid("_ref$"), self.uid("r$"));
                    let mut code = code_of(&format!("ref({arg}) {{ var {r} = "));
                    code.append(self.expr(e));
                    let _ = write!(code, "; typeof {r} === \"function\" ? {r}({arg}) : ");
                    code.append(self.expr(e));
                    let _ = write!(code, " = {arg}; }}");
                    return code;
                }
                if is_dynamic(e, true, component) {
                    let value = self.expr(e);
                    self.getter_prop(&key, value)
                } else {
                    let mut code = code_of(&format!("{key}: "));
                    code.append(self.expr(e));
                    code
                }
            }
        }
    }

    fn getter_prop(&self, key: &str, value: Code) -> Code {
        let mut code = code_of(&format!("get {key}() {{ return "));
        code.append(value);
        code.push("; }");
        code
    }

    fn component_children<'a>(&mut self, items: Vec<Item<'_, 'a>>) -> Option<Code> {
        match items.len() {
            0 => None,
            1 => Some(match items.into_iter().next().unwrap() {
                Item::Text(s) => code_of(&format!("children: {}", js_string(&s))),
                Item::Element(e) => {
                    let value = self.element(e);
                    self.getter_prop("children", value)
                }
                Item::Fragment(f) => {
                    let value = self.fragment(f);
                    self.getter_prop("children", value)
                }
                Item::Expr(e) if !is_function(e) && is_dynamic(e, true, true) => {
                    let value = self.expr(e);
                    self.getter_prop("children", value)
                }
                Item::Expr(e) => {
                    let mut code = code_of("children: ");
                    code.append(self.expr(e));
                    code
                }
            }),
            _ => {
                let mut code = code_of("get children() { return [");
                for (i, item) in items.into_iter().enumerate() {
                    if i > 0 {
                        code.push(", ");
                    }
                    code.append(self.list_item(item));
                }
                code.push("]; }");
                Some(code)
            }
        }
    }

    /// A spread argument; a computed one is re-read on every access to stay reactive.
    fn spread_source<'a>(&mut self, e: &Expression<'a>) -> Source {
        if is_dynamic(e, true, false) {
            let mut code = code_of("() => ");
            code.append(self.expr(e));
            Source::Function(code)
        } else {
            Source::Value(self.expr(e))
        }
    }

    /// A props object: a literal, a single spread source, or `mergeProps` over all sources.
    fn props(&mut self, props: Props) -> Code {
        let Props { mut sources, entries } = props;
        if sources.is_empty() {
            return object(entries);
        }
        if !entries.is_empty() {
            sources.push(Source::Value(object(entries)));
        }
        if sources.len() == 1
            && let Source::Value(_) = &sources[0]
        {
            let Some(Source::Value(code)) = sources.pop() else { unreachable!() };
            return code;
        }
        let merge = self.helper("mergeProps");
        let mut code = code_of(&format!("{merge}("));
        for (i, source) in sources.into_iter().enumerate() {
            if i > 0 {
                code.push(", ");
            }
            match source {
                Source::Value(c) | Source::Function(c) => code.append(c),
            }
        }
        code.push(")");
        code
    }

    // -----------------------------------------------------------------------------------------
    // Children

    /// Meaningful children: cleaned non-empty text (adjacent text merged), elements, fragments
    /// and expressions. For native elements, fragments are flattened and literal expressions
    /// become text.
    fn child_items<'b, 'a>(
        &mut self,
        children: &'b [JSXChild<'a>],
        native: bool,
    ) -> Vec<Item<'b, 'a>> {
        let mut items = Vec::new();
        collect_items(children, native, &mut items);
        items
    }
}

fn collect_items<'b, 'a>(
    children: &'b [JSXChild<'a>],
    native: bool,
    items: &mut Vec<Item<'b, 'a>>,
) {
    let push_text = |items: &mut Vec<Item<'b, 'a>>, s: String| {
        if s.is_empty() {
            return;
        }
        if let Some(Item::Text(prev)) = items.last_mut() {
            prev.push_str(&s);
        } else {
            items.push(Item::Text(s));
        }
    };
    for child in children {
        match child {
            JSXChild::Text(t) => {
                push_text(items, clean_jsx_text(&decode_entities(t.value.as_str())))
            }
            JSXChild::Element(e) => items.push(Item::Element(e)),
            JSXChild::Fragment(f) if native => collect_items(&f.children, native, items),
            JSXChild::Fragment(f) => items.push(Item::Fragment(f)),
            JSXChild::ExpressionContainer(c) => {
                let Some(e) = c.expression.as_expression() else { continue };
                match unparen(e) {
                    Expression::JSXElement(el) => items.push(Item::Element(el)),
                    Expression::JSXFragment(f) if native => {
                        collect_items(&f.children, native, items)
                    }
                    Expression::JSXFragment(f) => items.push(Item::Fragment(f)),
                    inner => match static_text(inner) {
                        Some(s) if native => push_text(items, s),
                        _ => items.push(Item::Expr(e)),
                    },
                }
            }
            JSXChild::Spread(s) => items.push(Item::Expr(&s.expression)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Template model

#[derive(Default)]
struct Template {
    html: String,
    stmts: Vec<Code>,
    binds: Vec<Bind>,
}

/// A static DOM node of a template: an element, a text run or a `<!>` marker.
struct Node {
    /// Assigned to elements up front and to text/markers once they are referenced.
    var: Option<String>,
    needs_ref: bool,
    children: Vec<Node>,
}

impl Node {
    fn leaf() -> Self {
        Self { var: None, needs_ref: false, children: Vec::new() }
    }
}

/// Declares each referenced node as a `firstChild`/`nextSibling` walk from its nearest
/// declared predecessor.
fn collect_decls(parent: &str, children: &[Node], decls: &mut Vec<String>) {
    let mut base = format!("{parent}.firstChild");
    let mut base_index = 0;
    for (i, child) in children.iter().enumerate() {
        if !child.needs_ref {
            continue;
        }
        let var = child.var.as_ref().expect("referenced nodes are named");
        decls.push(format!("{var} = {base}{}", ".nextSibling".repeat(i - base_index)));
        base = var.clone();
        base_index = i;
        collect_decls(var, &child.children, decls);
    }
}

enum Item<'b, 'a> {
    Text(String),
    Element(&'b JSXElement<'a>),
    Fragment(&'b JSXFragment<'a>),
    Expr(&'b Expression<'a>),
}

enum Value<'b, 'a> {
    Bare,
    Str(Cow<'b, str>),
    Expr(&'b Expression<'a>),
    Code(Code),
}

enum Lit {
    Bare,
    Str(String),
    Bool(bool),
    Null,
}

enum Kind {
    Attr(String),
    AttrNs(&'static str, String),
    Bool(String),
    Prop(String),
    /// A property whose literal initial value can be written as an attribute.
    InlineProp(String),
    Class,
    ClassList,
    Style,
}

enum Setter {
    Attr(String, String),
    AttrNs(String, &'static str, String),
    Bool(String, String),
    Prop(String, String),
    Class(String),
    ClassList(String),
    Style(String),
}

impl Setter {
    /// Setters that diff against the previous value they returned.
    fn threads_prev(&self) -> bool {
        matches!(self, Setter::ClassList(_) | Setter::Style(_))
    }
}

struct Bind {
    setter: Setter,
    value: Code,
}

enum Source {
    Value(Code),
    Function(Code),
}

#[derive(Default)]
struct Props {
    sources: Vec<Source>,
    entries: Vec<Code>,
}

impl Props {
    fn push(&mut self, entry: Code) {
        self.entries.push(entry);
    }

    fn spread(&mut self, source: Source) {
        if !self.entries.is_empty() {
            self.sources.push(Source::Value(object(std::mem::take(&mut self.entries))));
        }
        self.sources.push(source);
    }
}

fn object(entries: Vec<Code>) -> Code {
    if entries.is_empty() {
        return code_of("{}");
    }
    let mut code = code_of("{ ");
    for (i, entry) in entries.into_iter().enumerate() {
        if i > 0 {
            code.push(", ");
        }
        code.append(entry);
    }
    code.push(" }");
    code
}

fn code_of(s: &str) -> Code {
    let mut code = Code::new();
    code.push(s);
    code
}

// ---------------------------------------------------------------------------------------------
// AST helpers

enum Tag {
    Native(String),
    Component(Span),
}

fn tag_of(name: &JSXElementName<'_>) -> Tag {
    let by_name = |name: &str, span: Span| {
        if name.starts_with(|c: char| c.is_ascii_lowercase()) || name.contains('-') {
            Tag::Native(name.to_string())
        } else {
            Tag::Component(span)
        }
    };
    match name {
        JSXElementName::Identifier(id) => by_name(id.name.as_str(), id.span),
        JSXElementName::IdentifierReference(id) => by_name(id.name.as_str(), id.span),
        JSXElementName::NamespacedName(n) => {
            Tag::Native(format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str()))
        }
        JSXElementName::MemberExpression(m) => Tag::Component(m.span),
        JSXElementName::ThisExpression(t) => Tag::Component(t.span),
    }
}

fn attribute_name(attr: &JSXAttribute<'_>) -> String {
    match &attr.name {
        JSXAttributeName::Identifier(id) => id.name.to_string(),
        JSXAttributeName::NamespacedName(n) => {
            format!("{}:{}", n.namespace.name.as_str(), n.name.name.as_str())
        }
    }
}

fn unparen<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

fn is_function(e: &Expression<'_>) -> bool {
    matches!(unparen(e), Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
}

fn is_assignable(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
    )
}

// ---------------------------------------------------------------------------------------------

/// Inner span of a `Promise<X>` return annotation; `None` for anything else.
fn promise_inner(ret: &TSTypeAnnotation<'_>) -> Option<Span> {
    let TSType::TSTypeReference(r) = &ret.type_annotation else { return None };
    if !is_promise_ref(ret) {
        return None;
    }
    let args = r.type_arguments.as_ref()?;
    if args.params.len() != 1 {
        return None;
    }
    Some(args.params[0].span())
}

/// Whether the annotation names the global `Promise` type (whatever its arguments).
fn is_promise_ref(ret: &TSTypeAnnotation<'_>) -> bool {
    let TSType::TSTypeReference(r) = &ret.type_annotation else { return false };
    match &r.type_name {
        TSTypeName::IdentifierReference(id) => id.name.as_str() == "Promise",
        _ => false,
    }
}

/// Marker for the one `async_plan` failure that stays silent (ordinary async helpers).
const NO_TOP_AWAIT: &str = "no top-level `await`";

struct AsyncAwait<'a> {
    stmt_index: usize,
    kind: &'static str,
    pattern: Span,
    annotation: Option<Span>,
    argument: &'a Expression<'a>,
}

struct AsyncPlan<'a> {
    awaits: Vec<AsyncAwait<'a>>,
    ret_index: usize,
}

struct AsyncStep {
    val: String,
    set_val: String,
    settled: String,
    set_settled: String,
    tmp: String,
}

fn async_plan<'a>(body: &'a FunctionBody<'a>) -> Result<AsyncPlan<'a>, &'static str> {
    if !body.directives.is_empty() {
        return Err("function directives");
    }
    let mut awaits = Vec::new();
    for (stmt_index, stmt) in body.statements.iter().enumerate() {
        if let Statement::VariableDeclaration(decl) = stmt
            && decl.declarations.len() == 1
            && let Some(init) = &decl.declarations[0].init
            && let Expression::AwaitExpression(awaited) = unparen(init)
        {
            let declarator = &decl.declarations[0];
            let kind = match decl.kind {
                VariableDeclarationKind::Const => "const",
                VariableDeclarationKind::Let => "let",
                VariableDeclarationKind::Var => "var",
                VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                    return Err("`using` declaration");
                }
            };
            awaits.push(AsyncAwait {
                stmt_index,
                kind,
                pattern: declarator.id.span(),
                annotation: declarator.type_annotation.as_ref().map(|ann| ann.span()),
                argument: &awaited.argument,
            });
            continue;
        }
        let mut found_await = AwaitCheck { found: false };
        found_await.visit_statement(stmt);
        if found_await.found {
            return Err("`await` outside a top-level declarator");
        }
    }
    if awaits.is_empty() {
        return Err(NO_TOP_AWAIT);
    }
    let last_await = awaits.last().unwrap().stmt_index;
    for (stmt_index, stmt) in body.statements.iter().enumerate() {
        if stmt_index > last_await {
            break;
        }
        if awaits.iter().any(|awaited| awaited.stmt_index == stmt_index) {
            continue;
        }
        let mut found_exit = RetCheck { found: false };
        found_exit.visit_statement(stmt);
        if found_exit.found {
            return Err("early `return`/`throw` before `await`");
        }
    }
    let mut ret_index = body.statements.len();
    while ret_index > last_await + 1 {
        if matches!(body.statements[ret_index - 1], Statement::EmptyStatement(_)) {
            ret_index -= 1;
        } else {
            break;
        }
    }
    let Statement::ReturnStatement(ret) = &body.statements[ret_index - 1] else {
        return Err("suffix does not end with `return …`");
    };
    if ret.argument.is_none() {
        return Err("suffix does not end with `return …`");
    }
    let mut awaited_names = NameDecls::default();
    for awaited in &awaits {
        let Statement::VariableDeclaration(decl) = &body.statements[awaited.stmt_index] else {
            unreachable!("await statement shape")
        };
        awaited_names.visit_binding_pattern(&decl.declarations[0].id);
    }
    let mut seg_start = 0;
    for awaited in &awaits {
        let mut declared = NameDecls::default();
        for stmt in &body.statements[seg_start..awaited.stmt_index] {
            declared.visit_statement(stmt);
        }
        let mut refs = NameRefs::default();
        for stmt in &body.statements[awaited.stmt_index + 1..] {
            refs.visit_statement(stmt);
        }
        for name in &awaited_names.names {
            refs.names.remove(name);
        }
        if refs.names.iter().any(|name| declared.names.contains(name)) {
            return Err("segment locals used after `await`");
        }
        seg_start = awaited.stmt_index + 1;
    }
    Ok(AsyncPlan { awaits, ret_index: ret_index - 1 })
}

/// Whether `body` contains any JSX: only component-like functions warn or rewrite.
fn has_jsx(body: &FunctionBody<'_>) -> bool {
    let mut check = HasJsx { found: false };
    check.visit_function_body(body);
    check.found
}

struct HasJsx {
    found: bool,
}

impl<'a> Visit<'a> for HasJsx {
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found = true;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found = true;
    }
}

/// Any `await` outside nested function boundaries.
struct AwaitCheck {
    found: bool,
}

impl<'a> Visit<'a> for AwaitCheck {
    fn visit_await_expression(&mut self, _: &AwaitExpression<'a>) {
        self.found = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Any `return`/`throw` outside nested function boundaries.
struct RetCheck {
    found: bool,
}

impl<'a> Visit<'a> for RetCheck {
    fn visit_return_statement(&mut self, _: &ReturnStatement<'a>) {
        self.found = true;
    }

    fn visit_throw_statement(&mut self, _: &ThrowStatement<'a>) {
        self.found = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Binding names a statement list declares; nested function bodies are their own scope.
#[derive(Default)]
struct NameDecls {
    names: HashSet<String>,
}

impl<'a> Visit<'a> for NameDecls {
    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.names.insert(it.name.to_string());
    }

    fn visit_function(&mut self, it: &Function<'a>, _: ScopeFlags) {
        if let Some(id) = &it.id {
            self.names.insert(id.name.to_string());
        }
        self.visit_formal_parameters(&it.params);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.visit_formal_parameters(&it.params);
    }
}

/// Every identifier a statement list reads (closures included: they capture suffix scope).
#[derive(Default)]
struct NameRefs {
    names: HashSet<String>,
}

impl<'a> Visit<'a> for NameRefs {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.names.insert(it.name.to_string());
    }
}


/// The text a literal child renders as.
fn static_text(e: &Expression<'_>) -> Option<String> {
    match unparen(e) {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => format_number(n.value),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked.as_ref()).map(|c| c.to_string())
        }
        // `"a" + "b"` and mixes with static numbers fold (C08). `+` concatenates
        // only when an operand is a string, so both-numbers bail: `1 + 2` must
        // stay dynamic rather than fold to the wrong `"12"`.
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            match (string_value(&b.left), string_value(&b.right)) {
                (Some(l), Some(r)) => Some(format!("{l}{r}")),
                (Some(l), None) => static_text(&b.right).map(|r| format!("{l}{r}")),
                (None, Some(r)) => static_text(&b.left).map(|l| format!("{l}{r}")),
                (None, None) => None,
            }
        }
        _ => None,
    }
}

/// A statically known string: literals, expression-free templates, and `+`
/// chains of those (C08). Numbers are deliberately excluded — see above.
fn string_value(e: &Expression<'_>) -> Option<String> {
    match unparen(e) {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().and_then(|q| q.value.cooked.as_ref()).map(|c| c.to_string())
        }
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            Some(format!("{}{}", string_value(&b.left)?, string_value(&b.right)?))
        }
        _ => None,
    }
}

/// Common HTML/SVG attributes plus the framework's own, for near-miss suggestions (D02).
/// Anything absent here simply never suggests — including `data-*`/`aria-*`, which are
/// filtered before lookup.
const KNOWN_ATTRIBUTES: &[&str] = &[
    "abbr", "accept", "action", "align", "alt", "as", "async", "autoplay", "charset", "checked",
    "cite", "class", "className", "classList", "cols", "colspan", "content", "controls", "coords",
    "crossorigin", "cx", "cy", "d", "datetime", "decoding", "default", "defer", "disabled", "download",
    "draggable", "enctype", "fill", "for", "form", "formaction", "headers", "height", "hidden", "href",
    "hreflang", "id", "ismap", "kind", "label", "lang", "loading", "loop", "max", "maxlength",
    "media", "method", "min", "minlength", "multiple", "muted", "name", "nonce", "open", "pattern",
    "ping", "placeholder", "playsinline", "poster", "preload", "r", "readonly", "referrerpolicy", "rel",
    "required", "rev", "rows", "rowspan", "rx", "ry", "sandbox", "scope", "selected", "shape", "size",
    "sizes", "slot", "span", "spellcheck", "src", "srcdoc", "srclang", "srcset", "start", "step",
    "style", "tabindex", "target", "title", "translate", "type", "usemap", "value", "viewBox",
    "width", "wrap", "x", "x1", "x2", "y", "y1", "y2",
];

/// The closest known attribute within edit distance 2, if any (D02).
fn suggest_attribute(name: &str) -> Option<&'static str> {
    if name.is_empty() || KNOWN_ATTRIBUTES.contains(&name) {
        return None;
    }
    let mut best: Option<(&'static str, usize)> = None;
    for candidate in KNOWN_ATTRIBUTES {
        let distance = edit_distance(name, candidate, 2);
        if distance <= 2 && best.is_none_or(|(_, d)| distance < d) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(candidate, _)| candidate)
}

/// Levenshtein distance capped at `limit + 1`: anything costlier reports `limit + 1`.
fn edit_distance(a: &str, b: &str, limit: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            row_min = row_min.min(curr[j + 1]);
        }
        if row_min > limit {
            return limit + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// `style={{...}}` with all-static values folds to a static `style` attribute (C09).
/// Keys serialize verbatim: the runtime object path hands them to
/// `CSSStyleDeclaration.setProperty` unchanged, so this matches exactly.
/// Empty objects and anything dynamic fall through to the runtime (`None`).
fn static_style_object(e: &Expression<'_>) -> Option<String> {
    let Expression::ObjectExpression(obj) = unparen(e) else { return None };
    if obj.properties.is_empty() {
        return None;
    }
    let mut out = String::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else { return None };
        if p.computed || !matches!(p.kind, PropertyKind::Init) {
            return None;
        }
        let key = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.to_string(),
            PropertyKey::StringLiteral(s) => s.value.to_string(),
            _ => return None,
        };
        let value = static_text(&p.value)?;
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(&key);
        out.push(':');
        out.push_str(&value);
    }
    Some(out)
}

/// `classList={{...}}` with all-literal values folds to a static `class`
/// attribute (C10), mirroring `classListToObject`: truthy values keep the key's
/// whitespace-separated tokens, falsy ones drop them. An all-falsy object folds
/// to no attribute; anything dynamic falls through (`None`).
fn static_class_list(e: &Expression<'_>) -> Option<String> {
    let Expression::ObjectExpression(obj) = unparen(e) else { return None };
    if obj.properties.is_empty() {
        return None;
    }
    let mut out = String::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else { return None };
        if p.computed || !matches!(p.kind, PropertyKind::Init) {
            return None;
        }
        let key = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.to_string(),
            PropertyKey::StringLiteral(s) => s.value.to_string(),
            _ => return None,
        };
        if !literal_truthy(&p.value)? {
            continue;
        }
        for token in key.split_whitespace() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(token);
        }
    }
    Some(out)
}

/// Whether a `classList` value is statically truthy; `None` when not statically
/// known. Mirrors JS truthiness for the literal shapes `literal()`/`static_text()`
/// cover — note `0` is falsy while `"0"` and `" "` are truthy (C10).
fn literal_truthy(e: &Expression<'_>) -> Option<bool> {
    match unparen(e) {
        Expression::BooleanLiteral(b) => Some(b.value),
        Expression::NullLiteral(_) => Some(false),
        Expression::Identifier(id) if id.name.as_str() == "undefined" => Some(false),
        Expression::NumericLiteral(n) => Some(n.value != 0.0 && !n.value.is_nan()),
        _ => static_text(e).map(|s| !s.is_empty()),
    }
}

fn literal(e: &Expression<'_>) -> Option<Lit> {
    if let Some(s) = static_text(e) {
        return Some(Lit::Str(s));
    }
    match unparen(e) {
        Expression::BooleanLiteral(b) => Some(Lit::Bool(b.value)),
        Expression::NullLiteral(_) => Some(Lit::Null),
        Expression::Identifier(id) if id.name.as_str() == "undefined" => Some(Lit::Null),
        _ => None,
    }
}

/// `String(n)` for integers; other numbers are left to the runtime.
fn format_number(n: f64) -> Option<String> {
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15).then(|| format!("{}", n as i64))
}

/// Whether evaluating `e` may read reactive state: it calls something or reads a property.
/// Function bodies are not evaluated, so they never count.
fn is_dynamic(e: &Expression<'_>, member: bool, tags: bool) -> bool {
    let mut check = DynamicCheck { member, tags, found: false };
    check.visit_expression(e);
    check.found
}

struct DynamicCheck {
    member: bool,
    tags: bool,
    found: bool,
}

impl<'a> Visit<'a> for DynamicCheck {
    fn visit_call_expression(&mut self, _: &CallExpression<'a>) {
        self.found = true;
    }

    fn visit_tagged_template_expression(&mut self, _: &TaggedTemplateExpression<'a>) {
        self.found = true;
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if self.member {
            self.found = true;
        } else {
            walk::walk_static_member_expression(self, it);
        }
    }

    fn visit_computed_member_expression(&mut self, it: &ComputedMemberExpression<'a>) {
        if self.member {
            self.found = true;
        } else {
            walk::walk_computed_member_expression(self, it);
        }
    }

    fn visit_private_field_expression(&mut self, it: &PrivateFieldExpression<'a>) {
        if self.member {
            self.found = true;
        } else {
            walk::walk_private_field_expression(self, it);
        }
    }

    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {
        self.found |= self.tags;
    }

    fn visit_jsx_fragment(&mut self, _: &JSXFragment<'a>) {
        self.found |= self.tags;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Compiles each outermost JSX expression it meets.
struct Finder<'t, 's> {
    t: &'t mut Transformer<'s>,
    found: Vec<(Span, Code)>,
}

impl<'a> Visit<'a> for Finder<'_, '_> {
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        let code = self.t.element(it);
        self.found.push((it.span, code));
    }

    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        let code = self.t.fragment(it);
        self.found.push((it.span, code));
    }

    /// Single-await `async` functions rewrite whole; anything else walks through so
    /// nested JSX still compiles. Skipping the descent on rewrite keeps spans disjoint.
    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if let Some(code) = self.t.async_function(it) {
            self.found.push((it.span, code));
        } else {
            walk::walk_function(self, it, flags);
        }
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let Some(code) = self.t.async_arrow(it) {
            self.found.push((it.span, code));
        } else {
            walk::walk_arrow_function_expression(self, it);
        }
    }
}

#[derive(Default)]
struct NameCollector {
    names: HashSet<String>,
}

impl<'a> Visit<'a> for NameCollector {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        self.names.insert(it.name.to_string());
    }

    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.names.insert(it.name.to_string());
    }
}
