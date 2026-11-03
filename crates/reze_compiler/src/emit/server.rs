use std::collections::BTreeSet;

use oxc_ast::ast::{
    CallExpression, Expression, ExpressionStatement, ImportDeclaration, ImportDeclarationSpecifier,
    Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};

use crate::SERVER_RUNTIME;
use crate::analyze::{Analysis, Api, EndpointKind};
use crate::client::{self, ModulePlan, base36, scope_var};
use crate::html;
use crate::project::Module;
use crate::view::{
    AttributeItem, Branch, Call, Callback, Content, Element, Node, PropValue, Value,
};

#[derive(Clone, Copy)]
enum Wrap {
    /// A template literal that evaluates to a string.
    Template,
    /// `$h(…)`: markup used as a value, which `$t` renders unescaped.
    Raw,
}

enum Event<'e, 'a> {
    Edit(String),
    Jsx(&'e Content<'a>, Wrap),
}

struct Region<'e, 'a> {
    span: Span,
    event: Event<'e, 'a>,
}

struct Emitter<'e, 'a> {
    source: &'a str,
    regions: Vec<Region<'e, 'a>>,
    helpers: BTreeSet<&'static str>,
    plan: &'e ModulePlan,
}

fn template_escape(out: &mut String, s: &str) {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            _ => out.push(c),
        }
    }
}

pub(crate) fn head_key(element: &Element<'_>) -> Option<String> {
    let attribute = |name: &str| {
        element.attributes.iter().find_map(|item| match item {
            AttributeItem::Attribute(a) if a.name == name => Some(&a.value),
            _ => None,
        })
    };
    let is = |name: &str, expected: &str| matches!(attribute(name), Some(Value::Static(value)) if value == expected);
    match element.tag {
        "title" | "base" => Some(element.tag.to_string()),
        "link" if is("rel", "canonical") => Some("canonical".into()),
        "meta" if attribute("charset").is_some() => Some("meta:charset".into()),
        "meta" => {
            ["name", "property", "http-equiv"].iter().find_map(|name| match attribute(name) {
                Some(Value::Static(value)) => Some(format!("meta:{name}={value}")),
                _ => None,
            })
        }
        _ => None,
    }
}

