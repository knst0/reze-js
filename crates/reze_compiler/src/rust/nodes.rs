//! Markup: elements, control flow, and components inlined into the render function.

use std::collections::HashMap;
use std::rc::Rc;

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Expression, FormalParameters, LogicalOperator, Statement};
use oxc_span::{GetSpan, SourceType, Span};

use super::lower::{Sink, set, truthy, view};
use super::{Binding, Emitter, Frame, Ty, Val, literal};
use crate::analyze::Api;
use crate::client::{self, base36};
use crate::emit::head_key;
use crate::html;
use crate::view::{
    AttributeItem, Branch, Call, CallTarget, Callback, ComponentRef, Content, Each, Element, Match,
    Node, PropValue, Value,
};

type Names<'e, 'a> = HashMap<String, Binding<'e, 'a>>;

impl<'e, 'a> Emitter<'e, 'a> {
    fn content(&mut self, content: &'e Content<'a>, frame: &Rc<Frame<'e, 'a>>) {
        self.nodes(&content.nodes, content.span.start, frame);
    }

    /// The Rust variable of a client scope, for a `data-rz` value or an anchor.
    fn scope_id(&mut self, scope: usize, frame: &Rc<Frame<'e, 'a>>) {
        let out = self.out.clone();
        match frame.scope(scope) {
            Some(var) => self.stmt(&format!("{out}.id({var});")),
            None => self.raw("0"),
        }
    }

