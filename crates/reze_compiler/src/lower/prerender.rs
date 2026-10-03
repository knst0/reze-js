use std::collections::{HashMap, HashSet};

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;
use serde::Serialize;

use super::children::Item;
use super::constant::{Literal, literal, literal_truthy, static_text};
use super::island::{CalleeTarget, IslandPlan, is_island_attr, member_path, root_object};
use super::{Lowerer, attribute_name, is_component_name, is_native_name};
use crate::analyze::RUNTIME_MODULES;
use crate::html::{decode_entities, escape_text, is_void, push_attribute_value};
use crate::namer::Namer;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Tree {
    Html { html: String, namespace: &'static str },
    Text(String),
    Children(Vec<Tree>),
    Mixed { html: String, holes: Vec<PrerenderHole> },
    Component(ComponentRef),
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrerenderHole {
    pub id: usize,
    pub target: ComponentRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComponentRef {
    pub request: Option<String>,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrerenderComponent {
    pub name: String,
    pub exported: Vec<String>,
    pub tree: Tree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Default)]
pub struct PrerenderModule {
    pub components: Vec<PrerenderComponent>,
    pub roots: Vec<Tree>,
}

impl PrerenderModule {
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.roots.is_empty()
    }
}

fn marker(id: usize) -> String {
    format!("<!--reze{id}-->")
}

fn is_event_attr(name: &str) -> bool {
    if name.starts_with("on:") {
        return true;
    }
    name.len() > 2
        && name.starts_with("on")
        && !name.contains(':')
        && name.as_bytes()[2].is_ascii_uppercase()
}

fn has_island(attributes: &[JSXAttributeItem<'_>]) -> bool {
    attributes.iter().any(|item| match item {
        JSXAttributeItem::Attribute(a) => match &a.name {
            JSXAttributeName::Identifier(id) => is_island_attr(id.name.as_str()),
            JSXAttributeName::NamespacedName(_) => false,
        },
        JSXAttributeItem::SpreadAttribute(_) => false,
    })
}

enum Attr {
    Html { name: String, value: Option<String> },
    Silent,
    Dynamic,
}

enum TagKind {
    Native,
    Component,
    Intrinsic,
}

struct Scanner<'x, 'a, 'f> {
    lower: &'x mut Lowerer<'a, 'f>,
    plan: IslandPlan,
    render_names: HashSet<SymbolId>,
    render_namespaces: HashSet<SymbolId>,
    exports: HashMap<String, Vec<String>>,
    module: PrerenderModule,
    holes: Vec<PrerenderHole>,
}

impl<'x, 'a, 'f> Scanner<'x, 'a, 'f> {
    fn analysis(&self) -> &crate::analyze::Analysis<'f> {
        self.lower.analysis
    }

    fn tag_name<'b>(&self, name: &'b JSXElementName<'a>) -> String {
        match name {
            JSXElementName::Identifier(id) => id.name.as_str().to_string(),
            JSXElementName::IdentifierReference(id) => id.name.as_str().to_string(),
            JSXElementName::NamespacedName(n) => {
                format!("{}:{}", n.namespace.name, n.name.name)
            }
            JSXElementName::MemberExpression(_) | JSXElementName::ThisExpression(_) => {
                String::from("this")
            }
        }
    }

    fn tag_kind<'b>(&self, name: &'b JSXElementName<'a>) -> TagKind {
        if self.analysis().intrinsic(name).is_some() {
            return TagKind::Intrinsic;
        }
        match name {
            JSXElementName::Identifier(id) => {
                if is_native_name(id.name.as_str()) {
                    TagKind::Native
                } else {
                    TagKind::Component
                }
            }
            JSXElementName::IdentifierReference(id) => {
                if is_native_name(id.name.as_str()) {
                    TagKind::Native
                } else {
                    TagKind::Component
                }
            }
            JSXElementName::NamespacedName(_) => TagKind::Native,
            JSXElementName::MemberExpression(_) | JSXElementName::ThisExpression(_) => {
                TagKind::Component
            }
        }
    }

    fn classify_attr(&self, a: &JSXAttribute<'a>, tag: &str) -> Attr {
        let name = attribute_name(self.lower, a);
        if name == "ref" || name.starts_with("prop:") || name == "children" {
            return Attr::Silent;
        }
        if is_event_attr(name) {
            return Attr::Dynamic;
        }
        let boolean_name = name.strip_prefix("bool:").or_else(|| {
            (matches!(&a.value, Some(JSXAttributeValue::ExpressionContainer(_)))
                && ((name == "checked" && tag == "input")
                    || (name == "selected" && tag == "option")))
                .then_some(name)
        });
        if let Some(key) = boolean_name {
            let truthy = match &a.value {
                None => Some(true),
                Some(JSXAttributeValue::StringLiteral(s)) => Some(!s.value.is_empty()),
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    c.expression.as_expression().and_then(|e| literal_truthy(e, self.analysis()))
                }
                _ => None,
            };
            return match truthy {
                Some(true) => Attr::Html { name: key.to_string(), value: None },
                Some(false) => Attr::Silent,
                None => Attr::Dynamic,
            };
        }
        let (key, value) = match name.strip_prefix("attr:") {
            Some(key) => (key.to_string(), &a.value),
            None => (name.to_string(), &a.value),
        };
        match value {
            None => Attr::Html { name: key, value: None },
            Some(JSXAttributeValue::StringLiteral(s)) => {
                Attr::Html { name: key, value: Some(decode_entities(s.value.as_str()).to_string()) }
            }
            Some(JSXAttributeValue::ExpressionContainer(c)) => {
                match c.expression.as_expression().and_then(|e| static_text(e, self.analysis())) {
                    Some(text) => Attr::Html { name: key, value: Some(text) },
                    None => Attr::Dynamic,
                }
            }
            Some(_) => Attr::Dynamic,
        }
    }

    fn child_items<'b>(
        &mut self,
        attributes: &'b [JSXAttributeItem<'a>],
        children: &'b [JSXChild<'a>],
        is_native: bool,
    ) -> Vec<Item<'b, 'a>> {
        let mut items = self.lower.items(children, is_native);
        if items.is_empty() {
            for attr in attributes {
                let JSXAttributeItem::Attribute(a) = attr else { continue };
                if attribute_name(self.lower, a) != "children" {
                    continue;
                }
                let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value else { continue };
                let Some(e) = c.expression.as_expression() else { continue };
                items.push(Item::Expr(e));
            }
        }
        items
    }

    fn root_tree(&mut self, e: &Expression<'a>) -> Option<Tree> {
        match e.without_parentheses() {
            Expression::JSXElement(el) => Some(self.jsx_element_tree(el)),
            Expression::JSXFragment(fragment) => {
                let items = self.child_items(&[], &fragment.children, false);
                Some(Tree::Children(items.into_iter().map(|item| self.item_tree(item)).collect()))
            }
            _ => None,
        }
    }

    fn jsx_element_tree(&mut self, el: &JSXElement<'a>) -> Tree {
        match self.tag_kind(&el.opening_element.name) {
            TagKind::Native => self.element_tree(el),
            TagKind::Component => self.component_element(el),
            TagKind::Intrinsic => self.intrinsic_tree(el),
        }
    }

    fn element_tree(&mut self, el: &JSXElement<'a>) -> Tree {
        self.custom_element(el, &self.tag_name(&el.opening_element.name))
    }

    fn custom_element(&mut self, el: &JSXElement<'a>, tag: &str) -> Tree {
        let mut html = String::from("<") + tag;
        for item in &el.opening_element.attributes {
            match item {
                JSXAttributeItem::SpreadAttribute(spread) => {
                    if let Some(entries) = self.static_spread(&spread.argument) {
                        for (name, value) in entries {
                            html.push(' ');
                            html.push_str(&name);
                            if let Some(value) = value {
                                push_attribute_value(&mut html, &value);
                            }
                        }
                    }
                }
                JSXAttributeItem::Attribute(a) => {
                    if let Attr::Html { name, value } = self.classify_attr(a, tag) {
                        html.push(' ');
                        html.push_str(&name);
                        if let Some(value) = value {
                            push_attribute_value(&mut html, &value);
                        }
                    }
                }
            }
        }
        html.push('>');
        if !is_void(tag) {
            let items = self.child_items(&el.opening_element.attributes, &el.children, true);
            for item in items {
                match item {
                    Item::Text(text) => escape_text(&mut html, &text),
                    Item::Element(element) => {
                        let tree = self.jsx_element_tree(element);
                        self.inline(tree, &mut html);
                    }
                    Item::Fragment(_) => {}
                    Item::Expr(e) => {
                        if let Some(text) = static_text(e, self.analysis()) {
                            escape_text(&mut html, &text);
                        }
                    }
                }
            }
            html.push_str("</");
            html.push_str(tag);
            html.push('>');
        }
        if self.holes.is_empty() {
            Tree::Html { html, namespace: "html" }
        } else {
            Tree::Mixed { html, holes: std::mem::take(&mut self.holes) }
        }
    }
    fn static_spread(&self, e: &Expression<'a>) -> Option<Vec<(String, Option<String>)>> {
        let Expression::ObjectExpression(object) = e.without_parentheses() else { return None };
        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else { return None };
            if !matches!(property.kind, PropertyKind::Init) || property.computed {
                return None;
            }
            let key = match &property.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
                _ => return None,
            };
            match literal(&property.value, self.analysis())? {
                Literal::Str(value) => entries.push((key, Some(value))),
                Literal::Bool(true) => entries.push((key, None)),
                Literal::Bool(false) | Literal::Nullish => {}
            }
        }
        Some(entries)
    }

    fn inline(&mut self, tree: Tree, html: &mut String) {
        match tree {
            Tree::Html { html: inner, .. } => html.push_str(&inner),
            Tree::Text(text) => escape_text(html, &text),
            Tree::Children(children) => {
                for child in children {
                    self.inline(child, html);
                }
            }
            Tree::Mixed { html: inner, holes } => {
                let offset = self.holes.len();
                let mut inner = inner;
                for (index, mut hole) in holes.into_iter().enumerate() {
                    hole.id += offset;
                    inner = inner.replacen(&marker(index), &marker(hole.id), 1);
                    self.holes.push(hole);
                }
                html.push_str(&inner);
            }
            Tree::Component(target) => {
                let id = self.holes.len();
                self.holes.push(PrerenderHole { id, target });
                html.push_str(&marker(id));
            }
            Tree::Empty => {}
        }
    }

    fn component_element(&mut self, el: &JSXElement<'a>) -> Tree {
        let name = &el.opening_element.name;
        let island = has_island(&el.opening_element.attributes);
        let (root, members) = match name {
            JSXElementName::IdentifierReference(id) => (Some(id.as_ref()), Vec::new()),
            JSXElementName::MemberExpression(member) => {
                (root_object(&member.object), member_path(name).unwrap_or_default())
            }
            _ => (None, Vec::new()),
        };
        let target = root
            .and_then(|root| root.reference_id.get())
            .and_then(|reference| self.analysis().scoping.get_reference(reference).symbol_id())
            .map(|symbol| self.plan.callee_target(self.lower.source, symbol, &members));
        match (target, island) {
            (Some(CalleeTarget::Split { .. }), true) => self.island_tree(el),
            (None, true) => self.island_tree(el),
            (
                Some(CalleeTarget::Split { request, export })
                | Some(CalleeTarget::External { request, export }),
                _,
            ) => Tree::Component(ComponentRef { request: Some(request), path: vec![export] }),
            (Some(CalleeTarget::Local), _) if members.is_empty() => {
                let name = root.map(|root| root.name.as_str().to_string()).unwrap_or_default();
                Tree::Component(ComponentRef { request: None, path: vec![name] })
            }
            _ => Tree::Empty,
        }
    }

    fn fallback_tree(&mut self, value: Option<&JSXAttributeValue<'a>>) -> Tree {
        match value {
            None => Tree::Empty,
            Some(JSXAttributeValue::StringLiteral(s)) => {
                Tree::Text(decode_entities(s.value.as_str()).to_string())
            }
            Some(JSXAttributeValue::Element(element)) => self.jsx_element_tree(element),
            Some(JSXAttributeValue::Fragment(_)) => Tree::Empty,
            Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression() {
                Some(Expression::JSXElement(element)) => self.jsx_element_tree(element),
                Some(Expression::JSXFragment(_)) => Tree::Empty,
                Some(e) => match static_text(e, self.analysis()) {
                    Some(text) => Tree::Text(text),
                    None => Tree::Empty,
                },
                None => Tree::Empty,
            },
        }
    }

    fn island_tree(&mut self, el: &JSXElement<'a>) -> Tree {
        for item in &el.opening_element.attributes {
            let JSXAttributeItem::Attribute(a) = item else { continue };
            if !matches!(&a.name, JSXAttributeName::Identifier(id) if id.name.as_str() == "islandFallback")
            {
                continue;
            }
            return self.fallback_tree(a.value.as_ref());
        }
        Tree::Empty
    }

    fn intrinsic_tree(&mut self, el: &JSXElement<'a>) -> Tree {
        let intrinsic = self.analysis().intrinsic(&el.opening_element.name);
        match intrinsic.map(|intrinsic| intrinsic.name()) {
            Some("Loading") => {
                for item in &el.opening_element.attributes {
                    let JSXAttributeItem::Attribute(a) = item else { continue };
                    if !matches!(&a.name, JSXAttributeName::Identifier(id) if id.name.as_str() == "fallback")
                    {
                        continue;
                    }
                    return self.fallback_tree(a.value.as_ref());
                }
                Tree::Empty
            }
            Some("Errored") => {
                let items = self.child_items(&el.opening_element.attributes, &el.children, false);
                Tree::Children(items.into_iter().map(|item| self.item_tree(item)).collect())
            }
            _ => Tree::Empty,
        }
    }

    fn item_tree(&mut self, item: Item<'_, 'a>) -> Tree {
        match item {
            Item::Text(text) => Tree::Text(text),
            Item::Element(el) => self.jsx_element_tree(el),
            Item::Fragment(_) => Tree::Empty,
            Item::Expr(e) => match static_text(e, self.analysis()) {
                Some(text) => Tree::Text(text),
                None => Tree::Empty,
            },
        }
    }

    fn record_body(&mut self, name: String, body: &FunctionBody<'a>) {
        if let Some(tree) = self.block_tree(body) {
            self.module.components.push(PrerenderComponent { name, exported: Vec::new(), tree });
        }
    }

    fn record_default_body(&mut self, body: &FunctionBody<'a>) {
        if let Some(tree) = self.block_tree(body) {
            self.module.components.push(PrerenderComponent {
                name: String::from("default"),
                exported: vec![String::from("default")],
                tree,
            });
        }
    }
}

