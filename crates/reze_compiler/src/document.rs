use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionBody, BindingPattern, ExportDefaultDeclarationKind, Expression, FormalParameters,
    Function, ImportDeclarationSpecifier, ImportOrExportKind, JSXAttributeItem, JSXAttributeName,
    JSXAttributeValue, JSXChild, JSXElement, JSXElementName, PropertyKey, Statement,
};
use oxc_span::{GetSpan, SourceType, Span};

use crate::diagnostic::{self, Code, Diagnostic, Report};
use crate::html::{clean_jsx_text, decode_entities, escape_text, is_svg_element, is_void};

const DEFAULT_ROOT_ID: &str = "app";

/// A document module rendered to HTML.
pub struct Document {
    /// The whole page, `<!doctype html>` included, with an empty root element.
    pub html: String,
    /// `id` of the element that holds the app.
    pub root_id: String,
}

/// Renders the default-exported document component of `source` to static HTML. `Err` holds every
/// diagnostic when the module is not a static `<html>` page with exactly one children slot.
pub fn compile_document(source: &str, filename: &str) -> Result<Document, Vec<Diagnostic>> {
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::tsx());
    let allocator = Allocator::default();
    let program = crate::parse_program(&allocator, source, filename, source_type)?;
    let mut renderer = Renderer::default();
    let mut component = None;
    for statement in &program.body {
        match statement {
            Statement::ExportDefaultDeclaration(export) if component.is_none() => {
                component = Some(export);
            }
            statement if is_type_only(statement) => {}
            statement => renderer.shape(
                statement.span(),
                "has a runtime statement next to its default export; keep only type imports and type declarations",
            ),
        }
    }
    match component {
        Some(export) => renderer.component(&export.declaration, export.span),
        None => renderer.shape(Span::empty(0), "has no default export"),
    }
    renderer.finish(source, filename)
}

fn is_type_only(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ImportDeclaration(import) => {
            import.import_kind == ImportOrExportKind::Type
                || import.specifiers.as_ref().is_some_and(|specifiers| {
                    !specifiers.is_empty()
                        && specifiers.iter().all(|specifier| {
                            matches!(specifier, ImportDeclarationSpecifier::ImportSpecifier(s) if s.import_kind == ImportOrExportKind::Type)
                        })
                })
        }
        Statement::ExportNamedDeclaration(export) => export.export_kind == ImportOrExportKind::Type,
        Statement::ExportDeclaration(export) => export.declaration.is_typescript_syntax(),
        Statement::TSInterfaceDeclaration(_) | Statement::TSTypeAliasDeclaration(_) => true,
        _ => false,
    }
}