    /// `key` is the span start of the element or content that holds `nodes`.
    fn nodes(&mut self, nodes: &'e [Node<'a>], key: u32, frame: &Rc<Frame<'e, 'a>>) {
        let plan = &self.plan.modules[frame.module];
        let pieces: Vec<_> = nodes.iter().map(|n| client::piece(n, &plan.bindings)).collect();
        let separators = client::separators(&pieces);
        for (index, node) in nodes.iter().enumerate() {
            if let Some(&(scope, mark)) = self.plan.modules[frame.module].anchors.get(&(key, index))
            {
                self.raw("<!--");
                self.scope_id(scope, frame);
                self.raw(&format!(".{}-->", base36(mark)));
            }
            if separators[index] {
                self.raw("<!>");
            }
            self.node(node, frame);
        }
    }

    fn node(&mut self, node: &'e Node<'a>, frame: &Rc<Frame<'e, 'a>>) {
        match node {
            Node::Text(text) => {
                let mut escaped = String::new();
                html::escape_text(&mut escaped, text);
                self.raw(&escaped);
            }
            Node::Expr(expression) => self.text_hole(expression, frame),
            Node::Element(element) => self.element(element, frame),
            Node::Head(elements) => {
                for element in elements {
                    let var = self.fresh("h");
                    self.stmt(&format!("let mut {var} = ::reze_server::Html::new();"));
                    let out = std::mem::replace(&mut self.out, var.clone());
                    self.element(element, frame);
                    self.flush();
                    self.out = out;
                    let key = head_key(element).map_or_else(
                        || "None".to_string(),
                        |key| format!("Some({})", literal(&key)),
                    );
                    self.stmt(&format!("cx.head({key}, {var});"));
                }
            }
            Node::Component(call) => self.call(call, frame),
            Node::Show(show) => {
                self.when(show.when, &show.children, &[], show.fallback.as_ref(), frame);
            }
            Node::Switch(switch) => match switch.matches.split_first() {
                Some((first, rest)) => {
                    self.when(first.when, &first.children, rest, switch.fallback.as_ref(), frame);
                }
                None => self.fallback(switch.fallback.as_ref(), frame),
            },
            Node::For(each) => self.each(each, frame),
            Node::Repeat(each) => self.repeat(each, frame),
            Node::Loading(boundary) => self.content(&boundary.children, frame),
            Node::Errored(boundary) => {
                let (mark, var, label, result) =
                    (self.fresh("m"), self.fresh("h"), self.fresh("b"), self.fresh("r"));
                self.stmt(&format!("let {mark} = cx.mark();"));
                self.stmt(&format!("let mut {var} = ::reze_server::Html::new();"));
                self.open(&format!(
                    "let {result}: ::std::result::Result<(), ::reze_server::RouteError> = '{label}:"
                ));
                let out = std::mem::replace(&mut self.out, var.clone());
                self.labels.push(label);
                self.content(&boundary.children, frame);
                self.labels.pop();
                self.out = out;
                self.stmt("Ok(())");
                self.indent -= 1;
                self.line("};");
                let error = self.fresh("e");
                let out = self.out.clone();
                self.open(&format!("match {result}"));
                self.line(&format!("Ok(()) => {out}.append({var}),"));
                self.open(&format!("Err({error}) =>"));
                self.line(&format!("cx.restore({mark});"));
                self.line(&format!("cx.fail(&{error});"));
                match &boundary.fallback {
                    Some(Branch::Content(content)) => self.content(content, frame),
                    Some(Branch::Callback(callback)) => {
                        self.callback(callback, vec![Binding::Error(error)], frame);
                    }
                    None => {}
                }
                self.close();
                self.close();
            }
            Node::Portal(_) => {}
        }
    }

    /// A text hole, with `?:`, `&&`, `||`, and `??` lowered to branches.
    fn text_hole(&mut self, expression: &'a Expression<'a>, frame: &Rc<Frame<'e, 'a>>) {
        if let Err(reason) = self.try_text_hole(expression, frame) {
            self.report(frame.module, expression.span(), reason);
        }
    }

    fn try_text_hole(
        &mut self,
        expression: &'a Expression<'a>,
        frame: &Rc<Frame<'e, 'a>>,
    ) -> Result<(), String> {
        match expression {
            Expression::ParenthesizedExpression(inner) => {
                return self.try_text_hole(&inner.expression, frame);
            }
            Expression::ConditionalExpression(conditional) => {
                let test = self.val(&conditional.test, frame)?;
                self.open(&format!("if {}", truthy(&test)));
                self.text_hole(&conditional.consequent, frame);
                self.reopen("else");
                self.text_hole(&conditional.alternate, frame);
                self.close();
                return Ok(());
            }
            Expression::LogicalExpression(logical) => {
                let left = self.val(&logical.left, frame)?;
                match (logical.operator, &left.ty) {
                    (LogicalOperator::Coalesce, Ty::Null) => self.text_hole(&logical.right, frame),
                    (LogicalOperator::Coalesce, Ty::Opt(inner)) => {
                        let x = self.fresh("x");
                        self.open(&format!("if let Some({x}) = {}", left.code));
                        let result = self.output(&Val::new(x, (**inner).clone()), Sink::Text);
                        self.reopen("else");
                        self.text_hole(&logical.right, frame);
                        self.close();
                        result?;
                    }
                    (LogicalOperator::Coalesce, _) => self.output(&left, Sink::Text)?,
                    (operator, _) => {
                        self.open(&format!("if {}", truthy(&left)));
                        let result = if operator == LogicalOperator::And {
                            self.text_hole(&logical.right, frame);
                            self.reopen("else");
                            self.output(&left, Sink::Text)
                        } else {
                            let result = self.output(&left, Sink::Text);
                            self.reopen("else");
                            self.text_hole(&logical.right, frame);
                            result
                        };
                        self.close();
                        result?;
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        match self.lower(expression, frame, false)? {
            Binding::Children => {
                let out =
                    if self.out == "out" { "out".into() } else { format!("&mut {}", self.out) };
                self.fallible(&format!("::reze_server::children({out}, cx, page)"));
            }
            Binding::Content(content, frame) => self.content(content, &frame),
            Binding::Null => {}
            Binding::Val(val) => self.output(&val, Sink::Text)?,
            _ => return Err("An object prints as text, which has no Rust lowering".into()),
        }
        Ok(())
    }

    fn attr_hole(&mut self, name: &str, expression: &'a Expression<'a>, frame: &Rc<Frame<'e, 'a>>) {
        let result = match expression.without_parentheses() {
            Expression::ConditionalExpression(conditional) => {
                self.val(&conditional.test, frame).map(|test| {
                    self.open(&format!("if {}", truthy(&test)));
                    self.attr_hole(name, &conditional.consequent, frame);
                    self.reopen("else");
                    self.attr_hole(name, &conditional.alternate, frame);
                    self.close();
                })
            }
            _ => self.val(expression, frame).and_then(|val| self.output(&val, Sink::Attr(name))),
        };
        if let Err(reason) = result {
            self.report(frame.module, expression.span(), reason);
        }
    }

    fn element(&mut self, element: &'e Element<'a>, frame: &Rc<Frame<'e, 'a>>) {
        self.raw(&format!("<{}", element.tag));
        for item in &element.attributes {
            match item {
                AttributeItem::Attribute(attribute) => match &attribute.value {
                    Value::Static(value) => {
                        let mut html = format!(" {}", attribute.name);
                        html::push_attribute_value(&mut html, value);
                        self.raw(&html);
                    }
                    Value::True => self.raw(&format!(" {}", attribute.name)),
                    Value::Expr(expression) => self.attr_hole(attribute.name, expression, frame),
                },
                AttributeItem::Spread(expression) => {
                    if let Err(reason) = self.spread(expression, frame) {
                        self.report(frame.module, expression.span(), reason);
                    }
                }
            }
        }
        if let Some(mark) = self.plan.modules[frame.module].marks.get(&element.span.start) {
            self.raw(" data-rz=");
            self.scope_id(mark.scope, frame);
            self.raw(&format!(".{}", base36(mark.index)));
            if let Some(prevent) = &mark.prevent {
                let mut html = String::from(" data-rz-prevent");
                html::push_attribute_value(&mut html, prevent);
                self.raw(&html);
            }
        }
        self.raw(">");
        if html::is_void(element.tag) {
            return;
        }
        self.nodes(&element.children, element.span.start, frame);
        self.raw(&format!("</{}>", element.tag));
    }

    fn spread(
        &mut self,
        expression: &'a Expression<'a>,
        frame: &Rc<Frame<'e, 'a>>,
    ) -> Result<(), String> {
        let props = match self.lower(expression, frame, false)? {
            Binding::Props(props) => props,
            Binding::Null => return Ok(()),
            _ => return Err("Spreads take objects with known fields".into()),
        };
        for (name, value) in props.iter() {
            let handler = name
                .strip_prefix("on")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()));
            if name == "children" || name == "ref" || handler {
                continue;
            }
            let val = self.as_val(value.clone())?;
            self.output(&val, Sink::Attr(name))?;
        }
        Ok(())
    }

    /// `<Show>` and each `<Match>` of a `<Switch>`: the branch, else the next match or the fallback.
    fn when(
        &mut self,
        when: &'a Expression<'a>,
        children: &'e Branch<'a>,
        rest: &'e [Match<'a>],
        fallback: Option<&'e Branch<'a>>,
        frame: &Rc<Frame<'e, 'a>>,
    ) {
        let otherwise = |this: &mut Self| match rest.split_first() {
            Some((next, rest)) => this.when(next.when, &next.children, rest, fallback, frame),
            None => this.fallback(fallback, frame),
        };
        let binding = match self.lower(when, frame, false) {
            Ok(binding) => binding,
            Err(reason) => return self.report(frame.module, when.span(), reason),
        };
        match binding {
            Binding::Val(val) => match &val.ty {
                Ty::Opt(inner) => {
                    let x = self.fresh("x");
                    let item = Val::new(x.clone(), (**inner).clone());
                    let test = truthy(&item);
                    self.open(&format!("match {}", val.code));
                    self.open(&format!("Some({x}) if {test} =>"));
                    self.branch(children, vec![Binding::Val(item)], frame);
                    self.close();
                    self.open("_ =>");
                    otherwise(self);
                    self.close();
                    self.close();
                }
                _ => {
                    let x = self.fresh("x");
                    let item = Val { code: x.clone(), ..val.clone() };
                    self.stmt(&format!("let {x} = {};", val.code));
                    self.open(&format!("if {}", truthy(&item)));
                    self.branch(children, vec![Binding::Val(item)], frame);
                    self.reopen("else");
                    otherwise(self);
                    self.close();
                }
            },
            Binding::Null => otherwise(self),
            Binding::Children | Binding::Content(..) => {
                self.report(frame.module, when.span(), "Markup is used as a condition");
            }
            other => self.branch(children, vec![other], frame),
        }
    }

    fn branch(
        &mut self,
        branch: &'e Branch<'a>,
        args: Vec<Binding<'e, 'a>>,
        frame: &Rc<Frame<'e, 'a>>,
    ) {
        match branch {
            Branch::Content(content) => self.content(content, frame),
            Branch::Callback(callback) => self.callback(callback, args, frame),
        }
    }

    fn fallback(&mut self, fallback: Option<&'e Branch<'a>>, frame: &Rc<Frame<'e, 'a>>) {
        if let Some(branch) = fallback {
            self.branch(branch, Vec::new(), frame);
        }
    }

    /// The content of a callback called with `args`, in the scope of its row.
    fn callback(
        &mut self,
        callback: &'e Callback<'a>,
        args: Vec<Binding<'e, 'a>>,
        frame: &Rc<Frame<'e, 'a>>,
    ) {
        let mut names = Names::new();
        let mut args = args.into_iter();
        let params = &callback.function.params;
        for param in &params.items {
            let value = args.next().unwrap_or(Binding::Null);
            self.bind(&param.pattern, value, param.initializer.as_deref(), frame, &mut names);
        }
        let mut scopes = HashMap::new();
        let inner = Frame::child(frame, names, HashMap::new());
        let key = callback.content.span.start;
        if let Some(row) = self.plan.modules[frame.module].rows.get(&key) {
            let captures = self.captures(&row.captures, &inner);
            let parent = frame.scope(row.parent).unwrap_or_else(|| "0".into());
            let var = self.fresh("j");
            self.stmt(&format!("let {var} = cx.row({parent}, {}, vec![{captures}]);", row.scope));
            scopes.insert(row.scope, var);
        }
        let inner = Frame::child(&inner, Names::new(), scopes);
        self.content(&callback.content, &inner);
    }

    /// Binds a parameter or declaration pattern to `value`.
    fn bind(
        &mut self,
        pattern: &'a BindingPattern<'a>,
        value: Binding<'e, 'a>,
        default: Option<&'a Expression<'a>>,
        frame: &Rc<Frame<'e, 'a>>,
        names: &mut Names<'e, 'a>,
    ) {
        let value = match (value, default) {
            (Binding::Null, Some(default)) => Binding::Lazy(default, frame.clone()),
            (value, _) => value,
        };
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                names.insert(id.name.to_string(), value);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.bind(&assignment.left, value, Some(&assignment.right), frame, names);
            }
            BindingPattern::ObjectPattern(object) => {
                let mut used = Vec::new();
                for property in &object.properties {
                    let Some(key) = property.key.static_name() else {
                        continue;
                    };
                    used.push(key.to_string());
                    let field =
                        self.member(value.clone(), &key, false).unwrap_or_else(Binding::Invalid);
                    self.bind(&property.value, field, None, frame, names);
                }
                if let Some(rest) = &object.rest {
                    let remaining = match self.force(value.clone()) {
                        Ok(Binding::Props(props)) => Binding::Props(Rc::new(
                            props.iter().filter(|(key, _)| !used.contains(key)).cloned().collect(),
                        )),
                        Ok(_) => Binding::Invalid(
                            "A rest pattern takes an object with known fields".into(),
                        ),
                        Err(reason) => Binding::Invalid(reason),
                    };
                    self.bind(&rest.argument, remaining, None, frame, names);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.bind(
                        element,
                        Binding::Invalid("Array patterns have no Rust lowering".into()),
                        None,
                        frame,
                        names,
                    );
                }
            }
        }
    }

    fn each(&mut self, each: &'e Each<'a>, frame: &Rc<Frame<'e, 'a>>) {
        let source = match self.val(each.source, frame) {
            Ok(source) => source,
            Err(reason) => return self.report(frame.module, each.source.span(), reason),
        };
        let (list, optional) = match &source.ty {
            Ty::List(item) => ((**item).clone(), false),
            Ty::Opt(inner) => match &**inner {
                Ty::List(item) => ((**item).clone(), true),
                _ => {
                    return self.report(
                        frame.module,
                        each.source.span(),
                        "`<For each>` takes a list",
                    );
                }
            },
            Ty::Null => return self.fallback(each.fallback.as_ref(), frame),
            _ => return self.report(frame.module, each.source.span(), "`<For each>` takes a list"),
        };
        let (l, i, x) = (self.fresh("l"), self.fresh("i"), self.fresh("x"));
        if optional {
            self.open(&format!("match {}", source.code));
            self.open(&format!("Some({l}) if !{l}.is_empty() =>"));
        } else {
            self.stmt(&format!("let {l} = {};", source.code));
            self.open(&format!("if !{l}.is_empty()"));
        }
        let item = match view(&format!("(*{x})"), &list) {
            Ok(code) => Binding::Val(Val::new(code, list)),
            Err(reason) => Binding::Invalid(reason),
        };
        self.open(&format!("for ({i}, {x}) in {l}.iter().enumerate()"));
        let index = Binding::Val(Val::new(i, Ty::Num("usize".into())));
        self.callback(&each.children, vec![item, index], frame);
        self.close();
        if optional {
            self.close();
            self.open("_ =>");
            self.fallback(each.fallback.as_ref(), frame);
            self.close();
            self.close();
        } else if each.fallback.is_some() {
            self.reopen("else");
            self.fallback(each.fallback.as_ref(), frame);
            self.close();
        } else {
            self.close();
        }
    }

    fn repeat(&mut self, each: &'e Each<'a>, frame: &Rc<Frame<'e, 'a>>) {
        let count = match self.val(each.source, frame) {
            Ok(Val { code, ty, .. }) if matches!(ty, Ty::Num(_)) && !ty.is_big() => code,
            Ok(_) => {
                return self.report(
                    frame.module,
                    each.source.span(),
                    "`<Repeat count>` takes a number",
                );
            }
            Err(reason) => return self.report(frame.module, each.source.span(), reason),
        };
        let (c, i) = (self.fresh("c"), self.fresh("i"));
        self.stmt(&format!("let {c} = ({count}) as f64;"));
        self.open(&format!("if !({c} > 0.0)"));
        self.fallback(each.fallback.as_ref(), frame);
        self.reopen("else");
        self.stmt(&format!("let mut {i} = 0usize;"));
        self.open(&format!("while ({i} as f64) < {c}"));
        let index = Binding::Val(Val::new(i.clone(), Ty::Num("usize".into())));
        self.callback(&each.children, vec![index], frame);
        self.stmt(&format!("{i} += 1;"));
        self.close();
        self.close();
    }

    fn call(&mut self, call: &'e Call<'a>, frame: &Rc<Frame<'e, 'a>>) {
        let CallTarget::Component(component) = &call.target else {
            return self.report(
                frame.module,
                call.span,
                "Dynamic components have no Rust lowering",
            );
        };
        let mut props: Vec<(String, Binding<'e, 'a>)> = Vec::new();
        let values = call.props.iter().map(|p| (p.name, &p.value));
        for (name, value) in values.chain(call.children.iter().map(|c| ("children", c))) {
            let binding = match value {
                PropValue::Value(Value::Static(text)) => {
                    Binding::Val(Val::new(literal(text), Ty::Str))
                }
                PropValue::Value(Value::True) => Binding::Val(Val::new("true", Ty::Bool)),
                PropValue::Value(Value::Expr(expression)) => match expression {
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                        Binding::Invalid("Functions have no Rust lowering".into())
                    }
                    _ => Binding::Lazy(expression, frame.clone()),
                },
                PropValue::Content(content) => Binding::Content(content, frame.clone()),
                PropValue::Spread(expression) => {
                    match self.lower(expression, frame, false) {
                        Ok(Binding::Props(spread)) => {
                            for (key, value) in spread.iter() {
                                set(&mut props, key.clone(), value.clone());
                            }
                        }
                        Ok(Binding::Null) => {}
                        Ok(_) => self.report(
                            frame.module,
                            expression.span(),
                            "Spreads take objects with known fields",
                        ),
                        Err(reason) => self.report(frame.module, expression.span(), reason),
                    }
                    continue;
                }
            };
            set(&mut props, name.to_string(), binding);
        }
        self.inline(*component, Rc::new(props), frame.clone(), call.span);
    }

    /// Renders `component` with `props` in place.
    pub(super) fn inline(
        &mut self,
        component: ComponentRef,
        props: Rc<Vec<(String, Binding<'e, 'a>)>>,
        caller: Rc<Frame<'e, 'a>>,
        span: Span,
    ) {
        if self.inlining.contains(&component) {
            return self.report(caller.module, span, "Recursive components have no Rust lowering");
        }
        let facts = &self.analysis.facts[component.module];
        let definition = &facts.components[component.index];
        let module_frame = Frame::root(component.module);
        let mut names = Names::new();
        self.params(definition.function.params(), Binding::Props(props), &module_frame, &mut names);
        let mut frame = Frame::child(&module_frame, names, HashMap::new());
        if let Some(body) = definition.function.body() {
            let statements = &body.statements;
            let declarations = statements.len().saturating_sub(1);
            for statement in &statements[..declarations] {
                frame = self.declare(statement, &frame);
            }
        }
        let Some(template) = &definition.template else { return };
        let mut scopes = HashMap::new();
        if let Some(scope) =
            self.plan.modules[component.module].components.get(&template.span.start)
        {
            let captures = self.captures(&scope.captures, &frame);
            let var = self.fresh("s");
            self.stmt(&format!(
                "let {var} = cx.scope({}, {}, {}, vec![{captures}]);",
                scope.id,
                literal(&scope.events),
                scope.eager
            ));
            scopes.insert(0, var);
        }
        let frame = Frame::child(&frame, Names::new(), scopes);
        self.inlining.push(component);
        self.content(template, &frame);
        self.inlining.pop();
    }

    fn params(
        &mut self,
        params: &'a FormalParameters<'a>,
        props: Binding<'e, 'a>,
        frame: &Rc<Frame<'e, 'a>>,
        names: &mut Names<'e, 'a>,
    ) {
        if let Some(param) = params.items.first() {
            self.bind(&param.pattern, props, param.initializer.as_deref(), frame, names);
        }
    }

    /// The frame after one statement of a component body.
    fn declare(
        &mut self,
        statement: &'a Statement<'a>,
        frame: &Rc<Frame<'e, 'a>>,
    ) -> Rc<Frame<'e, 'a>> {
        let mut names = Names::new();
        match statement {
            Statement::VariableDeclaration(declaration) => {
                let mut frame = frame.clone();
                for declarator in &declaration.declarations {
                    let mut names = Names::new();
                    let value = match &declarator.init {
                        Some(init) => self.initial(init, &frame),
                        None => Binding::Null,
                    };
                    self.bind(&declarator.id, value, None, &frame, &mut names);
                    frame = Frame::child(&frame, names, HashMap::new());
                }
                return frame;
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    names.insert(
                        id.name.to_string(),
                        Binding::Invalid("Functions have no Rust lowering".into()),
                    );
                }
            }
            _ => return frame.clone(),
        }
        Frame::child(frame, names, HashMap::new())
    }

    /// The server value of a local's initializer: a signal's initial value, a computed's body.
    fn initial(&mut self, init: &'a Expression<'a>, frame: &Rc<Frame<'e, 'a>>) -> Binding<'e, 'a> {
        let Expression::CallExpression(call) = init.without_parentheses() else {
            return Binding::Lazy(init, frame.clone());
        };
        let argument = call.arguments.first().and_then(|a| a.as_expression());
        let Some(api) = self.api_of(frame.module, &call.callee) else {
            return Binding::Lazy(init, frame.clone());
        };
        match api {
            Api::Signal | Api::Store => match argument {
                Some(argument) => Binding::Lazy(argument, frame.clone()),
                None => Binding::Null,
            },
            Api::Computed => match argument {
                Some(Expression::ArrowFunctionExpression(arrow)) => match arrow.get_expression() {
                    Some(body) => Binding::Lazy(body, frame.clone()),
                    None => {
                        Binding::Invalid("A computed with a block body has no Rust lowering".into())
                    }
                },
                _ => Binding::Invalid("A computed takes an arrow function".into()),
            },
            Api::CreateUniqueId => {
                let var = self.fresh("u");
                self.stmt(&format!("let {var} = cx.id();"));
                Binding::Val(Val::new(format!("{var}.as_str()"), Ty::Str))
            }
            Api::UseParams => Binding::Params(usize::MAX),
            _ => Binding::Invalid(format!("`{api:?}` has no Rust lowering")),
        }
    }

    /// The values of a client scope's captures, as JSON strings.
    fn captures(&mut self, captures: &[String], frame: &Rc<Frame<'e, 'a>>) -> String {
        let mut out = Vec::new();
        for capture in captures {
            let allocator = Allocator::default();
            let parsed =
                oxc_parser::Parser::new(&allocator, capture, SourceType::tsx()).parse_expression();
            let result = match &parsed {
                Ok(expression) => self.lower(expression, frame, false).and_then(|b| self.as_val(b)),
                Err(_) => Err(format!("`{capture}` does not parse")),
            };
            match result {
                Ok(Val { ty: Ty::Null, .. }) => out.push("\"null\".to_string()".to_string()),
                Ok(val) => out.push(format!("::reze_server::reze::to_json(&{})", val.code)),
                Err(reason) => {
                    self.report(
                        frame.module,
                        Span::empty(0),
                        format!("Client code captures `{capture}`: {reason}"),
                    );
                    out.push("\"null\".to_string()".to_string());
                }
            }
        }
        out.join(", ")
    }
}
