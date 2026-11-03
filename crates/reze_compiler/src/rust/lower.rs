//! Expressions: values of a known Rust type, their truthiness, and how they print.

use std::rc::Rc;

use oxc_ast::ast::{
    BinaryOperator, ChainElement, Expression, LogicalOperator, ObjectPropertyKind, UnaryOperator,
};

use super::{
    Binding, Emitter, Frame, ISSUE, Lowered, Ty, TypeDef, TypeRef, Val, VariantDef, float, literal,
};
use crate::analyze::{Api, Endpoint, EndpointKind, ImportSource};

const JS: &str = "::reze_server::reze::js";
const DATE: &str = "::reze_server::reze::date";
const VARIANT: &str = "::reze_server::reze::Variant::name";

/// Where a value prints: as text, or as the value of an attribute.
#[derive(Clone, Copy)]
pub(super) enum Sink<'n> {
    Text,
    Attr(&'n str),
}

/// The view of `place`, an expression of the owned Rust type of `ty`.
pub(super) fn view(place: &str, ty: &Ty) -> Result<String, String> {
    Ok(match ty {
        Ty::Str => format!("{place}.as_str()"),
        Ty::Num(_) | Ty::Bool | Ty::Null => place.to_string(),
        Ty::Struct(_)
        | Ty::Enum(_)
        | Ty::List(_)
        | Ty::Tuple(_)
        | Ty::Date
        | Ty::Set(_)
        | Ty::Map(_) => format!("&{place}"),
        Ty::Opt(inner) => match **inner {
            Ty::Str => format!("{place}.as_deref()"),
            Ty::Num(_) | Ty::Bool => place.to_string(),
            Ty::Struct(_)
            | Ty::Enum(_)
            | Ty::List(_)
            | Ty::Tuple(_)
            | Ty::Date
            | Ty::Set(_)
            | Ty::Map(_) => format!("{place}.as_ref()"),
            _ => return Err("Nested optional values have no Rust lowering".into()),
        },
    })
}

/// The condition that `val` is truthy, as JS decides it.
pub(super) fn truthy(val: &Val) -> String {
    let code = &val.code;
    match &val.ty {
        Ty::Str => format!("!({code}).is_empty()"),
        Ty::Num(_) if val.ty.is_big() => "true".into(),
        Ty::Num(_) if val.ty.is_float() => format!("{JS}::truthy({code} as f64)"),
        Ty::Num(_) => format!("({code}) != 0"),
        Ty::Bool => format!("({code})"),
        Ty::Null => "false".into(),
        Ty::Struct(_)
        | Ty::Enum(_)
        | Ty::List(_)
        | Ty::Tuple(_)
        | Ty::Date
        | Ty::Set(_)
        | Ty::Map(_) => "true".into(),
        Ty::Opt(inner) => {
            let test = truthy(&Val::new("x", (**inner).clone()));
            if test == "true" {
                format!("({code}).is_some()")
            } else {
                format!("({code}).is_some_and(|x| {test})")
            }
        }
    }
}

fn number(val: &Val) -> Result<String, String> {
    match &val.ty {
        Ty::Num(_) if !val.ty.is_big() => Ok(format!("(({}) as f64)", val.code)),
        _ => Err("Arithmetic takes numbers".into()),
    }
}

