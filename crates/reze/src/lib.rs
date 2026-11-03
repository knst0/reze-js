//! Loader data that crosses from a Rust loader to the page: `#[derive(reze::Data)]`, and the JSON
//! it encodes to, byte for byte what `JSON.stringify` writes for the same value.

pub mod js;

use std::collections::BTreeSet;

pub use reze_derive::Data;

/// A value a Rust loader returns: a struct with named fields, a unit enum, or one of the scalars
/// and containers this crate implements it for.
pub trait Data {
    /// Appends the value as JSON, as `JSON.stringify` writes it.
    fn write(&self, out: &mut String);
}

/// A unit enum, which crosses as the name of its variant.
pub trait Variant {
    fn name(&self) -> &'static str;
}

/// The JSON of `value`.
pub fn to_json<T: Data + ?Sized>(value: &T) -> String {
    let mut out = String::new();
    value.write(&mut out);
    out
}

impl Data for str {
    fn write(&self, out: &mut String) {
        js::push_json_string(out, self);
    }
}

impl Data for String {
    fn write(&self, out: &mut String) {
        js::push_json_string(out, self);
    }
}

impl Data for bool {
    fn write(&self, out: &mut String) {
        out.push_str(if *self { "true" } else { "false" });
    }
}

impl Data for f64 {
    fn write(&self, out: &mut String) {
        if self.is_finite() { js::push_number(out, *self) } else { out.push_str("null") }
    }
}

impl Data for f32 {
    fn write(&self, out: &mut String) {
        f64::from(*self).write(out);
    }
}

macro_rules! small_ints {
    ($($ty:ty),*) => {$(
        impl Data for $ty {
            fn write(&self, out: &mut String) {
                out.push_str(&self.to_string());
            }
        }
    )*};
}

small_ints!(u8, u16, u32, i8, i16, i32);

macro_rules! big_ints {
    ($($ty:ty),*) => {$(
        impl Data for $ty {
            fn write(&self, out: &mut String) {
                out.push('"');
                out.push_str(&self.to_string());
                out.push('"');
            }
        }
    )*};
}

big_ints!(u64, i64);

impl<T: Data> Data for Option<T> {
    fn write(&self, out: &mut String) {
        match self {
            Some(value) => value.write(out),
            None => out.push_str("null"),
        }
    }
}

impl<T: Variant + ?Sized> Variant for &T {
    fn name(&self) -> &'static str {
        (**self).name()
    }
}

impl<T: Data + ?Sized> Data for &T {
    fn write(&self, out: &mut String) {
        (**self).write(out);
    }
}

impl<T: Data + ?Sized> Data for Box<T> {
    fn write(&self, out: &mut String) {
        (**self).write(out);
    }
}

impl<T: Data> Data for [T] {
    fn write(&self, out: &mut String) {
        out.push('[');
        for (index, item) in self.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            item.write(out);
        }
        out.push(']');
    }
}

impl<T: Data> Data for Vec<T> {
    fn write(&self, out: &mut String) {
        self.as_slice().write(out);
    }
}

impl<T: Data> Data for BTreeSet<T> {
    fn write(&self, out: &mut String) {
        out.push('[');
        for (index, item) in self.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            item.write(out);
        }
        out.push(']');
    }
}