struct ImportScan {
    render_names: HashSet<SymbolId>,
    render_namespaces: HashSet<SymbolId>,
}

impl<'a> Visit<'a> for ImportScan {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        if !RUNTIME_MODULES.contains(&it.source.value.as_str()) {
            return;
        }
        let Some(specifiers) = &it.specifiers else { return };
        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(spec) => {
                    if imported_name(&spec.imported) == Some("render") {
                        self.render_names.insert(spec.local.symbol_id());
                    }
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                    self.render_namespaces.insert(spec.local.symbol_id());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
            }
        }
    }
}

fn imported_name<'a>(name: &ModuleExportName<'a>) -> Option<&'a str> {
    match name {
        ModuleExportName::IdentifierName(name) => Some(name.name.as_str()),
        ModuleExportName::IdentifierReference(name) => Some(name.name.as_str()),
        ModuleExportName::StringLiteral(_) => None,
    }
}

impl<'x, 'a, 'f> Scanner<'x, 'a, 'f> {
    fn is_render_call(&self, call: &CallExpression<'a>) -> bool {
        match &call.callee {
            Expression::Identifier(id) => id
                .reference_id
                .get()
                .and_then(|reference| self.analysis().scoping.get_reference(reference).symbol_id())
                .is_some_and(|symbol| self.render_names.contains(&symbol)),
            Expression::StaticMemberExpression(member) => match &member.object {
                Expression::Identifier(namespace) => {
                    namespace
                        .reference_id
                        .get()
                        .and_then(|reference| {
                            self.analysis().scoping.get_reference(reference).symbol_id()
                        })
                        .is_some_and(|symbol| self.render_namespaces.contains(&symbol))
                        && member.property.name.as_str() == "render"
                        && !member.optional
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn record_render_arg(&mut self, call: &CallExpression<'a>) {
        let Some(arg) = call.arguments.first().and_then(|arg| arg.as_expression()) else {
            return;
        };
        let tree = match arg.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => self.arrow_tree(arrow),
            Expression::FunctionExpression(function) => {
                function.body.as_ref().and_then(|body| self.block_tree(body))
            }
            jsx @ (Expression::JSXElement(_) | Expression::JSXFragment(_)) => self.root_tree(jsx),
            _ => None,
        };
        if let Some(tree) = tree {
            self.module.roots.push(tree);
        }
    }

    fn arrow_tree(&mut self, arrow: &ArrowFunctionExpression<'a>) -> Option<Tree> {
        match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => self.block_tree(body),
            body => body.as_expression().and_then(|e| self.root_tree(e)),
        }
    }

    fn block_tree(&mut self, body: &FunctionBody<'a>) -> Option<Tree> {
        let mut returns = Returns { scanner: self, trees: Vec::new(), bail: false };
        returns.visit_function_body(body);
        if returns.bail {
            return None;
        }
        match returns.trees.len() {
            1 => returns.trees.pop(),
            _ => None,
        }
    }
}

impl<'x, 'a, 'f> Visit<'a> for Scanner<'x, 'a, 'f> {
    fn visit_declaration(&mut self, it: &Declaration<'a>) {
        if let Declaration::FunctionDeclaration(function) = it {
            if let (Some(id), Some(body)) = (&function.id, &function.body) {
                if is_component_name(id.name.as_str()) {
                    self.record_body(id.name.as_str().to_string(), body);
                }
            }
        }
        walk::walk_declaration(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id {
            if is_component_name(id.name.as_str()) {
                let init = it.init.as_ref().map(|init| init.without_parentheses());
                match init {
                    Some(Expression::FunctionExpression(function)) => {
                        if let Some(body) = &function.body {
                            self.record_body(id.name.as_str().to_string(), body);
                        }
                    }
                    Some(Expression::ArrowFunctionExpression(arrow)) => {
                        if let Some(tree) = self.arrow_tree(arrow) {
                            self.module.components.push(PrerenderComponent {
                                name: id.name.as_str().to_string(),
                                exported: Vec::new(),
                                tree,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        match &it.declaration {
            Declaration::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    self.exports
                        .entry(id.name.as_str().to_string())
                        .or_default()
                        .push(id.name.as_str().to_string());
                }
            }
            Declaration::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        self.exports
                            .entry(id.name.as_str().to_string())
                            .or_default()
                            .push(id.name.as_str().to_string());
                    }
                }
            }
            _ => {}
        }
        walk::walk_export_declaration(self, it);
    }

    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        for specifier in &it.specifiers {
            if let (Some(local), Some(exported)) =
                (exported_name(&specifier.local), exported_name(&specifier.exported))
            {
                self.exports.entry(local).or_default().push(exported);
            }
        }
        walk::walk_export_named_declaration(self, it);
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        match &it.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => match &function.id {
                Some(id) => {
                    if let Some(body) = &function.body {
                        self.record_body(id.name.as_str().to_string(), body);
                    }
                    self.exports
                        .entry(id.name.as_str().to_string())
                        .or_default()
                        .push(String::from("default"));
                }
                None => {
                    if let Some(body) = &function.body {
                        self.record_default_body(body);
                    }
                }
            },
            ExportDefaultDeclarationKind::FunctionExpression(function) => {
                if let Some(body) = &function.body {
                    self.record_default_body(body);
                }
            }
            ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => {
                    self.record_default_body(body);
                }
                body => {
                    if let Some(expression) = body.as_expression() {
                        self.record_default_arrow(expression);
                    }
                }
            },
            ExportDefaultDeclarationKind::Identifier(id) => {
                self.exports
                    .entry(id.name.as_str().to_string())
                    .or_default()
                    .push(String::from("default"));
            }
            _ => {}
        }
        walk::walk_export_default_declaration(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if self.is_render_call(it) {
            self.record_render_arg(it);
        }
        walk::walk_call_expression(self, it);
    }
}

fn exported_name(name: &ModuleExportName<'_>) -> Option<String> {
    imported_name(name).map(String::from)
}

impl<'x, 'a, 'f> Scanner<'x, 'a, 'f> {
    fn record_default_arrow(&mut self, e: &Expression<'a>) {
        if let Some(tree) = self.root_tree(e) {
            self.module.components.push(PrerenderComponent {
                name: String::from("default"),
                exported: vec![String::from("default")],
                tree,
            });
        }
    }
}

struct Returns<'s, 'x, 'a, 'f> {
    scanner: &'s mut Scanner<'x, 'a, 'f>,
    trees: Vec<Tree>,
    bail: bool,
}

impl<'a> Visit<'a> for Returns<'_, '_, 'a, '_> {
    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        match it.argument.as_ref().map(|argument| argument.without_parentheses()) {
            None => {}
            Some(Expression::JSXElement(_) | Expression::JSXFragment(_)) => {
                if let Some(argument) = &it.argument {
                    if let Some(tree) = self.scanner.root_tree(argument) {
                        self.trees.push(tree);
                    }
                }
            }
            Some(Expression::NullLiteral(_)) => {}
            Some(Expression::Identifier(id)) if id.name == "undefined" => {}
            _ => self.bail = true,
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, _: &Class<'a>) {}
}

impl<'a, 'f> Lowerer<'a, 'f> {
    pub(crate) fn prerender_module(&mut self, program: &Program<'a>) -> PrerenderModule {
        let plan = IslandPlan::scan(program, self.analysis.scoping);
        let settings =
            super::Settings { debug_names: false, hot: false, links: self.settings.links };
        let mut child = Lowerer::new(
            self.alloc,
            self.source,
            self.analysis,
            settings,
            Namer::new(self.analysis.scoping),
            Vec::new(),
        );
        let mut imports =
            ImportScan { render_names: HashSet::new(), render_namespaces: HashSet::new() };
        imports.visit_program(program);
        let mut scanner = Scanner {
            lower: &mut child,
            plan,
            render_names: imports.render_names,
            render_namespaces: imports.render_namespaces,
            exports: HashMap::new(),
            module: PrerenderModule::default(),
            holes: Vec::new(),
        };
        scanner.visit_program(program);
        for component in &mut scanner.module.components {
            if component.exported.is_empty() {
                component.exported =
                    scanner.exports.get(&component.name).cloned().unwrap_or_default();
            }
        }
        scanner.module
    }
}
