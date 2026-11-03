//! Data contracts of loaders and mutations: the type IR the compiler lowers from annotations, and
//! the validators generated from it.

use std::fmt::Write;

use oxc_ast::AstKind;
use oxc_ast::ast::{
    Argument, CallExpression, Declaration, Expression, FormalParameter, Statement, TSLiteral,
    TSSignature, TSType, TSTypeName, TSTypeOperatorOperator, TSTypeParameterInstantiation,
    UnaryOperator,
};
use oxc_span::{GetSpan, Span};

use super::{Api, Context, Export, ImportSource};
use crate::diagnostic::{Code, Report};
use crate::view::{Attribute, AttributeItem, Branch, ComponentRef, Node, PropValue, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Int {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

impl Int {
    fn from_name(name: &str) -> Option<Int> {
        Some(match name {
            "U8" => Int::U8,
            "U16" => Int::U16,
            "U32" => Int::U32,
            "U64" => Int::U64,
            "I8" => Int::I8,
            "I16" => Int::I16,
            "I32" => Int::I32,
            "I64" => Int::I64,
            _ => return None,
        })
    }

    fn range(self) -> (i128, i128) {
        match self {
            Int::U8 => (0, u8::MAX.into()),
            Int::U16 => (0, u16::MAX.into()),
            Int::U32 => (0, u32::MAX.into()),
            Int::U64 => (0, u64::MAX.into()),
            Int::I8 => (i8::MIN.into(), i8::MAX.into()),
            Int::I16 => (i16::MIN.into(), i16::MAX.into()),
            Int::I32 => (i32::MIN.into(), i32::MAX.into()),
            Int::I64 => (i64::MIN.into(), i64::MAX.into()),
        }
    }

    /// `U64` and `I64` are `bigint`, the rest `number`.
    fn is_big(self) -> bool {
        matches!(self, Int::U64 | Int::I64)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    String(String),
    Number(f64),
    Boolean(bool),
}

impl Literal {
    fn js(&self) -> String {
        match self {
            Literal::String(s) => serde_json::to_string(s).unwrap(),
            Literal::Number(n) => n.to_string(),
            Literal::Boolean(b) => b.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    String,
    Number,
    Int(Int),
    BigInt,
    Boolean,
    Null,
    Undefined,
    Literal(Literal),
    Union(Vec<Ty>),
    Object(Vec<Field>),
    Array(Box<Ty>),
    Tuple(Vec<Ty>),
    Record(Box<Ty>),
    Map(Box<Ty>),
    Set(Box<Ty>),
    Date,
    Bytes,
    File,
}

impl Ty {
    fn union(types: Vec<Ty>) -> Ty {
        let mut flat = Vec::new();
        for ty in types {
            match ty {
                Ty::Union(inner) => flat.extend(inner),
                ty if !flat.contains(&ty) => flat.push(ty),
                _ => {}
            }
        }
        if flat.len() == 1 { flat.pop().unwrap() } else { Ty::Union(flat) }
    }

    /// The type without `undefined` and `null`, and whether each was present.
    fn strip(&self) -> (Ty, bool, bool) {
        let Ty::Union(members) = self else { return (self.clone(), false, false) };
        let undefined = members.contains(&Ty::Undefined);
        let null = members.contains(&Ty::Null);
        let rest: Vec<Ty> =
            members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).cloned().collect();
        let base = match rest.len() {
            0 if null => Ty::Null,
            0 => Ty::Undefined,
            _ => Ty::union(rest),
        };
        (base, undefined, null)
    }
}

/// Why an annotation does not lower.
pub enum Unlowered {
    /// `unknown`, `any`, or no annotation: no contract at all.
    Untyped,
    Unresolved(String),
}

type Resolved = Result<Ty, Unlowered>;

fn unresolved(reason: impl Into<String>) -> Unlowered {
    Unlowered::Unresolved(reason.into())
}

const UTILITY_TYPES: [&str; 12] = [
    "Pick",
    "Omit",
    "Partial",
    "Required",
    "Readonly",
    "Exclude",
    "Extract",
    "NonNullable",
    "ReturnType",
    "Awaited",
    "Parameters",
    "InstanceType",
];

/// A named type declaration the resolver can expand.
enum Named<'a> {
    Interface(&'a oxc_ast::ast::TSInterfaceDeclaration<'a>),
    Alias(&'a oxc_ast::ast::TSTypeAliasDeclaration<'a>),
    Int(Int),
}

pub struct Resolver<'c, 'm, 'a> {
    pub cx: &'c Context<'m, 'a>,
    depth: u32,
}

impl<'c, 'm, 'a> Resolver<'c, 'm, 'a> {
    pub fn new(cx: &'c Context<'m, 'a>) -> Self {
        Self { cx, depth: 0 }
    }

    pub fn resolve(&mut self, module: usize, ty: &'a TSType<'a>) -> Resolved {
        self.depth += 1;
        let result = if self.depth > 32 {
            Err(unresolved("recursive types are not lowered"))
        } else {
            self.resolve_inner(module, ty)
        };
        self.depth -= 1;
        result
    }

    fn resolve_inner(&mut self, module: usize, ty: &'a TSType<'a>) -> Resolved {
        Ok(match ty {
            TSType::TSStringKeyword(_) => Ty::String,
            TSType::TSNumberKeyword(_) => Ty::Number,
            TSType::TSBooleanKeyword(_) => Ty::Boolean,
            TSType::TSBigIntKeyword(_) => Ty::BigInt,
            TSType::TSNullKeyword(_) => Ty::Null,
            TSType::TSUndefinedKeyword(_) | TSType::TSVoidKeyword(_) => Ty::Undefined,
            TSType::TSAnyKeyword(_) | TSType::TSUnknownKeyword(_) => {
                return Err(Unlowered::Untyped);
            }
            TSType::TSParenthesizedType(inner) => {
                return self.resolve(module, &inner.type_annotation);
            }
            TSType::TSLiteralType(literal) => Ty::Literal(match &literal.literal {
                TSLiteral::StringLiteral(s) => Literal::String(s.value.to_string()),
                TSLiteral::NumericLiteral(n) => Literal::Number(n.value),
                TSLiteral::BooleanLiteral(b) => Literal::Boolean(b.value),
                TSLiteral::UnaryExpression(unary)
                    if unary.operator == UnaryOperator::UnaryNegation
                        && let Expression::NumericLiteral(n) = &unary.argument =>
                {
                    Literal::Number(-n.value)
                }
                _ => return Err(unresolved("template and bigint literal types are not lowered")),
            }),
            TSType::TSUnionType(union) => {
                let mut members = Vec::new();
                for member in &union.types {
                    members.push(self.resolve(module, member)?);
                }
                Ty::union(members)
            }
            TSType::TSTypeLiteral(literal) => Ty::Object(self.members(module, &literal.members)?),
            TSType::TSArrayType(array) => {
                Ty::Array(Box::new(self.resolve(module, &array.element_type)?))
            }
            TSType::TSTypeOperatorType(operator)
                if operator.operator == TSTypeOperatorOperator::Readonly =>
            {
                return self.resolve(module, &operator.type_annotation);
            }
            TSType::TSTupleType(tuple) => {
                let mut elements = Vec::new();
                for element in &tuple.element_types {
                    let Some(inner) = element.as_ts_type() else {
                        return Err(unresolved("optional and rest tuple elements are not lowered"));
                    };
                    elements.push(self.resolve(module, inner)?);
                }
                Ty::Tuple(elements)
            }
            TSType::TSTypeReference(reference) => {
                return self.reference(
                    module,
                    &reference.type_name,
                    reference.type_arguments.as_deref(),
                );
            }
            _ => return Err(unresolved("mapped, conditional, and function types are not lowered")),
        })
    }

    fn members(
        &mut self,
        module: usize,
        members: &'a [TSSignature<'a>],
    ) -> Result<Vec<Field>, Unlowered> {
        let mut fields = Vec::new();
        for member in members {
            let TSSignature::TSPropertySignature(property) = member else {
                return Err(unresolved("index signatures and methods are not lowered"));
            };
            let Some(name) = property.key.static_name().filter(|_| !property.computed) else {
                return Err(unresolved("computed property names are not lowered"));
            };
            let Some(annotation) = &property.type_annotation else {
                return Err(unresolved(format!("property `{name}` has no type")));
            };
            let ty = self.resolve(module, &annotation.type_annotation)?;
            fields.push(Field { name: name.to_string(), ty, optional: property.optional });
        }
        Ok(fields)
    }

    fn argument(
        &mut self,
        module: usize,
        arguments: Option<&'a TSTypeParameterInstantiation<'a>>,
        index: usize,
        name: &str,
    ) -> Resolved {
        match arguments.and_then(|a| a.params.get(index)) {
            Some(argument) => self.resolve(module, argument),
            None => Err(unresolved(format!("`{name}` needs its type arguments"))),
        }
    }

    fn reference(
        &mut self,
        module: usize,
        name: &'a TSTypeName<'a>,
        arguments: Option<&'a TSTypeParameterInstantiation<'a>>,
    ) -> Resolved {
        let TSTypeName::IdentifierReference(identifier) = name else {
            return Err(unresolved(format!("`{}` has no lowering", name)));
        };
        let text = identifier.name.as_str();
        if let Some(symbol) = self.cx.symbol(module, identifier) {
            return match self.named(module, symbol, 0) {
                Some((target, Named::Interface(interface))) => {
                    let mut fields = Vec::new();
                    for heritage in &interface.extends {
                        match self.reference(
                            target,
                            &heritage.type_name,
                            heritage.type_arguments.as_deref(),
                        )? {
                            Ty::Object(inherited) => fields.extend(inherited),
                            _ => {
                                return Err(unresolved(
                                    "an interface extends a type that is not an object",
                                ));
                            }
                        }
                    }
                    for field in self.members(target, &interface.body.body)? {
                        fields.retain(|f: &Field| f.name != field.name);
                        fields.push(field);
                    }
                    if interface.type_parameters.is_some() {
                        return Err(unresolved(format!("generic `{text}` is not lowered")));
                    }
                    Ok(Ty::Object(fields))
                }
                Some((target, Named::Alias(alias))) => {
                    if alias.type_parameters.is_some() {
                        return Err(unresolved(format!("generic `{text}` is not lowered")));
                    }
                    self.resolve(target, &alias.type_annotation)
                }
                Some((_, Named::Int(int))) => Ok(Ty::Int(int)),
                None => Err(self.foreign(module, symbol, text)),
            };
        }
        Ok(match text {
            "Array" | "ReadonlyArray" => {
                Ty::Array(Box::new(self.argument(module, arguments, 0, text)?))
            }
            "Set" | "ReadonlySet" => Ty::Set(Box::new(self.argument(module, arguments, 0, text)?)),
            "Record" | "Map" | "ReadonlyMap" => {
                let key = self.argument(module, arguments, 0, text)?;
                if key != Ty::String {
                    return Err(unresolved(format!(
                        "`{text}` keys other than `string` are not lowered"
                    )));
                }
                let value = Box::new(self.argument(module, arguments, 1, text)?);
                if text == "Record" { Ty::Record(value) } else { Ty::Map(value) }
            }
            "Date" => Ty::Date,
            "Uint8Array" => Ty::Bytes,
            "File" => Ty::File,
            _ if UTILITY_TYPES.contains(&text) => {
                return Err(unresolved("mapped and utility types are not lowered"));
            }
            _ => return Err(unresolved(format!("`{text}` has no lowering"))),
        })
    }

    /// Why a symbol that is not a project type does not lower.
    fn foreign(&self, module: usize, symbol: oxc_semantic::SymbolId, text: &str) -> Unlowered {
        match self.cx.facts[module].imports.get(&symbol).map(|i| &i.source) {
            Some(ImportSource::Package(_)) => {
                unresolved("types from packages without source are not resolved")
            }
            _ => unresolved(format!("`{text}` is not an interface or a type alias")),
        }
    }

    /// The declaration a type symbol names, following imports between project modules.
    fn named(
        &self,
        module: usize,
        symbol: oxc_semantic::SymbolId,
        depth: u32,
    ) -> Option<(usize, Named<'a>)> {
        if depth > 32 {
            return None;
        }
        if let Some(import) = self.cx.facts[module].imports.get(&symbol) {
            return match &import.source {
                ImportSource::Package(package) if package == "reze-js/types" => {
                    Int::from_name(&import.name).map(|int| (module, Named::Int(int)))
                }
                ImportSource::Package(_) => None,
                ImportSource::Module(target) => self.exported(*target, &import.name, depth + 1),
            };
        }
        let semantic = &self.cx.modules[module].semantic;
        let node = semantic.scoping().symbol_declaration(symbol);
        match semantic.nodes().get_node(node).kind() {
            AstKind::TSInterfaceDeclaration(d) => Some((module, Named::Interface(d))),
            AstKind::TSTypeAliasDeclaration(d) => Some((module, Named::Alias(d))),
            _ => None,
        }
    }

    fn exported(&self, module: usize, name: &str, depth: u32) -> Option<(usize, Named<'a>)> {
        for statement in &self.cx.modules[module].program.body {
            let Statement::ExportDeclaration(export) = statement else { continue };
            match &export.declaration {
                Declaration::TSInterfaceDeclaration(d) if d.id.name == name => {
                    return Some((module, Named::Interface(d)));
                }
                Declaration::TSTypeAliasDeclaration(d) if d.id.name == name => {
                    return Some((module, Named::Alias(d)));
                }
                _ => {}
            }
        }
        match self.cx.facts[module].exports.get(name)? {
            Export::Local(symbol) => self.named(module, *symbol, depth + 1),
            Export::Reexport { module: target, name } => self.exported(*target, name, depth + 1),
            Export::Component(_) => None,
        }
    }
}

/// The type annotation of a function's first parameter.
pub fn first_param<'a>(function: &'a Expression<'a>) -> Option<&'a FormalParameter<'a>> {
    match function.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => arrow.params.items.first(),
        Expression::FunctionExpression(function) => function.params.items.first(),
        _ => None,
    }
}