impl<'e, 'a> Emitter<'e, 'a> {
    fn text(&self, span: Span) -> &'a str {
        let source: &'a str = self.source;
        &source[span.start as usize..span.end as usize]
    }

    fn helper(&mut self, name: &'static str) -> &'static str {
        self.helpers.insert(name);
        name
    }

    /// The source of `span`, with every edit and JSX region inside it applied.
    fn print(&mut self, span: Span) -> String {
        let mut out = String::new();
        let mut position = span.start;
        let mut index = self.regions.partition_point(|r| r.span.start < span.start);
        while index < self.regions.len() {
            let region = &self.regions[index];
            index += 1;
            let at_end = region.span.is_empty() && region.span.start == span.end;
            if region.span.start >= span.end && !at_end {
                break;
            }
            if region.span.start < position || region.span.end > span.end {
                continue;
            }
            let region_span = region.span;
            let event = match &region.event {
                Event::Edit(text) => Err(text.clone()),
                Event::Jsx(content, wrap) => Ok((*content, *wrap)),
            };
            out.push_str(self.text(Span::new(position, region_span.start)));
            position = region_span.end;
            match event {
                Err(text) => out.push_str(&text),
                Ok((content, Wrap::Template)) => out.push_str(&self.template(content)),
                Ok((content, Wrap::Raw)) => {
                    let body = self.template(content);
                    let h = self.helper("$h");
                    out.push_str(&format!("{h}({body})"));
                }
            }
        }
        out.push_str(self.text(Span::new(position, span.end)));
        out
    }

    fn template(&mut self, content: &Content<'a>) -> String {
        let mut out = String::from("`");
        self.nodes(&content.nodes, content.span.start, &mut out);
        out.push('`');
        let key = content.span.start;
        if let Some(scope) = self.plan.components.get(&key) {
            let captures = scope.captures.join(", ");
            return format!(
                "(($i) => {out})($r.scope({}, {:?}, [{captures}], {}))",
                scope.id, scope.events, scope.eager
            );
        }
        if let Some(row) = self.plan.rows.get(&key) {
            let captures = row.captures.join(", ");
            return format!(
                "(({}) => {out})($r.row({}, {}, [{captures}]))",
                scope_var(row.scope),
                scope_var(row.parent),
                row.scope
            );
        }
        out
    }

    fn expression(&mut self, expression: &Expression<'a>) -> String {
        self.print(expression.span())
    }

    /// `key` is the span start of the element or content that holds `nodes`.
    fn nodes(&mut self, nodes: &[Node<'a>], key: u32, out: &mut String) {
        let pieces: Vec<_> = nodes.iter().map(|n| client::piece(n, &self.plan.bindings)).collect();
        let separators = client::separators(&pieces);
        for (index, node) in nodes.iter().enumerate() {
            if let Some(&(scope, mark)) = self.plan.anchors.get(&(key, index)) {
                out.push_str(&format!("<!--${{{}}}.{}-->", scope_var(scope), base36(mark)));
            }
            if separators[index] {
                out.push_str("<!>");
            }
            self.node(node, out);
        }
    }

    fn interpolate(&mut self, out: &mut String, code: &str) {
        out.push_str("${");
        out.push_str(code);
        out.push('}');
    }

    fn node(&mut self, node: &Node<'a>, out: &mut String) {
        match node {
            Node::Text(text) => {
                let mut escaped = String::new();
                html::escape_text(&mut escaped, text);
                template_escape(out, &escaped);
            }
            Node::Expr(expression) => {
                let t = self.helper("$t");
                let code = format!("{t}({})", self.expression(expression));
                self.interpolate(out, &code);
            }
            Node::Element(element) => self.element(element, out),
            Node::Head(elements) => {
                for element in elements {
                    let mut html = String::from("`");
                    self.element(element, &mut html);
                    html.push('`');
                    let key = head_key(element).map_or_else(
                        || "null".to_string(),
                        |key| serde_json::to_string(&key).unwrap(),
                    );
                    self.interpolate(out, &format!("$r.head({key}, {html})"));
                }
            }
            Node::Component(call) => {
                let code = self.call(call);
                self.interpolate(out, &code);
            }
            Node::Show(show) => {
                let when = self.expression(show.when);
                let otherwise = self.fallback(show.fallback.as_ref());
                let code = match &show.children {
                    Branch::Content(content) => {
                        format!("({when}) ? {} : {otherwise}", self.template(content))
                    }
                    Branch::Callback(callback) => {
                        let show = self.helper("$show");
                        format!("{show}({when}, {}, () => {otherwise})", self.callback(callback))
                    }
                };
                self.interpolate(out, &code);
            }
            Node::Switch(switch) => {
                let mut code = self.fallback(switch.fallback.as_ref());
                for matched in switch.matches.iter().rev() {
                    let when = self.expression(matched.when);
                    code = match &matched.children {
                        Branch::Content(content) => {
                            format!("({when}) ? {} : {code}", self.template(content))
                        }
                        Branch::Callback(callback) => {
                            let show = self.helper("$show");
                            format!("{show}({when}, {}, () => {code})", self.callback(callback))
                        }
                    };
                }
                self.interpolate(out, &code);
            }
            Node::For(each) | Node::Repeat(each) => {
                let helper =
                    self.helper(if matches!(node, Node::For(_)) { "$for" } else { "$repeat" });
                let source = self.expression(each.source);
                let row = self.callback(&each.children);
                let code = match &each.fallback {
                    Some(fallback) => {
                        let fallback = self.branch(fallback);
                        format!("{helper}({source}, {row}, () => {fallback})")
                    }
                    None => format!("{helper}({source}, {row})"),
                };
                self.interpolate(out, &code);
            }
            Node::Loading(boundary) | Node::Errored(boundary) => {
                let mut children = String::from("`");
                self.nodes(&boundary.children.nodes, boundary.children.span.start, &mut children);
                children.push('`');
                let fallback = match &boundary.fallback {
                    Some(Branch::Content(content)) => format!("() => {}", self.template(content)),
                    Some(Branch::Callback(callback)) => self.callback(callback),
                    None => "() => ''".into(),
                };
                let method = if matches!(node, Node::Loading(_)) { "loading" } else { "boundary" };
                self.interpolate(out, &format!("$r.{method}(() => {children}, {fallback})"));
            }
            Node::Portal(_) => {}
        }
    }

    fn branch(&mut self, branch: &Branch<'a>) -> String {
        match branch {
            Branch::Content(content) => self.template(content),
            Branch::Callback(callback) => format!("({})()", self.callback(callback)),
        }
    }

    fn fallback(&mut self, fallback: Option<&Branch<'a>>) -> String {
        fallback.map_or_else(|| "''".to_string(), |branch| self.branch(branch))
    }

    fn callback(&mut self, callback: &Callback<'a>) -> String {
        self.print(callback.function.span)
    }

    fn element(&mut self, element: &Element<'a>, out: &mut String) {
        out.push('<');
        out.push_str(element.tag);
        for item in &element.attributes {
            match item {
                AttributeItem::Attribute(attribute) => match &attribute.value {
                    Value::Static(value) => {
                        let mut html = format!(" {}", attribute.name);
                        html::push_attribute_value(&mut html, value);
                        template_escape(out, &html);
                    }
                    Value::True => {
                        out.push(' ');
                        out.push_str(attribute.name);
                    }
                    Value::Expr(expression)
                        if element.tag == "form" && attribute.name == "action" =>
                    {
                        let fa = self.helper("$fa");
                        let code = format!("{fa}($r, {})", self.expression(expression));
                        self.interpolate(out, &code);
                    }
                    Value::Expr(expression) => {
                        let a = self.helper("$a");
                        let code =
                            format!("{a}({:?}, {})", attribute.name, self.expression(expression));
                        self.interpolate(out, &code);
                    }
                },
                AttributeItem::Spread(expression) => {
                    let s = self.helper("$s");
                    let code = format!("{s}({})", self.expression(expression));
                    self.interpolate(out, &code);
                }
            }
        }
        match element.active_href() {
            Some(Value::Static(href)) => {
                let ac = self.helper("$ac");
                let code = format!("{ac}($r, {})", serde_json::to_string(href).unwrap());
                self.interpolate(out, &code);
            }
            Some(Value::Expr(expression)) => {
                let ac = self.helper("$ac");
                let code = format!("{ac}($r, {})", self.expression(expression));
                self.interpolate(out, &code);
            }
            _ => {}
        }
        if let Some(mark) = self.plan.marks.get(&element.span.start) {
            out.push_str(&format!(
                " data-rz=${{{}}}.{}",
                scope_var(mark.scope),
                base36(mark.index)
            ));
            if let Some(prevent) = &mark.prevent {
                let mut html = String::from(" data-rz-prevent");
                html::push_attribute_value(&mut html, prevent);
                template_escape(out, &html);
            }
        }
        out.push('>');
        if html::is_void(element.tag) {
            return;
        }
        self.nodes(&element.children, element.span.start, out);
        out.push_str("</");
        out.push_str(element.tag);
        out.push('>');
    }

    fn prop_value(&mut self, value: &PropValue<'a>) -> String {
        match value {
            PropValue::Value(Value::Static(value)) => serde_json::to_string(value).unwrap(),
            PropValue::Value(Value::True) => "true".into(),
            PropValue::Value(Value::Expr(expression)) => self.expression(expression),
            PropValue::Content(content) => {
                let l = self.helper("$l");
                format!("{l}(() => {})", self.template(content))
            }
            PropValue::Spread(expression) => format!("...{}", self.expression(expression)),
        }
    }

    fn call(&mut self, call: &Call<'a>) -> String {
        let mut props = Vec::new();
        for prop in &call.props {
            let value = self.prop_value(&prop.value);
            if matches!(prop.value, PropValue::Spread(_)) {
                props.push(value);
            } else if html::is_identifier_name(prop.name) {
                props.push(format!("{}: {value}", prop.name));
            } else {
                props.push(format!("{:?}: {value}", prop.name));
            }
        }
        if let Some(children) = &call.children {
            let value = self.prop_value(children);
            props.push(format!("children: {value}"));
        }
        let props =
            if props.is_empty() { "{}".to_string() } else { format!("{{ {} }}", props.join(", ")) };
        format!("{}({props}, $r)", call.name)
    }
}

/// Lowers compile-time framework calls and drops imports that only the compiler reads.
struct Lowering<'f, 'a> {
    apis: &'f dyn Fn(&Expression<'a>) -> Option<Api>,
    edits: Vec<(Span, String)>,
}

