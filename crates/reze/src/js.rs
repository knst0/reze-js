//! JavaScript semantics the Rust render must reproduce exactly: number formatting, string
//! length and ordering, and `JSON.stringify` strings.

use std::cmp::Ordering;
use std::fmt::Write;

/// `Number.prototype.toString()` of `value`.
pub fn number(value: f64) -> String {
    let mut out = String::new();
    push_number(&mut out, value);
    out
}

/// Appends `Number.prototype.toString()` of `value`.
pub fn push_number(out: &mut String, value: f64) {
    if value.is_nan() {
        return out.push_str("NaN");
    }
    if value == 0.0 {
        return out.push('0');
    }
    if value.is_infinite() {
        return out.push_str(if value < 0.0 { "-Infinity" } else { "Infinity" });
    }
    if value < 0.0 {
        out.push('-');
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().unwrap() + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = n - 1;
        let _ = write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.abs());
    }
}

/// `value.length`: the count of UTF-16 code units.
pub fn len(value: &str) -> usize {
    value.chars().map(char::len_utf16).sum()
}

/// How JavaScript orders two strings: by UTF-16 code units.
pub fn compare(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

/// Whether a number is truthy.
pub fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

/// Appends `value` as `JSON.stringify` writes a string.
pub fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