/// The arguments a loader function destructures from its first parameter.
fn loader_reads(call: &CallExpression<'_>) -> Option<Vec<String>> {
    let function = call.arguments.last().and_then(Argument::as_expression)?;
    if !matches!(
        function.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    ) {
        return None;
    }
    let Some(param) = first_param(function) else { return Some(Vec::new()) };
    let oxc_ast::ast::BindingPattern::ObjectPattern(object) = &param.pattern else { return None };
    if object.rest.is_some() {
        return None;
    }
    object
        .properties
        .iter()
        .map(|property| property.key.static_name().map(|n| n.to_string()))
        .collect()
}

/// The result annotation of a loader: `loader<T>(…)`, or the function's return type, with
/// `Promise` unwrapped.
pub fn loader_result<'a>(call: &'a CallExpression<'a>) -> Option<&'a TSType<'a>> {
    if let Some(argument) = call.type_arguments.as_ref().and_then(|a| a.params.first()) {
        return Some(argument);
    }
    let function = call.arguments.last().and_then(Argument::as_expression)?;
    let annotation = match function.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => arrow.return_type.as_ref(),
        Expression::FunctionExpression(function) => function.return_type.as_ref(),
        _ => None,
    }?;
    let ty = &annotation.type_annotation;
    if let TSType::TSTypeReference(reference) = ty
        && matches!(&reference.type_name, TSTypeName::IdentifierReference(id) if id.name == "Promise")
    {
        return reference.type_arguments.as_ref().and_then(|a| a.params.first());
    }
    Some(ty)
}