impl<'a> Visit<'a> for Lowering<'_, 'a> {
    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        if let Expression::CallExpression(call) = it.expression.without_parentheses()
            && matches!((self.apis)(&call.callee), Some(Api::Effect | Api::OnCleanup))
        {
            self.edits.push((it.span, String::new()));
            return;
        }
        walk::walk_expression_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        match (self.apis)(&it.callee) {
            Some(Api::Signal | Api::Computed | Api::Store | Api::Action | Api::Dynamic) => {
                self.edits.push((it.callee.span(), String::new()));
            }
            Some(Api::Untrack) => {
                self.edits.push((it.callee.span(), String::new()));
                self.edits.push((Span::empty(it.span.end), "()".into()));
            }
            Some(Api::CreateUniqueId) => {
                self.edits.push((it.span, "$r.id()".into()));
                return;
            }
            Some(Api::UseParams) => {
                self.edits.push((it.span, "$r.params".into()));
                return;
            }
            Some(Api::UseLocation) => {
                self.edits.push((it.span, "$r.url".into()));
                return;
            }
            Some(Api::UseSearchParams) => {
                self.edits.push((it.span, "$r.url.searchParams".into()));
                return;
            }
            Some(Api::UseLinkState) => {
                self.edits.push((it.callee.span(), "$r.link".into()));
            }
            Some(Api::UseSubmission) => {
                self.edits.push((it.callee.span(), "$r.submission".into()));
            }
            _ => {}
        }
        walk::walk_call_expression(self, it);
    }
}