enum Slot<'a> {
    Absent,
    Member(&'a str),
    Binding(&'a str),
}

#[derive(Default)]
struct Renderer {
    html: String,
    reports: Vec<Report>,
    root: Option<(String, Option<Span>)>,
    slots: u32,
    ids: Vec<(String, Span)>,
}

impl Renderer {
    fn shape(&mut self, span: Span, detail: &str) {
        self.reports.push(Report::new(Code::DocumentShape, span).arg("detail", detail));
    }

    fn dynamic(&mut self, span: Span, detail: &str) {
        self.reports.push(Report::new(Code::DocumentDynamic, span).arg("detail", detail));
    }

    fn misplaced_root(&mut self, span: Span, detail: &str) {
        self.reports.push(Report::new(Code::DocumentRoot, span).arg("detail", detail));
    }

    fn component(&mut self, declaration: &ExportDefaultDeclarationKind<'_>, span: Span) {
        let returned = match declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                self.function(function).map(|body| (body, &function.params))
            }
            ExportDefaultDeclarationKind::FunctionExpression(function) => {
                self.function(function).map(|body| (body, &function.params))
            }
            ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                if arrow.r#async {
                    self.shape(arrow.span, "is async; it must return its page synchronously");
                    None
                } else {
                    let body = match &arrow.body {
                        ArrowFunctionBody::FunctionBody(body) => {
                            self.single_return(&body.statements, body.span)
                        }
                        body => body.as_expression(),
                    };
                    body.map(|body| (body, &arrow.params))
                }
            }
            _ => {
                self.shape(span, "default-exports something other than a function");
                None
            }
        };
        let Some((body, params)) = returned else { return };
        let slot = slot_of(params);
        match unparenthesized(body) {
            Expression::JSXElement(element)
                if tag_name(&element.opening_element.name) == Some("html") =>
            {
                self.html.push_str("<!doctype html>");
                self.page(element, &slot);
            }
            other => self.shape(other.span(), "returns something other than an `<html>` element"),
        }
        if self.slots == 0 {
            self.misplaced_root(
                span,
                "never renders its `children`; place them in the element the app renders into",
            );
        }
        if let Some((id, own)) = &self.root {
            let duplicates: Vec<Span> = self
                .ids
                .iter()
                .filter(|(other, span)| other == id && Some(*span) != *own)
                .map(|(_, span)| *span)
                .collect();
            for duplicate in duplicates {
                self.misplaced_root(duplicate, "gives another element the root element's `id`");
            }
        }
    }

    fn function<'b>(&mut self, function: &'b Function<'_>) -> Option<&'b Expression<'b>> {
        if function.r#async || function.generator {
            self.shape(
                function.span,
                "is async or a generator; it must return its page synchronously",
            );
            return None;
        }
        let body = function.body.as_ref()?;
        self.single_return(&body.statements, body.span)
    }

    fn single_return<'b>(
        &mut self,
        statements: &'b [Statement<'b>],
        span: Span,
    ) -> Option<&'b Expression<'b>> {
        if let [Statement::ReturnStatement(statement)] = statements
            && let Some(argument) = &statement.argument
        {
            return Some(argument);
        }
        self.shape(span, "has statements besides `return <html>…</html>`");
        None
    }

    fn page(&mut self, html: &JSXElement<'_>, slot: &Slot<'_>) {
        self.open(html, "html");
        let parts: Vec<&JSXElement<'_>> = html
            .children
            .iter()
            .filter(|child| is_meaningful(child))
            .filter_map(|child| match child {
                JSXChild::Element(element) => Some(element.as_ref()),
                other => {
                    self.shape(other.span(), "`<html>` holds only `<head>` and `<body>`");
                    None
                }
            })
            .collect();
        let tags: Vec<Option<&str>> =
            parts.iter().map(|part| tag_name(&part.opening_element.name)).collect();
        if tags != [Some("head"), Some("body")] {
            self.shape(html.opening_element.span, "`<html>` holds `<head>` and then `<body>`");
        }
        for part in parts {
            self.element(
                part,
                slot,
                part.opening_element.name.span(),
                tag_name(&part.opening_element.name) == Some("body"),
            );
        }
        self.html.push_str("</html>");
    }

    fn element(&mut self, element: &JSXElement<'_>, slot: &Slot<'_>, span: Span, in_body: bool) {
        let Some(tag) = tag_name(&element.opening_element.name) else {
            self.dynamic(span, "a component");
            return;
        };
        let meaningful: Vec<&JSXChild<'_>> =
            element.children.iter().filter(|child| is_meaningful(child)).collect();
        if let [JSXChild::ExpressionContainer(container)] = meaningful.as_slice()
            && container
                .expression
                .as_expression()
                .is_some_and(|expression| is_slot(expression, slot))
        {
            self.root_element(element, tag, container.span, in_body);
            return;
        }
        self.open(element, tag);
        if is_void(tag) {
            if !meaningful.is_empty() {
                self.shape(element.span, "gives a void element children");
            }
            return;
        }
        let raw = matches!(tag, "script" | "style");
        let text_start = self.html.len();
        for child in &element.children {
            self.child(child, slot, raw, in_body);
        }
        if raw {
            let closing = format!("</{tag}");
            if self.html[text_start..].to_ascii_lowercase().contains(&closing) {
                self.shape(element.span, "puts the closing tag text inside a raw-text element");
            }
        }
        self.html.push_str("</");
        self.html.push_str(tag);
        self.html.push('>');
    }

    fn root_element(
        &mut self,
        element: &JSXElement<'_>,
        tag: &str,
        slot_span: Span,
        in_body: bool,
    ) {
        self.slots += 1;
        if self.slots > 1 {
            self.misplaced_root(slot_span, "renders its `children` more than once");
        }
        if !in_body {
            self.misplaced_root(slot_span, "renders its `children` outside `<body>`");
        } else if matches!(
            tag,
            "body" | "script" | "style" | "title" | "textarea" | "template" | "noscript"
        ) || is_void(tag)
            || tag == "svg"
            || tag == "math"
            || is_svg_element(tag)
        {
            self.misplaced_root(
                slot_span,
                "renders its `children` directly in a `<body>`, raw-text, template or SVG element; wrap them in an element such as `<div>`",
            );
        }
        let root = match self.open(element, tag) {
            Some((id, span)) => (id, Some(span)),
            None => {
                self.html.insert_str(self.html.len() - 1, &format!(" id={DEFAULT_ROOT_ID}"));
                (DEFAULT_ROOT_ID.to_string(), None)
            }
        };
        self.root.get_or_insert(root);
        self.html.push_str("</");
        self.html.push_str(tag);
        self.html.push('>');
    }

    fn child(&mut self, child: &JSXChild<'_>, slot: &Slot<'_>, raw: bool, in_body: bool) {
        match child {
            JSXChild::Text(text) => {
                let cleaned = clean_jsx_text(&decode_entities(text.value.as_str()));
                self.text(&cleaned, raw);
            }
            JSXChild::Element(element) => {
                if raw {
                    self.shape(element.span, "puts an element inside a raw-text element");
                    return;
                }
                let span = element.opening_element.name.span();
                self.element(element, slot, span, in_body);
            }
            JSXChild::Fragment(fragment) => {
                for child in &fragment.children {
                    self.child(child, slot, raw, in_body);
                }
            }
            JSXChild::ExpressionContainer(container) => {
                let Some(expression) = container.expression.as_expression() else { return };
                if is_slot(expression, slot) {
                    self.slots += 1;
                    self.misplaced_root(
                        container.span,
                        "renders its `children` next to other content; give them an element of their own",
                    );
                    return;
                }
                match static_value(expression) {
                    Some(StaticValue::Text(text)) => self.text(&text, raw),
                    Some(StaticValue::Flag(_)) => {}
                    None => self.dynamic(container.span, "an expression"),
                }
            }
            JSXChild::Spread(spread) => self.dynamic(spread.span, "a spread child"),
        }
    }

    fn text(&mut self, text: &str, raw: bool) {
        if raw {
            self.html.push_str(text);
        } else {
            escape_text(&mut self.html, text);
        }
    }

    /// Writes the opening tag; returns its literal `id` and the attribute's span.
    fn open(&mut self, element: &JSXElement<'_>, tag: &str) -> Option<(String, Span)> {
        self.html.push('<');
        self.html.push_str(tag);
        let mut id = None;
        for item in &element.opening_element.attributes {
            let attribute = match item {
                JSXAttributeItem::Attribute(attribute) => attribute,
                JSXAttributeItem::SpreadAttribute(spread) => {
                    self.dynamic(spread.span, "a spread attribute");
                    continue;
                }
            };
            let name = match &attribute.name {
                JSXAttributeName::Identifier(name) => name.name.to_string(),
                JSXAttributeName::NamespacedName(name) => {
                    format!("{}:{}", name.namespace.name, name.name.name)
                }
            };
            let value = match &attribute.value {
                None => StaticValue::Flag(true),
                Some(JSXAttributeValue::StringLiteral(literal)) => {
                    StaticValue::Text(decode_entities(literal.value.as_str()).into_owned())
                }
                Some(JSXAttributeValue::ExpressionContainer(container)) => {
                    match container.expression.as_expression().and_then(static_value) {
                        Some(value) => value,
                        None => {
                            self.dynamic(container.span, "an attribute expression");
                            continue;
                        }
                    }
                }
                Some(other) => {
                    self.dynamic(other.span(), "an element-valued attribute");
                    continue;
                }
            };
            match value {
                StaticValue::Flag(false) => {}
                StaticValue::Flag(true) => {
                    self.html.push(' ');
                    self.html.push_str(&name);
                }
                StaticValue::Text(text) => {
                    self.html.push(' ');
                    self.html.push_str(&name);
                    crate::html::push_attribute_value(&mut self.html, &text);
                    if name == "id" {
                        self.ids.push((text.clone(), attribute.span));
                        id = Some((text, attribute.span));
                    }
                }
            }
        }
        self.html.push('>');
        id
    }

    fn finish(self, source: &str, filename: &str) -> Result<Document, Vec<Diagnostic>> {
        if !self.reports.is_empty() {
            return Err(diagnostic::resolve(self.reports, source, filename));
        }
        let (root_id, _) = self.root.expect("a document without errors has a root");
        Ok(Document { html: self.html, root_id })
    }
}