/// The type of property `name` in an annotation written as an object literal, or an
/// intersection with one, `LoaderArgs & { params: { id: number } }`.
pub fn member<'a>(ty: &'a TSType<'a>, name: &str) -> Option<&'a TSType<'a>> {
    match ty {
        TSType::TSIntersectionType(intersection) => {
            intersection.types.iter().find_map(|ty| member(ty, name))
        }
        TSType::TSParenthesizedType(inner) => member(&inner.type_annotation, name),
        TSType::TSTypeLiteral(literal) => literal.members.iter().find_map(|member| match member {
            TSSignature::TSPropertySignature(p) if p.key.static_name().as_deref() == Some(name) => {
                p.type_annotation.as_ref().map(|a| &a.type_annotation)
            }
            _ => None,
        }),
        _ => None,
    }
}

/// Base-36 FNV-1a of `module#export`: a mutation id that stays the same across builds.
pub fn mutation_id(module: &str, export: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in module.bytes().chain(*b"#").chain(export.bytes()) {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    crate::client::base36(hash as usize)
}

const ISSUE: &str = "const e = (p, m) => void i.push({ message: m, path: p });";

fn describe(ty: &Ty) -> String {
    match ty {
        Ty::String => "a string".into(),
        Ty::Number => "a number".into(),
        Ty::Int(int) => {
            let (min, max) = int.range();
            format!("an integer from {min} to {max}")
        }
        Ty::BigInt => "an integer".into(),
        Ty::Boolean => "a boolean".into(),
        Ty::Null => "null".into(),
        Ty::Undefined => "nothing".into(),
        Ty::Literal(literal) => literal.js(),
        Ty::Union(members) => members.iter().map(describe).collect::<Vec<_>>().join(" or "),
        Ty::Object(_) | Ty::Record(_) => "an object".into(),
        Ty::Array(_) | Ty::Tuple(_) | Ty::Set(_) => "an array".into(),
        Ty::Map(_) => "an array of [key, value] pairs".into(),
        Ty::Date => "a date".into(),
        Ty::Bytes => "base64 bytes".into(),
        Ty::File => "a file".into(),
    }
}

fn expected(ty: &Ty) -> String {
    serde_json::to_string(&format!("Expected {}", describe(ty))).unwrap()
}

fn big_range(int: Int) -> String {
    let (min, max) = int.range();
    format!("b >= {min}n && b <= {max}n")
}

/// A JS function `(x, p) => value` that checks a JSON value against `ty` and restores the values
/// JSON cannot carry. It reports through `e` and `i` of the enclosing parser.
fn json_check(ty: &Ty) -> String {
    let fail = format!("e(p, {})", expected(ty));
    match ty {
        Ty::String => format!("(x, p) => typeof x === \"string\" ? x : {fail}"),
        Ty::Number => {
            format!("(x, p) => typeof x === \"number\" && Number.isFinite(x) ? x : {fail}")
        }
        Ty::Int(int) if int.is_big() => format!(
            "(x, p) => {{ if (typeof x === \"string\" && /^-?\\d+$/.test(x)) {{ const b = BigInt(x); if ({}) return b; }} return {fail}; }}",
            big_range(*int)
        ),
        Ty::Int(int) => {
            let (min, max) = int.range();
            format!("(x, p) => Number.isInteger(x) && x >= {min} && x <= {max} ? x : {fail}")
        }
        Ty::BigInt => {
            format!("(x, p) => typeof x === \"string\" && /^-?\\d+$/.test(x) ? BigInt(x) : {fail}")
        }
        Ty::Boolean => format!("(x, p) => typeof x === \"boolean\" ? x : {fail}"),
        Ty::Null => format!("(x, p) => x === null ? x : {fail}"),
        Ty::Undefined => format!("(x, p) => x === undefined ? x : {fail}"),
        Ty::Literal(literal) => format!("(x, p) => x === {} ? x : {fail}", literal.js()),
        Ty::Union(members) => {
            let checks: Vec<String> =
                members.iter().map(|m| format!("({})", json_check(m))).collect();
            format!(
                "(x, p) => {{ for (const c of [{}]) {{ const n = i.length; const v = c(x, p); if (i.length === n) return v; i.length = n; }} return {fail}; }}",
                checks.join(", ")
            )
        }
        Ty::Object(fields) => {
            let mut body = format!(
                "(x, p) => {{ if (typeof x !== \"object\" || x === null || Array.isArray(x)) return {fail}; const o = {{}};"
            );
            for field in fields {
                let key = serde_json::to_string(&field.name).unwrap();
                let check = json_check(&field.ty);
                if field.optional {
                    write!(body, " if (x[{key}] !== undefined) o[{key}] = ({check})(x[{key}], [...p, {key}]);").unwrap();
                } else {
                    write!(body, " o[{key}] = ({check})(x[{key}], [...p, {key}]);").unwrap();
                }
            }
            body.push_str(" return o; }");
            body
        }
        Ty::Array(element) => format!(
            "(x, p) => Array.isArray(x) ? x.map((v, k) => ({})(v, [...p, k])) : {fail}",
            json_check(element)
        ),
        Ty::Set(element) => format!(
            "(x, p) => Array.isArray(x) ? new Set(x.map((v, k) => ({})(v, [...p, k]))) : {fail}",
            json_check(element)
        ),
        Ty::Tuple(elements) => {
            let items: Vec<String> = elements
                .iter()
                .enumerate()
                .map(|(k, element)| format!("({})(x[{k}], [...p, {k}])", json_check(element)))
                .collect();
            format!(
                "(x, p) => Array.isArray(x) && x.length === {} ? [{}] : {fail}",
                elements.len(),
                items.join(", ")
            )
        }
        Ty::Record(value) => format!(
            "(x, p) => typeof x === \"object\" && x !== null && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).map(([k, v]) => [k, ({})(v, [...p, k])])) : {fail}",
            json_check(value)
        ),
        Ty::Map(value) => format!(
            "(x, p) => Array.isArray(x) && x.every((v) => Array.isArray(v) && v.length === 2 && typeof v[0] === \"string\") ? new Map(x.map(([k, v]) => [k, ({})(v, [...p, k])])) : {fail}",
            json_check(value)
        ),
        Ty::Date => format!(
            "(x, p) => {{ const d = typeof x === \"string\" || typeof x === \"number\" ? new Date(x) : undefined; return d && !Number.isNaN(d.getTime()) ? d : {fail}; }}"
        ),
        Ty::Bytes => format!(
            "(x, p) => {{ try {{ return Uint8Array.from(atob(x), (c) => c.charCodeAt(0)); }} catch {{ return {fail}; }} }}"
        ),
        Ty::File => "(x, p) => e(p, \"Files are sent through a form\")".into(),
    }
}

/// The JS parser of a mutation's JSON input: `(x, i) => value`.
pub fn json_parser(ty: &Ty) -> String {
    format!("(x, i) => {{ {ISSUE} return ({})(x, []); }}", json_check(ty))
}

/// A JS function `(v, p) => value` that coerces one string form value, or `None` when the type is
/// not a form scalar.
fn scalar(ty: &Ty) -> Option<String> {
    let fail = format!("e(p, {})", expected(ty));
    Some(match ty {
        Ty::String => format!("(v, p) => typeof v === \"string\" ? v : {fail}"),
        Ty::Number => format!(
            "(v, p) => {{ const n = typeof v === \"string\" && v.trim() !== \"\" ? Number(v) : NaN; return Number.isFinite(n) ? n : {fail}; }}"
        ),
        Ty::Int(int) if int.is_big() => format!(
            "(v, p) => {{ if (typeof v === \"string\" && /^-?\\d+$/.test(v)) {{ const b = BigInt(v); if ({}) return b; }} return {fail}; }}",
            big_range(*int)
        ),
        Ty::Int(int) => {
            let (min, max) = int.range();
            format!(
                "(v, p) => {{ const n = typeof v === \"string\" && /^-?\\d+$/.test(v) ? Number(v) : NaN; return n >= {min} && n <= {max} ? n : {fail}; }}"
            )
        }
        Ty::BigInt => {
            format!("(v, p) => typeof v === \"string\" && /^-?\\d+$/.test(v) ? BigInt(v) : {fail}")
        }
        Ty::Boolean => format!(
            "(v, p) => v === \"on\" || v === \"true\" ? true : v === \"false\" ? false : {fail}"
        ),
        Ty::Literal(literal) => {
            format!("(v, p) => v === String({0}) ? {0} : {fail}", literal.js())
        }
        Ty::Union(members) if members.iter().all(|m| matches!(m, Ty::Literal(_))) => {
            let values: Vec<String> = members
                .iter()
                .map(|m| match m {
                    Ty::Literal(literal) => literal.js(),
                    _ => unreachable!(),
                })
                .collect();
            format!(
                "(v, p) => {{ for (const c of [{}]) if (String(c) === v) return c; return {fail}; }}",
                values.join(", ")
            )
        }
        Ty::Date => format!(
            "(v, p) => {{ const d = typeof v === \"string\" ? new Date(v) : undefined; return d && !Number.isNaN(d.getTime()) ? d : {fail}; }}"
        ),
        Ty::File => format!("(v, p) => typeof v === \"object\" && v !== null ? v : {fail}"),
        _ => return None,
    })
}

/// The JS coercer of named string inputs, `(g, i, l) => object`, for form fields, params, and
/// search params. `g(name)` returns every value of a name; `l` turns a failed optional value into
/// `undefined`. `None` when a field is not a form scalar or a list of them.
pub fn fields_coercer(ty: &Ty) -> Option<String> {
    let Ty::Object(fields) = ty else { return None };
    let mut body = format!("(g, i, l) => {{ {ISSUE} const o = {{}};");
    for field in fields {
        let key = serde_json::to_string(&field.name).unwrap();
        let (base, undefined, null) = field.ty.strip();
        let absent = field.optional || undefined;
        match &base {
            Ty::Array(element) | Ty::Set(element) => {
                let check = scalar(element)?;
                let list = format!("g({key}).map((v, k) => ({check})(v, [{key}, k]))");
                let value =
                    if matches!(base, Ty::Set(_)) { format!("new Set({list})") } else { list };
                write!(body, " o[{key}] = {value};").unwrap();
            }
            _ => {
                let check = scalar(&base)?;
                let missing = if absent {
                    String::new()
                } else if null {
                    format!("o[{key}] = null;")
                } else if base == Ty::Boolean {
                    format!("o[{key}] = false;")
                } else if base == Ty::String {
                    format!("if (v === \"\") o[{key}] = v; else e([{key}], \"Required\");")
                } else {
                    format!("e([{key}], \"Required\");")
                };
                let present = if absent {
                    format!(
                        "{{ const n = i.length; const c = ({check})(v, [{key}]); if (l && i.length > n) i.length = n; else o[{key}] = c; }}"
                    )
                } else {
                    format!("o[{key}] = ({check})(v, [{key}]);")
                };
                write!(
                    body,
                    " {{ const v = g({key})[0]; if (v === undefined || v === \"\") {{ {missing} }} else {present} }}"
                )
                .unwrap();
            }
        }
    }
    body.push_str(" return o; }");
    Some(body)
}

/// Names of the fields of an object type.
pub fn field_names(ty: &Ty) -> Option<Vec<&str>> {
    match ty {
        Ty::Object(fields) => Some(fields.iter().map(|f| f.name.as_str()).collect()),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    Loader,
    Mutation,
}

/// An exported `loader(…)` or `mutation(…)` call, which the server code wraps with its metadata.
pub struct Endpoint {
    pub kind: EndpointKind,
    pub export: String,
    pub symbol: Option<oxc_semantic::SymbolId>,
    pub call: Span,
    /// The mutation id.
    pub id: Option<String>,
    /// A JS object literal: the generated coercers and parsers.
    pub meta: String,
    /// The input type of a mutation without a schema.
    pub input: Option<Ty>,
    /// Whether a loader annotates its result.
    pub typed: bool,
    /// The result type of a loader, when it lowers.
    pub data: Option<Ty>,
    /// The pattern of the route that exports the endpoint.
    pub route: Option<String>,
    /// The loader arguments the function reads, `None` when it may read any of them.
    pub reads: Option<Vec<String>>,
    /// `loader.external<T>()`: the Rust server loads the data.
    pub external: bool,
    /// The Rust path of an external loader's type, `crate::Name`, when it comes from
    /// `virtual:reze-types/<crate>`.
    pub data_type: Option<String>,
}

fn report_unlowered(reports: &mut Vec<Report>, unlowered: Unlowered, span: Span) {
    reports.push(match unlowered {
        Unlowered::Untyped => Report::new(Code::MutationInputUntyped, span),
        Unlowered::Unresolved(reason) => {
            Report::new(Code::MutationInputUnresolved, span).arg("reason", capitalize(&reason))
        }
    });
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The endpoints a module exports, with the reports of contracts that do not lower.
pub fn endpoints<'a>(
    cx: &Context<'_, 'a>,
    module: usize,
    pattern: Option<(&str, Vec<&str>)>,
    reports: &mut Vec<Report>,
) -> Vec<Endpoint> {
    let mut out = Vec::new();
    for statement in &cx.modules[module].program.body {
        let Statement::ExportDeclaration(export) = statement else { continue };
        let Declaration::VariableDeclaration(declaration) = &export.declaration else { continue };
        for declarator in &declaration.declarations {
            let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let Some(Expression::CallExpression(call)) =
                declarator.init.as_ref().map(Expression::without_parentheses)
            else {
                continue;
            };
            let external = match call.callee.without_parentheses() {
                Expression::StaticMemberExpression(member) => {
                    member.property.name == "external"
                        && cx.api(module, &member.object) == Some(Api::Loader)
                }
                _ => false,
            };
            let kind = match cx.api(module, &call.callee) {
                _ if external => EndpointKind::Loader,
                Some(Api::Loader) => EndpointKind::Loader,
                Some(Api::Mutation) => EndpointKind::Mutation,
                _ => continue,
            };
            let mut endpoint = Endpoint {
                kind,
                export: id.name.to_string(),
                symbol: id.symbol_id.get(),
                call: call.span,
                id: None,
                meta: "{}".into(),
                input: None,
                typed: loader_result(call).is_some(),
                data: loader_result(call).and_then(|ty| Resolver::new(cx).resolve(module, ty).ok()),
                route: pattern.as_ref().map(|(route, _)| route.to_string()),
                reads: loader_reads(call),
                external,
                data_type: None,
            };
            match kind {
                EndpointKind::Loader if external => {
                    endpoint.meta = format!("{{ external: {:?} }}", cx.modules[module].path);
                    endpoint.typed = true;
                    endpoint.reads = None;
                    endpoint.data_type = external_type(cx, module, call);
                }
                EndpointKind::Loader => {
                    endpoint.meta = loader_meta(cx, module, call, pattern.as_ref(), reports);
                }
                EndpointKind::Mutation => {
                    endpoint.id = Some(mutation_id(&cx.modules[module].path, &id.name));
                    let (meta, input) = mutation_meta(cx, module, call, reports);
                    endpoint.meta = meta;
                    endpoint.input = input;
                }
            }
            out.push(endpoint);
        }
    }
    out
}

/// The type at `path`, a list of field names, inside `ty`.
pub fn field_type(ty: &Ty, path: &[&str]) -> Option<Ty> {
    let Some((first, rest)) = path.split_first() else { return Some(ty.clone()) };
    let (base, undefined, null) = ty.strip();
    let Ty::Object(fields) = base else { return None };
    let field = fields.iter().find(|field| field.name == *first)?;
    let inner = field_type(&field.ty, rest)?;
    let nullable = undefined || null || field.optional;
    Some(if nullable { Ty::union(vec![inner, Ty::Undefined]) } else { inner })
}

/// The JS expression that converts `code`, a value of `ty` read from the state block, into its
/// native form: a `Date`, a `bigint`, a `Map`, or a `Set`. `None` when the value reads as it is.
pub fn restore(ty: &Ty, code: &str, depth: usize) -> Option<String> {
    let v = format!("$v{depth}");
    let (base, undefined, null) = ty.strip();
    if undefined || null {
        let inner = restore(&base, code, depth)?;
        return Some(format!("({code} == null ? {code} : {inner})"));
    }
    Some(match &base {
        Ty::Date => format!("new Date({code})"),
        Ty::BigInt => format!("BigInt({code})"),
        Ty::Int(int) if int.is_big() => format!("BigInt({code})"),
        Ty::Set(inner) => match restore(inner, &v, depth + 1) {
            Some(inner) => format!("new Set({code}.map(({v}) => {inner}))"),
            None => format!("new Set({code})"),
        },
        Ty::Map(inner) => match restore(inner, &v, depth + 1) {
            Some(inner) => {
                format!("new Map({code}.map(([$k{depth}, {v}]) => [$k{depth}, {inner}]))")
            }
            None => format!("new Map({code})"),
        },
        Ty::Array(inner) => format!("{code}.map(({v}) => {})", restore(inner, &v, depth + 1)?),
        Ty::Record(inner) => format!(
            "Object.fromEntries(Object.entries({code}).map(([$k{depth}, {v}]) => [$k{depth}, {}]))",
            restore(inner, &v, depth + 1)?
        ),
        Ty::Tuple(elements) => {
            let restored: Vec<Option<String>> = elements
                .iter()
                .enumerate()
                .map(|(i, element)| restore(element, &format!("{code}[{i}]"), depth))
                .collect();
            if restored.iter().all(Option::is_none) {
                return None;
            }
            let items: Vec<String> = restored
                .into_iter()
                .enumerate()
                .map(|(i, r)| r.unwrap_or_else(|| format!("{code}[{i}]")))
                .collect();
            format!("[{}]", items.join(", "))
        }
        Ty::Object(fields) => {
            let restored: Vec<String> = fields
                .iter()
                .filter_map(|field| {
                    let ty = if field.optional {
                        Ty::union(vec![field.ty.clone(), Ty::Undefined])
                    } else {
                        field.ty.clone()
                    };
                    let name = serde_json::to_string(&field.name).unwrap();
                    restore(&ty, &format!("{v}[{name}]"), depth + 1).map(|r| format!("{name}: {r}"))
                })
                .collect();
            if restored.is_empty() {
                return None;
            }
            format!("(({v}) => ({{ ...{v}, {} }}))({code})", restored.join(", "))
        }
        _ => return None,
    })
}

/// The props type of a component, from the annotation of its first parameter. The default export
/// of a route module types `data` with the result of its `load`.
pub fn props_type(cx: &Context<'_, '_>, module: usize, index: usize) -> Option<Ty> {
    let component = &cx.facts[module].components[index];
    let annotation =
        component.function.params().items.first().and_then(|p| p.type_annotation.as_ref());
    let props = annotation.and_then(|a| Resolver::new(cx).resolve(module, &a.type_annotation).ok());
    let data = cx.facts[module]
        .endpoints
        .iter()
        .find(|e| e.kind == EndpointKind::Loader && e.export == "load")
        .and_then(|e| e.data.clone())
        .filter(|_| cx.exported(module, "default") == Some(ComponentRef { module, index }));
    let Some(data) = data else { return props };
    let mut fields = match props {
        Some(Ty::Object(fields)) => fields,
        _ => Vec::new(),
    };
    fields.retain(|field| field.name != "data");
    fields.push(Field { name: "data".into(), ty: data, optional: false });
    Some(Ty::Object(fields))
}

/// `crate::Name` of the type argument of `loader.external<Name>()`, imported from
/// `virtual:reze-types/<crate>`.
fn external_type(cx: &Context<'_, '_>, module: usize, call: &CallExpression<'_>) -> Option<String> {
    let argument = call.type_arguments.as_ref()?.params.first()?;
    let TSType::TSTypeReference(reference) = argument else { return None };
    let TSTypeName::IdentifierReference(id) = &reference.type_name else { return None };
    let symbol = cx.symbol(module, id)?;
    let import = cx.facts[module].imports.get(&symbol)?;
    let ImportSource::Package(package) = &import.source else { return None };
    let krate = package.strip_prefix("virtual:reze-types/")?;
    Some(format!("{}::{}", krate.replace('-', "_"), import.name))
}

fn loader_meta<'a>(
    cx: &Context<'_, 'a>,
    module: usize,
    call: &'a CallExpression<'a>,
    pattern: Option<&(&str, Vec<&str>)>,
    reports: &mut Vec<Report>,
) -> String {
    let Some(function) = call.arguments.last().and_then(Argument::as_expression) else {
        return "{}".into();
    };
    let Some(annotation) = first_param(function).and_then(|p| p.type_annotation.as_ref()) else {
        return "{}".into();
    };
    let mut meta = Vec::new();
    for name in ["params", "search"] {
        let Some(ty) = member(&annotation.type_annotation, name) else { continue };
        let span = ty.span();
        let resolved = match Resolver::new(cx).resolve(module, ty) {
            Ok(resolved) => resolved,
            Err(unlowered) => {
                report_unlowered(reports, unlowered, span);
                continue;
            }
        };
        if name == "params"
            && let (Some((route, names)), Some(fields)) = (pattern, field_names(&resolved))
        {
            for field in fields.iter().filter(|field| !names.contains(field)) {
                reports.push(
                    Report::new(Code::LoaderParamsType, span)
                        .arg("name", *field)
                        .arg("route", *route),
                );
            }
        }
        match fields_coercer(&resolved) {
            Some(coercer) => meta.push(format!("{name}: {coercer}")),
            None => report_unlowered(
                reports,
                unresolved(format!(
                    "{name} take strings, numbers, booleans, literals, dates, and lists of them"
                )),
                span,
            ),
        }
    }
    format!("{{ {} }}", meta.join(", "))
}

fn mutation_meta<'a>(
    cx: &Context<'_, 'a>,
    module: usize,
    call: &'a CallExpression<'a>,
    reports: &mut Vec<Report>,
) -> (String, Option<Ty>) {
    let Some(first) = call.arguments.first().and_then(Argument::as_expression) else {
        reports.push(Report::new(Code::MutationInputUntyped, call.span));
        return ("{}".into(), None);
    };
    if !matches!(
        first.without_parentheses(),
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
    ) {
        return ("{}".into(), None);
    }
    let param = first_param(first);
    let Some(annotation) = param.and_then(|p| p.type_annotation.as_ref()) else {
        let span = param.map_or(first.span(), |p| p.span);
        reports.push(Report::new(Code::MutationInputUntyped, span));
        return ("{}".into(), None);
    };
    match Resolver::new(cx).resolve(module, &annotation.type_annotation) {
        Ok(ty) => {
            let mut meta = vec![format!("json: {}", json_parser(&ty))];
            if let Some(coercer) = fields_coercer(&ty) {
                meta.push(format!("form: {coercer}"));
            }
            (format!("{{ {} }}", meta.join(", ")), Some(ty))
        }
        Err(unlowered) => {
            report_unlowered(reports, unlowered, annotation.type_annotation.span());
            ("{}".into(), None)
        }
    }
}