fn import_edit<'a>(
    declaration: &ImportDeclaration<'a>,
    keep: impl Fn(&ImportDeclarationSpecifier<'a>) -> bool,
) -> Option<String> {
    let specifiers = declaration.specifiers.as_ref()?;
    let kept: Vec<_> = specifiers
        .iter()
        .filter(|s| {
            keep(s)
                && !matches!(s, ImportDeclarationSpecifier::ImportSpecifier(s) if s.import_kind.is_type())
        })
        .collect();
    if kept.len() == specifiers.len() {
        return None;
    }
    if kept.is_empty() {
        return Some(String::new());
    }
    let named: Vec<String> = kept
        .iter()
        .filter_map(|s| match s {
            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                Some(format!("{} as {}", s.imported.name(), s.local.name))
            }
            _ => None,
        })
        .collect();
    let default = kept.iter().find_map(|s| match s {
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => Some(s.local.name.to_string()),
        _ => None,
    });
    let mut parts = Vec::new();
    parts.extend(default);
    if !named.is_empty() {
        parts.push(format!("{{ {} }}", named.join(", ")));
    }
    Some(format!("import {} from {:?};", parts.join(", "), declaration.source.value.as_str()))
}

pub fn server(
    modules: &[Module<'_>],
    analysis: &Analysis<'_>,
    index: usize,
    plan: &ModulePlan,
) -> String {
    let module = &modules[index];
    let facts = &analysis.facts[index];
    let scoping = module.semantic.scoping();
    let api_of = |expression: &Expression<'_>| match expression.without_parentheses() {
        Expression::Identifier(reference) => {
            let symbol = scoping.get_reference(reference.reference_id.get()?).symbol_id()?;
            facts.apis.get(&symbol).copied()
        }
        _ => None,
    };
    let mut lowering = Lowering { apis: &api_of, edits: Vec::new() };
    lowering.visit_program(module.program);

    let mut regions: Vec<Region<'_, '_>> = lowering
        .edits
        .into_iter()
        .map(|(span, text)| Region { span, event: Event::Edit(text) })
        .collect();

    for statement in &module.program.body {
        let Statement::ImportDeclaration(declaration) = statement else { continue };
        let package = declaration.source.value.as_str();
        if module.resolved.contains_key(package) {
            continue;
        }
        let edit = import_edit(declaration, |specifier| {
            let api = specifier.local().symbol_id.get().and_then(|s| facts.apis.get(&s));
            !api.is_some_and(|api| api.is_syntax())
        });
        if let Some(text) = edit {
            let mut span = declaration.span;
            if text.is_empty() {
                let rest = &module.source[span.end as usize..];
                let line = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                if let Some(after) =
                    rest[line..].strip_prefix('\n').or(rest[line..].strip_prefix("\r\n"))
                {
                    span.end = (module.source.len() - after.len()) as u32;
                }
            }
            regions.push(Region { span, event: Event::Edit(text) });
        }
    }

    for component in &facts.components {
        let params = component.function.params();
        let source = module.source;
        let parenthesized = source.as_bytes().get(params.span.start as usize) == Some(&b'(');
        if parenthesized {
            let at = params.span.end - 1;
            let text =
                if params.items.is_empty() && params.rest.is_none() { "$p, $r" } else { ", $r" };
            regions.push(Region { span: Span::empty(at), event: Event::Edit(text.into()) });
        } else {
            regions.push(Region {
                span: Span::empty(params.span.start),
                event: Event::Edit("(".into()),
            });
            regions.push(Region {
                span: Span::empty(params.span.end),
                event: Event::Edit(", $r)".into()),
            });
        }
        if let Some(template) = &component.template {
            regions.push(Region {
                span: component.returned.without_parentheses().span(),
                event: Event::Jsx(template, Wrap::Template),
            });
            collect_callbacks(&template.nodes, &mut regions);
        }
    }
    for content in facts.values.values() {
        regions.push(Region { span: content.span, event: Event::Jsx(content, Wrap::Raw) });
        collect_callbacks(&content.nodes, &mut regions);
    }
    let mut helpers = BTreeSet::new();
    for endpoint in &facts.endpoints {
        let open = match (endpoint.kind, &endpoint.id) {
            (EndpointKind::Mutation, Some(id)) => {
                helpers.insert("$mut");
                format!(", {id:?}, {})", endpoint.meta)
            }
            (EndpointKind::Loader, _) if endpoint.meta != "{}" => {
                helpers.insert("$load");
                format!(", {})", endpoint.meta)
            }
            _ => continue,
        };
        let helper = if endpoint.kind == EndpointKind::Mutation { "$mut(" } else { "$load(" };
        regions.push(Region {
            span: Span::empty(endpoint.call.start),
            event: Event::Edit(helper.into()),
        });
        regions.push(Region { span: Span::empty(endpoint.call.end), event: Event::Edit(open) });
    }
    regions.sort_by_key(|r| (r.span.start, std::cmp::Reverse(r.span.end)));

    if regions.iter().any(|r| matches!(&r.event, Event::Edit(text) if text == "`${$t(")) {
        helpers.insert("$t");
    }
    let mut emitter = Emitter { source: module.source, regions, helpers, plan };
    let body = emitter.print(Span::new(0, module.source.len() as u32));
    if emitter.helpers.is_empty() {
        return body;
    }
    let helpers: Vec<&str> = emitter.helpers.into_iter().collect();
    format!("import {{ {} }} from {SERVER_RUNTIME:?};\n{body}", helpers.join(", "))
}

