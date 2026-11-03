//! Mutation input: a JSON body or form fields, checked against the input type with the issues the
//! JS validators report for the same type.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, Hash};
use std::time::SystemTime;

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(name, _)| name == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// One step of an issue's path: a field name or a list index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Field(String),
    Index(usize),
}

/// A problem with one value of the input, in the Standard Schema format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub message: String,
    pub path: Vec<Key>,
}

impl Issue {
    /// The path joined with `.`, the key of `useSubmission().issues`.
    pub fn key(&self) -> String {
        let parts: Vec<String> = self
            .path
            .iter()
            .map(|key| match key {
                Key::Field(name) => name.clone(),
                Key::Index(index) => index.to_string(),
            })
            .collect();
        parts.join(".")
    }
}

impl crate::Data for Issue {
    fn write(&self, out: &mut String) {
        out.push_str("{\"message\":");
        self.message.write(out);
        out.push_str(",\"path\":[");
        for (index, key) in self.path.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            match key {
                Key::Field(name) => name.write(out),
                Key::Index(index) => out.push_str(&index.to_string()),
            }
        }
        out.push_str("]}");
    }
}

/// The fields of a form post: every value of a name, in order.
pub type Form = [(String, String)];

/// `path` with `key` appended.
pub fn child(path: &[Key], key: Key) -> Vec<Key> {
    let mut path = path.to_vec();
    path.push(key);
    path
}

fn expected<T: Input>(path: &[Key], issues: &mut Vec<Issue>) -> Option<T> {
    issues.push(Issue { message: format!("Expected {}", T::describe()), path: path.to_vec() });
    None
}

/// A mutation's input type, or a type of one of its fields: `#[derive(reze::Data)]` implements it.
pub trait Input: Sized {
    /// Whether one form value can carry the type.
    const SCALAR: bool = false;
    /// Whether the type can be one field of a form: a scalar, or an optional value or a list of
    /// one.
    const FIELD: bool = Self::SCALAR;
    /// Whether form fields can carry the type: a struct whose fields can.
    const FORM: bool = false;

    /// What the type is, as the issues name it: `a string`, `an object`.
    fn describe() -> String;

    /// Reads a JSON value, `None` for a missing key.
    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self>;

    /// Coerces one form value.
    fn scalar(_value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        expected(path, issues)
    }

    /// Reads every value of the form field `name`.
    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let path = [Key::Field(name.into())];
        match values.first() {
            None | Some(&"") => {
                issues.push(Issue { message: "Required".into(), path: path.to_vec() });
                None
            }
            Some(value) => Self::scalar(value, &path, issues),
        }
    }

    /// Reads a whole form, for a struct whose fields are [`Input::FIELD`].
    fn form(_form: &Form, _issues: &mut Vec<Issue>) -> Option<Self> {
        None
    }
}

/// Every value of the form field `name`.
pub fn values<'f>(form: &'f Form, name: &str) -> Vec<&'f str> {
    form.iter().filter(|(key, _)| key == name).map(|(_, value)| value.as_str()).collect()
}

/// The JSON field `name` of an object, which [`Input::json`] of a struct reads.
pub fn field<'j>(value: &'j Json, name: &str) -> Option<&'j Json> {
    value.get(name)
}

impl Input for String {
    const SCALAR: bool = true;

    fn describe() -> String {
        "a string".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        match value {
            Some(Json::String(text)) => Some(text.clone()),
            _ => expected::<Self>(path, issues),
        }
    }

    fn scalar(value: &str, _: &[Key], _: &mut Vec<Issue>) -> Option<Self> {
        Some(value.into())
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        match values.first() {
            Some(value) => Some((*value).into()),
            None => {
                issues.push(Issue {
                    message: "Required".into(),
                    path: vec![Key::Field(name.into())],
                });
                None
            }
        }
    }
}

impl Input for bool {
    const SCALAR: bool = true;

    fn describe() -> String {
        "a boolean".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        match value {
            Some(Json::Bool(value)) => Some(*value),
            _ => expected::<Self>(path, issues),
        }
    }

    fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        match value {
            "on" | "true" => Some(true),
            "false" => Some(false),
            _ => expected::<Self>(path, issues),
        }
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        match values.first() {
            None | Some(&"") => Some(false),
            Some(value) => Self::scalar(value, &[Key::Field(name.into())], issues),
        }
    }
}

/// A finite number from a form value, as `Number(v)` reads it.
fn number(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A decimal integer string, as `/^-?\d+$/` accepts it.
fn digits(value: &str) -> bool {
    let rest = value.strip_prefix('-').unwrap_or(value);
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

macro_rules! floats {
    ($($ty:ty),*) => {$(
        impl Input for $ty {
            const SCALAR: bool = true;

            fn describe() -> String {
                "a number".into()
            }

            fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                match value {
                    Some(Json::Number(n)) if n.is_finite() => Some(*n as $ty),
                    _ => expected::<Self>(path, issues),
                }
            }

            fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                number(value).map(|n| n as $ty).or_else(|| expected::<Self>(path, issues))
            }
        }
    )*};
}

floats!(f64, f32);

macro_rules! ints {
    ($($ty:ty),*) => {$(
        impl Input for $ty {
            const SCALAR: bool = true;

            fn describe() -> String {
                format!("an integer from {} to {}", <$ty>::MIN, <$ty>::MAX)
            }

            fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                match value {
                    Some(Json::Number(n))
                        if n.fract() == 0.0 && *n >= <$ty>::MIN as f64 && *n <= <$ty>::MAX as f64 =>
                    {
                        Some(*n as $ty)
                    }
                    _ => expected::<Self>(path, issues),
                }
            }

            fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                match value.parse() {
                    Ok(n) if digits(value) => Some(n),
                    _ => expected::<Self>(path, issues),
                }
            }
        }
    )*};
}

ints!(u8, u16, u32, i8, i16, i32);

macro_rules! bigints {
    ($($ty:ty),*) => {$(
        impl Input for $ty {
            const SCALAR: bool = true;

            fn describe() -> String {
                format!("an integer from {} to {}", <$ty>::MIN, <$ty>::MAX)
            }

            fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                match value {
                    Some(Json::String(text)) => Self::scalar(text, path, issues),
                    _ => expected::<Self>(path, issues),
                }
            }

            fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                match value.parse() {
                    Ok(n) if digits(value) => Some(n),
                    _ => expected::<Self>(path, issues),
                }
            }
        }
    )*};
}

bigints!(u64, i64);

impl<T: Input> Input for Option<T> {
    const FIELD: bool = T::SCALAR;

    fn describe() -> String {
        format!("{} or null", T::describe())
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        if value == Some(&Json::Null) {
            return Some(None);
        }
        let mut own = Vec::new();
        match T::json(value, path, &mut own) {
            Some(value) if own.is_empty() => Some(Some(value)),
            _ => expected::<Self>(path, issues),
        }
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        match values.first() {
            None | Some(&"") => Some(None),
            Some(value) => T::scalar(value, &[Key::Field(name.into())], issues).map(Some),
        }
    }
}

fn list<T: Input>(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Vec<T>> {
    let Some(Json::Array(items)) = value else {
        issues.push(Issue { message: "Expected an array".into(), path: path.to_vec() });
        return None;
    };
    let read: Vec<Option<T>> = items
        .iter()
        .enumerate()
        .map(|(index, item)| T::json(Some(item), &child(path, Key::Index(index)), issues))
        .collect();
    read.into_iter().collect()
}

fn list_field<T: Input>(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Vec<T>> {
    let read: Vec<Option<T>> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            T::scalar(value, &[Key::Field(name.into()), Key::Index(index)], issues)
        })
        .collect();
    read.into_iter().collect()
}

impl<T: Input> Input for Vec<T> {
    const FIELD: bool = T::SCALAR;

    fn describe() -> String {
        "an array".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        list(value, path, issues)
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        list_field(values, name, issues)
    }
}