/// The mutation an expression names, following imports between project modules.
fn mutation_of<'e>(
    cx: &'e Context<'_, '_>,
    module: usize,
    expression: &Expression<'_>,
) -> Option<&'e Endpoint> {
    let Expression::Identifier(reference) = expression.without_parentheses() else { return None };
    let symbol = cx.symbol(module, reference)?;
    let (target, name) = match cx.facts[module].imports.get(&symbol) {
        Some(import) => match import.source {
            ImportSource::Module(target) => (target, import.name.as_str()),
            ImportSource::Package(_) => return None,
        },
        None => {
            return cx.facts[module]
                .endpoints
                .iter()
                .find(|e| e.kind == EndpointKind::Mutation && e.symbol == Some(symbol));
        }
    };
    cx.facts[target].endpoints.iter().find(|e| e.kind == EndpointKind::Mutation && e.export == name)
}

fn named_fields<'v>(nodes: &'v [Node<'_>], out: &mut Vec<(&'v str, Span)>) {
    visit_nodes(nodes, &mut |node| {
        if let Node::Element(element) = node
            && matches!(element.tag, "input" | "select" | "textarea" | "button")
        {
            for item in &element.attributes {
                if let AttributeItem::Attribute(Attribute {
                    name: "name",
                    value: Value::Static(value),
                }) = item
                {
                    out.push((value.as_str(), element.span));
                }
            }
        }
    });
}