enum StaticValue {
    Text(String),
    Flag(bool),
}

fn static_value(expression: &Expression<'_>) -> Option<StaticValue> {
    match unparenthesized(expression) {
        Expression::StringLiteral(literal) => Some(StaticValue::Text(literal.value.to_string())),
        Expression::NumericLiteral(literal) => Some(StaticValue::Text(literal.value.to_string())),
        Expression::BooleanLiteral(literal) => Some(StaticValue::Flag(literal.value)),
        Expression::TemplateLiteral(literal) if literal.expressions.is_empty() => literal
            .quasis
            .first()?
            .value
            .cooked
            .as_ref()
            .map(|cooked| StaticValue::Text(cooked.to_string())),
        _ => None,
    }
}

fn unparenthesized<'b, 'a>(mut expression: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(inner) = expression {
        expression = &inner.expression;
    }
    expression
}

fn slot_of<'a>(params: &'a FormalParameters<'a>) -> Slot<'a> {
    let Some(first) = params.items.first() else { return Slot::Absent };
    match &first.pattern {
        BindingPattern::BindingIdentifier(identifier) => Slot::Member(identifier.name.as_str()),
        BindingPattern::ObjectPattern(object) => object
            .properties
            .iter()
            .find_map(|property| match (&property.key, &property.value) {
                (PropertyKey::StaticIdentifier(key), BindingPattern::BindingIdentifier(local))
                    if key.name == "children" && !property.computed =>
                {
                    Some(Slot::Binding(local.name.as_str()))
                }
                _ => None,
            })
            .unwrap_or(Slot::Absent),
        _ => Slot::Absent,
    }
}

fn is_slot(expression: &Expression<'_>, slot: &Slot<'_>) -> bool {
    match (unparenthesized(expression), slot) {
        (Expression::StaticMemberExpression(member), Slot::Member(props)) => {
            !member.optional
                && member.property.name == "children"
                && matches!(&member.object, Expression::Identifier(object) if object.name == *props)
        }
        (Expression::Identifier(identifier), Slot::Binding(children)) => {
            identifier.name == *children
        }
        _ => false,
    }
}

fn tag_name<'a>(name: &'a JSXElementName<'a>) -> Option<&'a str> {
    match name {
        JSXElementName::Identifier(identifier) => Some(identifier.name.as_str()),
        _ => None,
    }
}

fn is_meaningful(child: &JSXChild<'_>) -> bool {
    match child {
        JSXChild::Text(text) => !clean_jsx_text(&decode_entities(text.value.as_str())).is_empty(),
        JSXChild::ExpressionContainer(container) => container.expression.as_expression().is_some(),
        _ => true,
    }
}