/// Regions for the returns of control-flow callbacks, which render to strings.
fn collect_callbacks<'e, 'a>(nodes: &'e [Node<'a>], regions: &mut Vec<Region<'e, 'a>>) {
    for node in nodes {
        match node {
            Node::Element(element) => collect_callbacks(&element.children, regions),
            Node::Head(elements) => {
                for element in elements {
                    collect_callbacks(&element.children, regions);
                }
            }
            Node::Component(call) => {
                let children = call.children.iter();
                for value in call.props.iter().map(|p| &p.value).chain(children) {
                    if let PropValue::Content(content) = value {
                        collect_callbacks(&content.nodes, regions);
                    }
                }
            }
            Node::Show(show) => {
                collect_branch(&show.children, regions);
                show.fallback.iter().for_each(|b| collect_branch(b, regions));
            }
            Node::Switch(switch) => {
                for matched in &switch.matches {
                    collect_branch(&matched.children, regions);
                }
                switch.fallback.iter().for_each(|b| collect_branch(b, regions));
            }
            Node::For(each) | Node::Repeat(each) => {
                collect_callback(&each.children, regions);
                each.fallback.iter().for_each(|b| collect_branch(b, regions));
            }
            Node::Loading(boundary) | Node::Errored(boundary) | Node::Portal(boundary) => {
                collect_callbacks(&boundary.children.nodes, regions);
                boundary.fallback.iter().for_each(|b| collect_branch(b, regions));
            }
            Node::Text(_) | Node::Expr(_) => {}
        }
    }
}

fn collect_branch<'e, 'a>(branch: &'e Branch<'a>, regions: &mut Vec<Region<'e, 'a>>) {
    match branch {
        Branch::Content(content) => collect_callbacks(&content.nodes, regions),
        Branch::Callback(callback) => collect_callback(callback, regions),
    }
}

fn collect_callback<'e, 'a>(callback: &'e Callback<'a>, regions: &mut Vec<Region<'e, 'a>>) {
    let content = &callback.content;
    match content.nodes.as_slice() {
        [Node::Expr(expression)] if expression.span() == content.span => {
            regions.push(Region {
                span: Span::empty(content.span.start),
                event: Event::Edit("`${$t(".into()),
            });
            regions.push(Region {
                span: Span::empty(content.span.end),
                event: Event::Edit(")}`".into()),
            });
        }
        _ => {
            regions.push(Region { span: content.span, event: Event::Jsx(content, Wrap::Template) });
            collect_callbacks(&content.nodes, regions);
        }
    }
}