/// Calls `f` on every node of `nodes` and below, through elements and control flow.
fn visit_nodes<'v, 'a>(nodes: &'v [Node<'a>], f: &mut dyn FnMut(&'v Node<'a>)) {
    for node in nodes {
        f(node);
        let branch = |branch: &'v Branch<'a>, f: &mut dyn FnMut(&'v Node<'a>)| match branch {
            Branch::Content(content) => visit_nodes(&content.nodes, f),
            Branch::Callback(callback) => visit_nodes(&callback.content.nodes, f),
        };
        match node {
            Node::Element(element) => visit_nodes(&element.children, f),
            Node::Show(show) => {
                branch(&show.children, f);
                show.fallback.iter().for_each(|b| branch(b, f));
            }
            Node::Switch(switch) => {
                switch.matches.iter().for_each(|m| branch(&m.children, f));
                switch.fallback.iter().for_each(|b| branch(b, f));
            }
            Node::For(each) | Node::Repeat(each) => {
                visit_nodes(&each.children.content.nodes, f);
                each.fallback.iter().for_each(|b| branch(b, f));
            }
            Node::Loading(boundary) | Node::Errored(boundary) | Node::Portal(boundary) => {
                visit_nodes(&boundary.children.nodes, f);
                boundary.fallback.iter().for_each(|b| branch(b, f));
            }
            Node::Component(call) => {
                for value in call.props.iter().map(|p| &p.value).chain(call.children.iter()) {
                    if let PropValue::Content(content) = value {
                        visit_nodes(&content.nodes, f);
                    }
                }
            }
            Node::Text(_) | Node::Expr(_) | Node::Head(_) => {}
        }
    }
}

/// Reports form fields that the input type of the form's mutation does not declare.
pub fn check_forms(cx: &Context<'_, '_>, module: usize, reports: &mut Vec<Report>) {
    let facts = &cx.facts[module];
    let contents =
        facts.components.iter().filter_map(|c| c.template.as_ref()).chain(facts.values.values());
    for content in contents {
        visit_nodes(&content.nodes, &mut |node| {
            let Node::Element(element) = node else { return };
            if element.tag != "form" {
                return;
            }
            let action = element.attributes.iter().find_map(|item| match item {
                AttributeItem::Attribute(Attribute { name: "action", value: Value::Expr(e) }) => {
                    Some(*e)
                }
                _ => None,
            });
            let Some(endpoint) = action.and_then(|action| mutation_of(cx, module, action)) else {
                return;
            };
            let Some(declared) = endpoint.input.as_ref().and_then(field_names) else { return };
            let mut fields = Vec::new();
            named_fields(&element.children, &mut fields);
            for (field, span) in fields {
                if !declared.contains(&field) {
                    reports.push(
                        Report::new(Code::MutationFormField, span)
                            .arg("field", field)
                            .arg("mutation", endpoint.export.clone()),
                    );
                }
            }
        });
    }
}