impl<T: Input + Ord> Input for BTreeSet<T> {
    const FIELD: bool = T::SCALAR;

    fn describe() -> String {
        "an array".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        list(value, path, issues).map(|items: Vec<T>| items.into_iter().collect())
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        list_field(values, name, issues).map(|items: Vec<T>| items.into_iter().collect())
    }
}

impl<T: Input + Eq + Hash, S: BuildHasher + Default> Input for HashSet<T, S> {
    const FIELD: bool = T::SCALAR;

    fn describe() -> String {
        "an array".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        list(value, path, issues).map(|items: Vec<T>| items.into_iter().collect())
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        list_field(values, name, issues).map(|items: Vec<T>| items.into_iter().collect())
    }
}

fn pairs<V: Input, M: FromIterator<(String, V)>>(
    value: Option<&Json>,
    path: &[Key],
    issues: &mut Vec<Issue>,
) -> Option<M> {
    let entries = match value {
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| match item {
                Json::Array(pair) => match pair.as_slice() {
                    [Json::String(key), value] => Some((key, value)),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    let Some(entries) = entries else {
        issues.push(Issue {
            message: "Expected an array of [key, value] pairs".into(),
            path: path.to_vec(),
        });
        return None;
    };
    let read: Vec<Option<(String, V)>> = entries
        .into_iter()
        .map(|(key, value)| {
            V::json(Some(value), &child(path, Key::Field(key.clone())), issues)
                .map(|value| (key.clone(), value))
        })
        .collect();
    read.into_iter().collect()
}

impl<V: Input> Input for BTreeMap<String, V> {
    fn describe() -> String {
        "an array of [key, value] pairs".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        pairs(value, path, issues)
    }
}

impl<V: Input, S: BuildHasher + Default> Input for HashMap<String, V, S> {
    fn describe() -> String {
        "an array of [key, value] pairs".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        pairs(value, path, issues)
    }
}

/// A `Date`: an ISO 8601 string, or milliseconds since the epoch in JSON.
impl Input for SystemTime {
    const SCALAR: bool = true;

    fn describe() -> String {
        "a date".into()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        let time = match value {
            Some(Json::String(text)) => crate::date::parse(text),
            Some(Json::Number(ms)) => crate::date::from_millis(*ms),
            _ => None,
        };
        time.or_else(|| expected(path, issues))
    }

    fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        crate::date::parse(value).or_else(|| expected(path, issues))
    }
}

macro_rules! tuples {
    ($(($($ty:ident $index:tt),+; $len:literal)),*) => {$(
        /// A JSON array of exactly the tuple's length.
        impl<$($ty: Input),+> Input for ($($ty,)+) {
            fn describe() -> String {
                "an array".into()
            }

            fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
                let Some(Json::Array(items)) = value.filter(
                    |value| matches!(value, Json::Array(items) if items.len() == $len),
                ) else {
                    return expected(path, issues);
                };
                let read = ($($ty::json(items.get($index), &child(path, Key::Index($index)), issues),)+);
                Some(($(read.$index?,)+))
            }
        }
    )*};
}

tuples!((A 0; 1), (A 0, B 1; 2), (A 0, B 1, C 2; 3), (A 0, B 1, C 2, D 3; 4));

impl<T: Input> Input for Box<T> {
    const SCALAR: bool = T::SCALAR;
    const FIELD: bool = T::FIELD;
    const FORM: bool = T::FORM;

    fn describe() -> String {
        T::describe()
    }

    fn json(value: Option<&Json>, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        T::json(value, path, issues).map(Box::new)
    }

    fn scalar(value: &str, path: &[Key], issues: &mut Vec<Issue>) -> Option<Self> {
        T::scalar(value, path, issues).map(Box::new)
    }

    fn field(values: &[&str], name: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        T::field(values, name, issues).map(Box::new)
    }

    fn form(form: &Form, issues: &mut Vec<Issue>) -> Option<Self> {
        T::form(form, issues).map(Box::new)
    }
}