/// Statements that append `val` to the `String` in `var`, as a template literal prints it.
fn push(var: &str, val: &Val) -> Result<String, String> {
    let code = &val.code;
    Ok(match &val.ty {
        Ty::Str => format!("{var}.push_str({});", val.str()),
        Ty::Num(_) if val.ty.is_float() => format!("{JS}::push_number(&mut {var}, {code} as f64);"),
        Ty::Num(_) => format!("{var}.push_str(&({code}).to_string());"),
        Ty::Bool => format!("{var}.push_str(if {code} {{ \"true\" }} else {{ \"false\" }});"),
        Ty::Enum(_) => format!("{var}.push_str({VARIANT}({code}));"),
        Ty::Null | Ty::Opt(_) => {
            return Err(
                "A template literal of a value that may be absent prints `undefined`".into()
            );
        }
        Ty::Struct(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Date | Ty::Set(_) | Ty::Map(_) => {
            return Err("A template literal of an object or a list has no Rust lowering".into());
        }
    })
}

/// The codes of `a` and `b` as one Rust type, for the two branches of a value.
fn unify(a: &Val, b: &Val) -> Result<(String, String, Val), String> {
    let owned =
        |val: &Val| if val.owned { val.code.clone() } else { format!("{}.to_string()", val.code) };
    match (&a.ty, &b.ty) {
        (Ty::Str, Ty::Str) if a.owned || b.owned => Ok((owned(a), owned(b), Val::owned(""))),
        (x, y) if x == y => Ok((a.code.clone(), b.code.clone(), Val::new("", x.clone()))),
        (Ty::Num(_), Ty::Num(_)) => {
            Ok((number(a)?, number(b)?, Val::new("", Ty::Num("f64".into()))))
        }
        (Ty::Null, Ty::Opt(_)) => Ok(("None".into(), b.code.clone(), Val::new("", b.ty.clone()))),
        (Ty::Opt(_), Ty::Null) => Ok((a.code.clone(), "None".into(), Val::new("", a.ty.clone()))),
        (Ty::Opt(inner), other) if **inner == *other && !b.owned => {
            Ok((a.code.clone(), format!("Some({})", b.code), Val::new("", a.ty.clone())))
        }
        (other, Ty::Opt(inner)) if **inner == *other && !a.owned => {
            Ok((format!("Some({})", a.code), b.code.clone(), Val::new("", b.ty.clone())))
        }
        (Ty::Null, other) if !b.owned => Ok((
            "None".into(),
            format!("Some({})", b.code),
            Val::new("", Ty::Opt(Box::new(other.clone()))),
        )),
        (other, Ty::Null) if !a.owned => Ok((
            format!("Some({})", a.code),
            "None".into(),
            Val::new("", Ty::Opt(Box::new(other.clone()))),
        )),
        _ => Err("The two branches have different types".into()),
    }
}

fn with_code(template: Val, code: String) -> Val {
    Val { code, ..template }
}

impl<'e, 'a> Emitter<'e, 'a> {
    pub(super) fn force(&mut self, binding: Binding<'e, 'a>) -> Lowered<'e, 'a> {
        match binding {
            Binding::Lazy(expression, frame) => {
                if self.lowering > 64 {
                    return Err("The value refers to itself".into());
                }
                self.lowering += 1;
                let lowered = self.lower(expression, &frame, false);
                self.lowering -= 1;
                lowered
            }
            Binding::Invalid(reason) => Err(reason),
            other => Ok(other),
        }
    }

    pub(super) fn val<'x>(
        &mut self,
        expression: &'x Expression<'x>,
        frame: &Rc<Frame<'e, 'a>>,
    ) -> Result<Val, String> {
        let binding = self.lower(expression, frame, false)?;
        self.as_val(binding)
    }

    pub(super) fn as_val(&mut self, binding: Binding<'e, 'a>) -> Result<Val, String> {
        match self.force(binding)? {
            Binding::Val(val) => Ok(val),
            Binding::Null => Ok(Val::new("()", Ty::Null)),
            Binding::Props(_) | Binding::Params(_) | Binding::Search | Binding::Error(_) => {
                Err("An object is used as a value".into())
            }
            _ => Err("Markup is used as a value".into()),
        }
    }

    fn name(&mut self, name: &str, frame: &Rc<Frame<'e, 'a>>) -> Lowered<'e, 'a> {
        if name == "undefined" {
            return Ok(Binding::Null);
        }
        if let Some(binding) = frame.lookup(name) {
            return self.force(binding);
        }
        if let Some(init) = self.constant(frame.module, name) {
            return self.force(Binding::Lazy(init, Frame::root(frame.module)));
        }
        Err(format!("`{name}` is not a projection of params, data, or loop variables"))
    }

    pub(super) fn api_of(&self, module: usize, expression: &Expression<'_>) -> Option<Api> {
        let Expression::Identifier(reference) = expression.without_parentheses() else {
            return None;
        };
        let symbol = self.modules[module]
            .semantic
            .scoping()
            .get_reference(reference.reference_id.get()?)
            .symbol_id()?;
        self.analysis.facts[module].apis.get(&symbol).copied()
    }

    /// The mutation an expression names, following imports between project modules.
    pub(super) fn mutation_of(
        &self,
        module: usize,
        expression: &Expression<'_>,
    ) -> Option<&'e Endpoint> {
        let Expression::Identifier(reference) = expression.without_parentheses() else {
            return None;
        };
        let symbol = self.modules[module]
            .semantic
            .scoping()
            .get_reference(reference.reference_id.get()?)
            .symbol_id()?;
        let facts = &self.analysis.facts;
        let mutation = |endpoint: &&Endpoint| endpoint.kind == EndpointKind::Mutation;
        match facts[module].imports.get(&symbol) {
            Some(import) => match import.source {
                ImportSource::Module(target) => facts[target]
                    .endpoints
                    .iter()
                    .filter(mutation)
                    .find(|endpoint| endpoint.export == import.name),
                ImportSource::Package(_) => None,
            },
            None => facts[module]
                .endpoints
                .iter()
                .filter(mutation)
                .find(|endpoint| endpoint.symbol == Some(symbol)),
        }
    }

    pub(super) fn lower<'x>(
        &mut self,
        expression: &'x Expression<'x>,
        frame: &Rc<Frame<'e, 'a>>,
        chain: bool,
    ) -> Lowered<'e, 'a> {
        let binding = self.binding(expression, frame, chain)?;
        self.force(binding)
    }

    fn binding<'x>(
        &mut self,
        expression: &'x Expression<'x>,
        frame: &Rc<Frame<'e, 'a>>,
        chain: bool,
    ) -> Lowered<'e, 'a> {
        let val = |val: Val| Ok(Binding::Val(val));
        match expression {
            Expression::Identifier(id) => self.name(&id.name, frame),
            Expression::StringLiteral(s) => val(Val::new(literal(&s.value), Ty::Str)),
            Expression::NumericLiteral(n) => val(Val::new(float(n.value), Ty::Num("f64".into()))),
            Expression::BooleanLiteral(b) => val(Val::new(b.value.to_string(), Ty::Bool)),
            Expression::NullLiteral(_) => Ok(Binding::Null),
            Expression::TemplateLiteral(template) => {
                let var = self.fresh("t");
                let mut code = format!("{{ let mut {var} = String::new(); ");
                for (index, quasi) in template.quasis.iter().enumerate() {
                    let Some(cooked) = &quasi.value.cooked else {
                        return Err("The template literal has an invalid escape".into());
                    };
                    if !cooked.is_empty() {
                        code.push_str(&format!("{var}.push_str({}); ", literal(cooked)));
                    }
                    if let Some(expression) = template.expressions.get(index) {
                        let part = self.val(expression, frame)?;
                        code.push_str(&push(&var, &part)?);
                        code.push(' ');
                    }
                }
                code.push_str(&format!("{var} }}"));
                val(Val::owned(code))
            }
            Expression::StaticMemberExpression(member) => {
                let object = self.lower(&member.object, frame, chain)?;
                self.member(object, &member.property.name, chain)
            }
            Expression::ComputedMemberExpression(member) => {
                let key = literal_key(&member.expression)?;
                let object = self.lower(&member.object, frame, chain)?;
                self.member(object, &key, chain)
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => {
                    let object = self.lower(&member.object, frame, true)?;
                    self.member(object, &member.property.name, true)
                }
                ChainElement::ComputedMemberExpression(member) => {
                    let key = literal_key(&member.expression)?;
                    let object = self.lower(&member.object, frame, true)?;
                    self.member(object, &key, true)
                }
                _ => Err("Optional calls have no Rust lowering".into()),
            },
            Expression::ParenthesizedExpression(inner) => {
                self.lower(&inner.expression, frame, chain)
            }
            Expression::TSAsExpression(inner) => self.lower(&inner.expression, frame, chain),
            Expression::TSSatisfiesExpression(inner) => self.lower(&inner.expression, frame, chain),
            Expression::TSNonNullExpression(inner) => {
                match self.lower(&inner.expression, frame, chain)? {
                    Binding::Val(Val { code, ty: Ty::Opt(ty), .. }) => {
                        val(Val::new(format!("({code}).unwrap()"), *ty))
                    }
                    other => Ok(other),
                }
            }
            Expression::UnaryExpression(unary) => {
                let argument = self.val(&unary.argument, frame)?;
                match unary.operator {
                    UnaryOperator::LogicalNot => {
                        val(Val::new(format!("!{}", truthy(&argument)), Ty::Bool))
                    }
                    UnaryOperator::UnaryNegation => {
                        val(Val::new(format!("-{}", number(&argument)?), Ty::Num("f64".into())))
                    }
                    UnaryOperator::UnaryPlus => {
                        val(Val::new(number(&argument)?, Ty::Num("f64".into())))
                    }
                    _ => Err(format!("`{}` has no Rust lowering", unary.operator.as_str())),
                }
            }
            Expression::BinaryExpression(binary) => {
                let left = self.val(&binary.left, frame)?;
                let right = self.val(&binary.right, frame)?;
                self.binary(&left, binary.operator, &right).map(Binding::Val)
            }
            Expression::LogicalExpression(logical) => {
                let left = self.val(&logical.left, frame)?;
                let right = self.val(&logical.right, frame)?;
                self.logical(&left, logical.operator, &right).map(Binding::Val)
            }
            Expression::ConditionalExpression(conditional) => {
                let test = self.val(&conditional.test, frame)?;
                let consequent = self.val(&conditional.consequent, frame)?;
                let alternate = self.val(&conditional.alternate, frame)?;
                let (a, b, template) = unify(&consequent, &alternate)?;
                val(with_code(template, format!("if {} {{ {a} }} else {{ {b} }}", truthy(&test))))
            }
            Expression::ObjectExpression(object) => {
                let mut fields: Vec<(String, Binding<'e, 'a>)> = Vec::new();
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            let Some(key) = property.key.static_name() else {
                                return Err("Computed keys have no Rust lowering".into());
                            };
                            let value = match &property.value {
                                Expression::ArrowFunctionExpression(_)
                                | Expression::FunctionExpression(_) => {
                                    Binding::Invalid("Functions have no Rust lowering".into())
                                }
                                value => {
                                    self.lower(value, frame, false).unwrap_or_else(Binding::Invalid)
                                }
                            };
                            set(&mut fields, key.to_string(), value);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            match self.lower(&spread.argument, frame, false)? {
                                Binding::Props(props) => {
                                    for (key, value) in props.iter() {
                                        set(&mut fields, key.clone(), value.clone());
                                    }
                                }
                                Binding::Null => {}
                                _ => return Err("Spreads take objects with known fields".into()),
                            }
                        }
                    }
                }
                Ok(Binding::Props(Rc::new(fields)))
            }
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                Ok(Binding::Invalid("Functions have no Rust lowering".into()))
            }
            Expression::CallExpression(call) => {
                if let Expression::StaticMemberExpression(member) =
                    call.callee.without_parentheses()
                    && let method @ ("get" | "has") = member.property.name.as_str()
                    && let Ok(Binding::Search) = self.lower(&member.object, frame, false)
                {
                    let Some(name) = call.arguments.first().and_then(|a| a.as_expression()) else {
                        return Err(format!("`{method}` takes the name of a search param"));
                    };
                    let name = self.val(name, frame)?;
                    if name.ty != Ty::Str {
                        return Err(format!("`{method}` takes a string"));
                    }
                    let query = format!("page.query({})", name.str());
                    return val(if method == "get" {
                        Val::new(query, Ty::Opt(Box::new(Ty::Str)))
                    } else {
                        Val::new(format!("{query}.is_some()"), Ty::Bool)
                    });
                }
                if self.api_of(frame.module, &call.callee) == Some(Api::BuildPaths) {
                    return self.build_path(call, frame).map(Binding::Val);
                }
                if self.api_of(frame.module, &call.callee) == Some(Api::Untrack)
                    && let Some(Expression::ArrowFunctionExpression(arrow)) =
                        call.arguments.first().and_then(|a| a.as_expression())
                    && let Some(body) = arrow.get_expression()
                {
                    return self.lower(body, frame, chain);
                }
                if let Expression::StaticMemberExpression(member) =
                    call.callee.without_parentheses()
                    && let Ok(object) = self.val(&member.object, frame)
                    && matches!(object.ty, Ty::Date | Ty::Set(_) | Ty::Map(_))
                {
                    let mut arguments = Vec::new();
                    for argument in &call.arguments {
                        let Some(argument) = argument.as_expression() else {
                            return Err("Spread arguments have no Rust lowering".into());
                        };
                        arguments.push(self.val(argument, frame)?);
                    }
                    return self
                        .method(&object, &member.property.name, &arguments)
                        .map(Binding::Val);
                }
                Err("Calls to unanalyzed functions are not lowered".into())
            }
            _ => Err("This expression has no Rust lowering".into()),
        }
    }

    /// `buildPaths("/posts/:id", { id })`: the pattern is a literal, so its segments are known.
    fn build_path(
        &mut self,
        call: &oxc_ast::ast::CallExpression<'_>,
        frame: &Rc<Frame<'e, 'a>>,
    ) -> Result<Val, String> {
        let mut arguments = call.arguments.iter().map(|a| a.as_expression());
        let Some(Some(Expression::StringLiteral(pattern))) = arguments.next() else {
            return Err("`buildPaths` takes a pattern literal".into());
        };
        let params = match arguments.next().flatten() {
            Some(params) => self.lower(params, frame, false)?,
            None => Binding::Null,
        };
        let var = self.fresh("t");
        let mut code = format!("{{ let mut {var} = String::new(); ");
        for part in pattern.value.split('/').filter(|part| !part.is_empty()) {
            let (splat, name) = match part.as_bytes()[0] {
                b':' => (false, &part[1..]),
                b'*' => (true, &part[1..]),
                _ => {
                    code.push_str(&format!("{var}.push_str({}); ", literal(&format!("/{part}"))));
                    continue;
                }
            };
            let (optional, name) =
                name.strip_suffix('?').map_or((splat, name), |name| (true, name));
            let value = match &params {
                Binding::Props(_) => self.member(params.clone(), name, false)?,
                _ => Binding::Null,
            };
            let value = self.as_val(value)?;
            let segment = |this: &mut Self, val: &Val| -> Result<String, String> {
                let p = this.fresh("p");
                Ok(format!(
                    "{{ let mut {p} = String::new(); {} {var}.push('/'); ::reze_server::push_segment(&mut {var}, &{p}, {splat}); }} ",
                    push(&p, val)?
                ))
            };
            match &value.ty {
                Ty::Null if optional => {}
                Ty::Null => return Err(format!("`buildPaths` needs the param `{name}`")),
                Ty::Opt(inner) if optional => {
                    let x = self.fresh("x");
                    let inner = segment(self, &Val::new(x.clone(), (**inner).clone()))?;
                    code.push_str(&format!("if let Some({x}) = {} {{ {inner}}} ", value.code));
                }
                Ty::Str if optional => {
                    let inner = segment(self, &value)?;
                    code.push_str(&format!("if !({}).is_empty() {{ {inner}}} ", value.str()));
                }
                _ => code.push_str(&segment(self, &value)?),
            }
        }
        code.push_str(&format!("if {var}.is_empty() {{ {var}.push('/'); }} {var} }}"));
        Ok(Val::owned(code))
    }

    pub(super) fn member(
        &mut self,
        object: Binding<'e, 'a>,
        name: &str,
        chain: bool,
    ) -> Lowered<'e, 'a> {
        Ok(match self.force(object)? {
            Binding::Props(props) => props
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map_or(Binding::Null, |(_, b)| b.clone()),
            Binding::Params(node) => {
                let required = self.routes.nodes.get(node).is_some_and(|node| {
                    node.segments.iter().any(|segment| {
                        matches!(segment, crate::routes::Segment::Param(n) | crate::routes::Segment::Splat(n) if n == name)
                    })
                });
                if required {
                    Binding::Val(Val::new(format!("page.param({})", literal(name)), Ty::Str))
                } else {
                    Binding::Val(Val::new(
                        format!("page.param_opt({})", literal(name)),
                        Ty::Opt(Box::new(Ty::Str)),
                    ))
                }
            }
            Binding::Search => {
                return Err(format!("`{name}` of search params has no Rust lowering"));
            }
            Binding::Error(var) => match name {
                "status" => Binding::Val(Val::new(format!("{var}.status"), Ty::Num("u16".into()))),
                "message" => Binding::Val(Val::new(format!("{var}.message.as_str()"), Ty::Str)),
                _ => Binding::Null,
            },
            Binding::Val(val) if name == "length" => Binding::Val(length(&val, chain)?),
            Binding::Val(val) => match &val.ty {
                Ty::Opt(inner) if chain => {
                    let x = self.fresh("x");
                    let field = self.field(&Val::new(x.clone(), (**inner).clone()), name)?;
                    let (code, ty) = match field.ty {
                        Ty::Opt(_) => {
                            (format!("({}).and_then(|{x}| {})", val.code, field.code), field.ty)
                        }
                        ty => (
                            format!("({}).map(|{x}| {})", val.code, field.code),
                            Ty::Opt(Box::new(ty)),
                        ),
                    };
                    Binding::Val(Val::new(code, ty))
                }
                Ty::Opt(_) => {
                    return Err(format!("`{name}` is read from a value that may be absent"));
                }
                _ => Binding::Val(self.field(&val, name)?),
            },
            Binding::Null => return Err(format!("`{name}` is read from `undefined`")),
            Binding::Children | Binding::Content(..) => {
                return Err(format!("`{name}` is read from markup"));
            }
            Binding::Lazy(..) | Binding::Invalid(_) => unreachable!("forced"),
        })
    }

    fn field(&self, object: &Val, name: &str) -> Result<Val, String> {
        if let Ty::Set(_) | Ty::Map(_) = object.ty {
            return match name {
                "size" => Ok(Val::new(format!("({}).len()", object.code), Ty::Num("usize".into()))),
                _ => Err(format!("`{name}` of a set or a map has no Rust lowering")),
            };
        }
        if let Ty::Tuple(items) = &object.ty {
            let Some(ty) = name.parse::<usize>().ok().and_then(|index| items.get(index)) else {
                return Err(format!("`{name}` is read from a tuple without it"));
            };
            let code = view(&format!("({}).{name}", object.code), ty)?;
            return Ok(Val::new(code, ty.clone()));
        }
        let Ty::Struct(path) = &object.ty else {
            return Err(format!("`{name}` is read from a value that is not a struct"));
        };
        if path == ISSUE {
            return match name {
                "message" => Ok(Val::new(format!("({}).message.as_str()", object.code), Ty::Str)),
                _ => Err(format!("`{name}` of an issue has no Rust lowering")),
            };
        }
        let fields = match self.types.get(path).copied() {
            Some(TypeDef::Struct { fields, .. }) => fields,
            Some(TypeDef::Union { .. }) if name == "kind" => {
                return Ok(Val::new(format!("{VARIANT}({})", object.code), Ty::Str));
            }
            Some(TypeDef::Union { variants, .. }) => {
                return self.variant_field(object, path, variants, name);
            }
            _ => return Err(format!("`{path}` is not a struct of the Rust type IR")),
        };
        let Some(field) = fields.iter().find(|f| f.name == name) else {
            return Err(format!("`{path}` has no field `{name}`"));
        };
        let krate = path.split("::").next().unwrap_or_default();
        let ty = self.ty_of(&field.ty, krate);
        let mut place = format!("({}).{}", object.code, super::rust_ident(name));
        if matches!(field.ty, TypeRef::Defer { .. }) {
            let pending = "Err(::reze_server::Render::pending())";
            let stop = match self.labels.last() {
                Some(label) => format!("break '{label} {pending}"),
                None => format!("return {pending}"),
            };
            place = format!(
                "(*match {place}.get() {{ Some(x) => x, None => {{ {place}.wait(); {stop} }} }})"
            );
        }
        let code = view(&place, &ty)?;
        Ok(Val::new(code, ty))
    }

    /// A method call on a `Date`, a `Set`, or a `Map`.
    fn method(&mut self, object: &Val, name: &str, arguments: &[Val]) -> Result<Val, String> {
        let code = &object.code;
        match (&object.ty, name, arguments) {
            (Ty::Date, "getTime" | "valueOf", []) => {
                Ok(Val::new(format!("({DATE}::millis(*({code})) as f64)"), Ty::Num("f64".into())))
            }
            (Ty::Date, "toISOString" | "toJSON", []) => {
                Ok(Val::owned(format!("{DATE}::iso(*({code}))")))
            }
            (Ty::Set(of), "has", [item]) => {
                let test = match (&**of, &item.ty) {
                    (Ty::Str, Ty::Str) => format!("({code}).contains({})", item.str()),
                    (Ty::Bool, Ty::Bool) => format!("({code}).contains(&({}))", item.code),
                    (Ty::Enum(x), Ty::Enum(y)) if x == y => {
                        format!("({code}).contains({})", item.code)
                    }
                    (Ty::Num(rust), Ty::Num(_)) if !of.is_float() && !of.is_big() => {
                        let v = self.fresh("v");
                        format!(
                            "{{ let {v} = {}; ({v} as {rust}) as f64 == {v} && ({code}).contains(&({v} as {rust})) }}",
                            number(item)?
                        )
                    }
                    _ => "false".into(),
                };
                Ok(Val::new(test, Ty::Bool))
            }
            (Ty::Map(_), "has", [key]) if key.ty == Ty::Str => {
                Ok(Val::new(format!("({code}).contains_key({})", key.str()), Ty::Bool))
            }
            (Ty::Map(value), "get", [key]) if key.ty == Ty::Str => {
                if matches!(**value, Ty::Opt(_)) {
                    return Err("Nested optional values have no Rust lowering".into());
                }
                let x = self.fresh("x");
                let read = view(&format!("(*{x})"), value)?;
                Ok(Val::new(
                    format!("({code}).get({}).map(|{x}| {read})", key.str()),
                    Ty::Opt(value.clone()),
                ))
            }
            _ => Err(format!("`{name}` with these arguments has no Rust lowering")),
        }
    }

    /// A field of an enum with fields, `undefined` in the variants without it, as in JS.
    fn variant_field(
        &self,
        object: &Val,
        path: &str,
        variants: &[VariantDef],
        name: &str,
    ) -> Result<Val, String> {
        let krate = path.split("::").next().unwrap_or_default();
        let mut ty = None;
        let mut arms = String::new();
        for variant in variants {
            let Some(field) = variant.fields.iter().find(|f| f.name == name) else { continue };
            if matches!(field.ty, TypeRef::Defer { .. }) {
                return Err(format!("`{name}` of an enum variant is deferred"));
            }
            let field_ty = self.ty_of(&field.ty, krate);
            if ty.as_ref().is_some_and(|ty| *ty != field_ty) {
                return Err(format!("`{name}` has another type in each variant of `{path}`"));
            }
            let field = super::rust_ident(name);
            let value = view("(*x)", &field_ty)?;
            arms.push_str(&format!(
                "::{path}::{} {{ {field}: x, .. }} => Some({value}), ",
                variant.name
            ));
            ty = Some(field_ty);
        }
        let Some(ty) = ty else { return Err(format!("No variant of `{path}` has `{name}`")) };
        if matches!(ty, Ty::Opt(_)) {
            return Err("Nested optional values have no Rust lowering".into());
        }
        let code = format!("(match {} {{ {arms}_ => None }})", object.code);
        Ok(Val::new(code, Ty::Opt(Box::new(ty))))
    }

    fn binary(&mut self, left: &Val, operator: BinaryOperator, right: &Val) -> Result<Val, String> {
        use BinaryOperator::*;
        let boolean = |code: String| Ok(Val::new(code, Ty::Bool));
        match operator {
            StrictEquality | Equality => boolean(self.equal(left, right)?),
            StrictInequality | Inequality => boolean(format!("!({})", self.equal(left, right)?)),
            LessThan | LessEqualThan | GreaterThan | GreaterEqualThan => {
                let (op, method) = match operator {
                    LessThan => ("<", "is_lt"),
                    LessEqualThan => ("<=", "is_le"),
                    GreaterThan => (">", "is_gt"),
                    _ => (">=", "is_ge"),
                };
                match (&left.ty, &right.ty) {
                    (Ty::Str, Ty::Str) => boolean(format!(
                        "{JS}::compare({}, {}).{method}()",
                        left.str(),
                        right.str()
                    )),
                    _ => boolean(format!("({} {op} {})", number(left)?, number(right)?)),
                }
            }
            Addition if matches!(left.ty, Ty::Str) || matches!(right.ty, Ty::Str) => {
                let var = self.fresh("t");
                Ok(Val::owned(format!(
                    "{{ let mut {var} = String::new(); {} {} {var} }}",
                    push(&var, left)?,
                    push(&var, right)?
                )))
            }
            Addition | Subtraction | Multiplication | Division | Remainder => {
                let op = match operator {
                    Addition => "+",
                    Subtraction => "-",
                    Multiplication => "*",
                    Division => "/",
                    _ => "%",
                };
                Ok(Val::new(
                    format!("({} {op} {})", number(left)?, number(right)?),
                    Ty::Num("f64".into()),
                ))
            }
            Exponential => Ok(Val::new(
                format!("{}.powf({})", number(left)?, number(right)?),
                Ty::Num("f64".into()),
            )),
            _ => Err(format!("`{}` has no Rust lowering", operator.as_str())),
        }
    }

    /// The condition that `a` equals `b`: `==` and `===` agree on these types.
    fn equal(&mut self, a: &Val, b: &Val) -> Result<String, String> {
        Ok(match (&a.ty, &b.ty) {
            (Ty::Null, Ty::Null) => "true".into(),
            (Ty::Opt(_), Ty::Null) => format!("({}).is_none()", a.code),
            (Ty::Null, Ty::Opt(_)) => format!("({}).is_none()", b.code),
            (Ty::Null, _) | (_, Ty::Null) => "false".into(),
            (Ty::Opt(inner), Ty::Opt(other)) => {
                let (x, y) = (self.fresh("x"), self.fresh("y"));
                let test = self.equal(
                    &Val::new(x.clone(), (**inner).clone()),
                    &Val::new(y.clone(), (**other).clone()),
                )?;
                format!(
                    "match ({}, {}) {{ (Some({x}), Some({y})) => {test}, (None, None) => true, _ => false }}",
                    a.code, b.code
                )
            }
            (Ty::Opt(inner), _) => {
                let x = self.fresh("x");
                let test = self.equal(&Val::new(x.clone(), (**inner).clone()), b)?;
                format!("({}).is_some_and(|{x}| {test})", a.code)
            }
            (_, Ty::Opt(_)) => return self.equal(b, a),
            (Ty::Str, Ty::Str) => format!("({} == {})", a.str(), b.str()),
            (Ty::Enum(_), Ty::Str) => format!("({VARIANT}({}) == {})", a.code, b.str()),
            (Ty::Str, Ty::Enum(_)) => format!("({} == {VARIANT}({}))", a.str(), b.code),
            (Ty::Enum(x), Ty::Enum(y)) if x == y => {
                format!("({VARIANT}({}) == {VARIANT}({}))", a.code, b.code)
            }
            (Ty::Num(_), Ty::Num(_)) if !a.ty.is_big() && !b.ty.is_big() => {
                format!("({} == {})", number(a)?, number(b)?)
            }
            (Ty::Bool, Ty::Bool) => format!("({} == {})", a.code, b.code),
            (
                Ty::Struct(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Date | Ty::Set(_) | Ty::Map(_),
                _,
            )
            | (
                _,
                Ty::Struct(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Date | Ty::Set(_) | Ty::Map(_),
            ) => {
                return Err("Objects compare by identity, which has no Rust lowering".into());
            }
            _ => "false".into(),
        })
    }

    fn logical(
        &mut self,
        left: &Val,
        operator: LogicalOperator,
        right: &Val,
    ) -> Result<Val, String> {
        let x = self.fresh("x");
        match (operator, &left.ty) {
            (LogicalOperator::Coalesce, Ty::Null) => Ok(right.clone()),
            (LogicalOperator::Coalesce, Ty::Opt(inner)) => {
                let (a, b, template) = unify(&Val::new(x.clone(), (**inner).clone()), right)?;
                Ok(with_code(
                    template,
                    format!("match {} {{ Some({x}) => {a}, None => {b} }}", left.code),
                ))
            }
            (LogicalOperator::Coalesce, _) => Ok(left.clone()),
            (LogicalOperator::Or, Ty::Opt(inner)) => {
                let item = Val::new(x.clone(), (**inner).clone());
                let (a, b, template) = unify(&item, right)?;
                Ok(with_code(
                    template,
                    format!(
                        "match {} {{ Some({x}) if {} => {a}, _ => {b} }}",
                        left.code,
                        truthy(&item)
                    ),
                ))
            }
            (LogicalOperator::Or, _) => {
                let item = Val { code: x.clone(), ..left.clone() };
                let (a, b, template) = unify(&item, right)?;
                Ok(with_code(
                    template,
                    format!(
                        "{{ let {x} = {}; if {} {{ {a} }} else {{ {b} }} }}",
                        left.code,
                        truthy(&item)
                    ),
                ))
            }
            (LogicalOperator::And, Ty::Opt(_)) => {
                Err("`&&` on a value that may be absent has no Rust lowering outside text".into())
            }
            (LogicalOperator::And, _) => {
                let item = Val { code: x.clone(), ..left.clone() };
                let (a, b, template) = unify(&item, right)?;
                Ok(with_code(
                    template,
                    format!(
                        "{{ let {x} = {}; if {} {{ {b} }} else {{ {a} }} }}",
                        left.code,
                        truthy(&item)
                    ),
                ))
            }
        }
    }

    /// Statements that print `val`.
    pub(super) fn output(&mut self, val: &Val, sink: Sink<'_>) -> Result<(), String> {
        let out = self.out.clone();
        let code = &val.code;
        match (&val.ty, sink) {
            (Ty::Null, _) => {}
            (Ty::Bool, Sink::Text) => {}
            (Ty::Str, Sink::Text) => self.stmt(&format!("{out}.text({});", val.str())),
            (Ty::Str, Sink::Attr(name)) => {
                self.stmt(&format!("{out}.attr({}, {});", literal(name), val.str()));
            }
            (Ty::Enum(_), Sink::Text) => self.stmt(&format!("{out}.text({VARIANT}({code}));")),
            (Ty::Enum(_), Sink::Attr(name)) => {
                self.stmt(&format!("{out}.attr({}, {VARIANT}({code}));", literal(name)));
            }
            (Ty::Num(_), Sink::Text) if val.ty.is_float() => {
                self.stmt(&format!("{out}.number({code} as f64);"));
            }
            (Ty::Num(_), Sink::Text) => self.stmt(&format!("{out}.display({code});")),
            (Ty::Num(_), Sink::Attr(name)) if val.ty.is_float() => {
                self.stmt(&format!("{out}.attr({}, &{JS}::number({code} as f64));", literal(name)));
            }
            (Ty::Num(_), Sink::Attr(name)) => {
                self.stmt(&format!("{out}.attr({}, &({code}).to_string());", literal(name)));
            }
            (Ty::Bool, Sink::Attr(name)) if name.starts_with("aria-") => self.stmt(&format!(
                "{out}.raw(if {code} {{ {} }} else {{ {} }});",
                literal(&format!(" {name}=\"true\"")),
                literal(&format!(" {name}=\"false\""))
            )),
            (Ty::Bool, Sink::Attr(name)) => {
                self.stmt(&format!("if {code} {{ {out}.raw({}); }}", literal(&format!(" {name}"))));
            }
            (Ty::Opt(inner), _) => {
                let x = self.fresh("x");
                self.open(&format!("if let Some({x}) = {code}"));
                let result = self.output(&Val::new(x, (**inner).clone()), sink);
                self.close();
                result?;
            }
            (
                Ty::Struct(_) | Ty::List(_) | Ty::Tuple(_) | Ty::Date | Ty::Set(_) | Ty::Map(_),
                _,
            ) => {
                return Err("An object or a list prints as text, which has no Rust lowering".into());
            }
        }
        Ok(())
    }
}

/// The key of a computed member access: a string, or an integer index of a tuple.
fn literal_key(key: &Expression<'_>) -> Result<String, String> {
    match key.without_parentheses() {
        Expression::StringLiteral(key) => Ok(key.value.to_string()),
        Expression::NumericLiteral(index) if index.value.fract() == 0.0 && index.value >= 0.0 => {
            Ok((index.value as usize).to_string())
        }
        _ => Err("Computed member access has no Rust lowering".into()),
    }
}

/// `.length` of a string, in UTF-16 units, or of a list.
fn length(val: &Val, chain: bool) -> Result<Val, String> {
    let usize = Ty::Num("usize".into());
    match &val.ty {
        Ty::Str => Ok(Val::new(format!("{JS}::len({})", val.str()), usize)),
        Ty::List(_) => Ok(Val::new(format!("({}).len()", val.code), usize)),
        Ty::Opt(inner) if chain && matches!(**inner, Ty::Str | Ty::List(_)) => {
            let len = if **inner == Ty::Str { format!("{JS}::len(x)") } else { "x.len()".into() };
            Ok(Val::new(format!("({}).map(|x| {len})", val.code), Ty::Opt(Box::new(usize))))
        }
        _ => Err("`.length` is read from a value without a length".into()),
    }
}

/// Sets `key` in an object, keeping the position of an earlier value as JS does.
pub(super) fn set<'e, 'a>(
    fields: &mut Vec<(String, Binding<'e, 'a>)>,
    key: String,
    value: Binding<'e, 'a>,
) {
    match fields.iter_mut().find(|(k, _)| *k == key) {
        Some(field) => field.1 = value,
        None => fields.push((key, value)),
    }
}
