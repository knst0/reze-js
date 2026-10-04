include!(concat!(env!("OUT_DIR"), "/html-data.rs"));

/// The DOM event of `onFooBar`'s `FooBar`; `spread` in `@rezejs/dom` maps names the same way.
pub fn event_name(camel: &str) -> String {
    let lower = camel.to_ascii_lowercase();
    if lower == "doubleclick" { String::from("dblclick") } else { lower }
}

pub fn is_mathml_root(tag: &str) -> bool {
    tag == "math"
}

fn push_escaped(out: &mut String, s: &str, quote: bool) {
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        let Some(escaped) = escape_html_byte(b, quote) else { continue };
        out.push_str(&s[start..i]);
        out.push_str(escaped);
        start = i + 1;
    }
    out.push_str(&s[start..]);
}

pub fn escape_text(out: &mut String, s: &str) {
    push_escaped(out, s, false);
}

/// Appends `=value` after an attribute name, unquoted when the HTML parser reads it back
/// unchanged; an empty value appends nothing, since a bare attribute is empty.
pub fn push_attribute_value(out: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    let needs_quotes = value.ends_with('/')
        || value.bytes().any(|b| {
            matches!(
                b,
                b' ' | b'\t' | b'\n' | b'\x0C' | b'\r' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'
            )
        });
    out.push('=');
    if needs_quotes {
        out.push('"');
        push_escaped(out, value, true);
        out.push('"');
    } else {
        push_escaped(out, value, true);
    }
}

/// Drops the closing tags that end `html`: the parser closes open elements at end of input.
pub fn trim_trailing_end_tags(html: &mut String) {
    while html.ends_with('>') {
        let Some(start) = html.rfind("</") else { return };
        let name = &html[start + 2..html.len() - 1];
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b':')
        {
            return;
        }
        html.truncate(start);
    }
}

pub fn is_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Common HTML/SVG attributes plus the framework's own, for near-miss suggestions.
const KNOWN_ATTRIBUTES: &[&str] = &[
    "abbr",
    "accept",
    "action",
    "align",
    "alt",
    "as",
    "async",
    "autoplay",
    "charset",
    "checked",
    "cite",
    "class",
    "cols",
    "colspan",
    "content",
    "controls",
    "coords",
    "crossorigin",
    "cx",
    "cy",
    "d",
    "datetime",
    "decoding",
    "default",
    "defer",
    "disabled",
    "download",
    "draggable",
    "enctype",
    "fill",
    "for",
    "form",
    "formaction",
    "headers",
    "height",
    "hidden",
    "href",
    "hreflang",
    "id",
    "ismap",
    "kind",
    "label",
    "lang",
    "loading",
    "loop",
    "max",
    "maxlength",
    "media",
    "method",
    "min",
    "minlength",
    "multiple",
    "muted",
    "name",
    "nonce",
    "open",
    "pattern",
    "ping",
    "placeholder",
    "playsinline",
    "poster",
    "preload",
    "r",
    "readonly",
    "referrerpolicy",
    "rel",
    "required",
    "rev",
    "rows",
    "rowspan",
    "rx",
    "ry",
    "sandbox",
    "scope",
    "selected",
    "shape",
    "size",
    "sizes",
    "slot",
    "span",
    "spellcheck",
    "src",
    "srcdoc",
    "srclang",
    "srcset",
    "start",
    "step",
    "style",
    "tabindex",
    "target",
    "title",
    "translate",
    "type",
    "usemap",
    "value",
    "viewBox",
    "width",
    "wrap",
    "x",
    "x1",
    "x2",
    "y",
    "y1",
    "y2",
];

/// The closest known attribute within edit distance 2; `None` for known names and far misses.
pub fn suggest_attribute(name: &str) -> Option<&'static str> {
    if name.is_empty() || KNOWN_ATTRIBUTES.contains(&name) {
        return None;
    }
    if matches!(name, "className" | "classList") {
        return Some("class");
    }
    let mut best: Option<(&'static str, usize)> = None;
    for &candidate in KNOWN_ATTRIBUTES {
        let distance = edit_distance(name, candidate, 2);
        if distance <= 2 && best.is_none_or(|(_, d)| distance < d) {
            best = Some((candidate, distance));
        }
    }
    best.map(|(candidate, _)| candidate)
}

/// Levenshtein distance, saturating at `limit + 1`.
fn edit_distance(a: &str, b: &str, limit: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut row_min = current[0];
        for (j, &cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
            row_min = row_min.min(current[j + 1]);
        }
        if row_min > limit {
            return limit + 1;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// JSX text as it renders (the Babel/TypeScript rule): every line is trimmed except the outer
/// edges of the first and last, empty lines are dropped, and the rest join with one space.
pub fn clean_jsx_text(value: &str) -> String {
    let lines: Vec<&str> =
        value.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    let last = lines.len() - 1;
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut line = *line;
        if i != 0 {
            line = line.trim_start_matches([' ', '\t']);
        }
        if i != last {
            line = line.trim_end_matches([' ', '\t']);
        }
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(line.chars().map(|c| if c == '\t' { ' ' } else { c }));
    }
    out
}

/// Decodes `&#123;`, `&#x7B;` and the XHTML named entities; anything else, a bare `&`
/// included, stays as written.
pub fn decode_entities(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest[1..]
            .char_indices()
            .take(10)
            .find(|&(_, c)| c == ';')
            .and_then(|(semicolon, _)| Some((entity(&rest[1..semicolon + 1])?, semicolon + 2)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn entity(name: &str) -> Option<char> {
    let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(decimal) = name.strip_prefix('#') {
        decimal.parse().ok()?
    } else {
        named_entity(name)?
    };
    char::from_u32(code)
}

fn named_entity(name: &str) -> Option<u32> {
    Some(match name {
        "quot" => 34,
        "amp" => 38,
        "apos" => 39,
        "lt" => 60,
        "gt" => 62,
        "nbsp" => 160,
        "iexcl" => 161,
        "cent" => 162,
        "pound" => 163,
        "curren" => 164,
        "yen" => 165,
        "brvbar" => 166,
        "sect" => 167,
        "uml" => 168,
        "copy" => 169,
        "ordf" => 170,
        "laquo" => 171,
        "not" => 172,
        "shy" => 173,
        "reg" => 174,
        "macr" => 175,
        "deg" => 176,
        "plusmn" => 177,
        "sup2" => 178,
        "sup3" => 179,
        "acute" => 180,
        "micro" => 181,
        "para" => 182,
        "middot" => 183,
        "cedil" => 184,
        "sup1" => 185,
        "ordm" => 186,
        "raquo" => 187,
        "frac14" => 188,
        "frac12" => 189,
        "frac34" => 190,
        "iquest" => 191,
        "Agrave" => 192,
        "Aacute" => 193,
        "Acirc" => 194,
        "Atilde" => 195,
        "Auml" => 196,
        "Aring" => 197,
        "AElig" => 198,
        "Ccedil" => 199,
        "Egrave" => 200,
        "Eacute" => 201,
        "Ecirc" => 202,
        "Euml" => 203,
        "Igrave" => 204,
        "Iacute" => 205,
        "Icirc" => 206,
        "Iuml" => 207,
        "ETH" => 208,
        "Ntilde" => 209,
        "Ograve" => 210,
        "Oacute" => 211,
        "Ocirc" => 212,
        "Otilde" => 213,
        "Ouml" => 214,
        "times" => 215,
        "Oslash" => 216,
        "Ugrave" => 217,
        "Uacute" => 218,
        "Ucirc" => 219,
        "Uuml" => 220,
        "Yacute" => 221,
        "THORN" => 222,
        "szlig" => 223,
        "agrave" => 224,
        "aacute" => 225,
        "acirc" => 226,
        "atilde" => 227,
        "auml" => 228,
        "aring" => 229,
        "aelig" => 230,
        "ccedil" => 231,
        "egrave" => 232,
        "eacute" => 233,
        "ecirc" => 234,
        "euml" => 235,
        "igrave" => 236,
        "iacute" => 237,
        "icirc" => 238,
        "iuml" => 239,
        "eth" => 240,
        "ntilde" => 241,
        "ograve" => 242,
        "oacute" => 243,
        "ocirc" => 244,
        "otilde" => 245,
        "ouml" => 246,
        "divide" => 247,
        "oslash" => 248,
        "ugrave" => 249,
        "uacute" => 250,
        "ucirc" => 251,
        "uuml" => 252,
        "yacute" => 253,
        "thorn" => 254,
        "yuml" => 255,
        "OElig" => 338,
        "oelig" => 339,
        "Scaron" => 352,
        "scaron" => 353,
        "Yuml" => 376,
        "fnof" => 402,
        "circ" => 710,
        "tilde" => 732,
        "Alpha" => 913,
        "Beta" => 914,
        "Gamma" => 915,
        "Delta" => 916,
        "Epsilon" => 917,
        "Zeta" => 918,
        "Eta" => 919,
        "Theta" => 920,
        "Iota" => 921,
        "Kappa" => 922,
        "Lambda" => 923,
        "Mu" => 924,
        "Nu" => 925,
        "Xi" => 926,
        "Omicron" => 927,
        "Pi" => 928,
        "Rho" => 929,
        "Sigma" => 931,
        "Tau" => 932,
        "Upsilon" => 933,
        "Phi" => 934,
        "Chi" => 935,
        "Psi" => 936,
        "Omega" => 937,
        "alpha" => 945,
        "beta" => 946,
        "gamma" => 947,
        "delta" => 948,
        "epsilon" => 949,
        "zeta" => 950,
        "eta" => 951,
        "theta" => 952,
        "iota" => 953,
        "kappa" => 954,
        "lambda" => 955,
        "mu" => 956,
        "nu" => 957,
        "xi" => 958,
        "omicron" => 959,
        "pi" => 960,
        "rho" => 961,
        "sigmaf" => 962,
        "sigma" => 963,
        "tau" => 964,
        "upsilon" => 965,
        "phi" => 966,
        "chi" => 967,
        "psi" => 968,
        "omega" => 969,
        "thetasym" => 977,
        "upsih" => 978,
        "piv" => 982,
        "ensp" => 8194,
        "emsp" => 8195,
        "thinsp" => 8201,
        "zwnj" => 8204,
        "zwj" => 8205,
        "lrm" => 8206,
        "rlm" => 8207,
        "ndash" => 8211,
        "mdash" => 8212,
        "lsquo" => 8216,
        "rsquo" => 8217,
        "sbquo" => 8218,
        "ldquo" => 8220,
        "rdquo" => 8221,
        "bdquo" => 8222,
        "dagger" => 8224,
        "Dagger" => 8225,
        "bull" => 8226,
        "hellip" => 8230,
        "permil" => 8240,
        "prime" => 8242,
        "Prime" => 8243,
        "lsaquo" => 8249,
        "rsaquo" => 8250,
        "oline" => 8254,
        "frasl" => 8260,
        "euro" => 8364,
        "image" => 8465,
        "weierp" => 8472,
        "real" => 8476,
        "trade" => 8482,
        "alefsym" => 8501,
        "larr" => 8592,
        "uarr" => 8593,
        "rarr" => 8594,
        "darr" => 8595,
        "harr" => 8596,
        "crarr" => 8629,
        "lArr" => 8656,
        "uArr" => 8657,
        "rArr" => 8658,
        "dArr" => 8659,
        "hArr" => 8660,
        "forall" => 8704,
        "part" => 8706,
        "exist" => 8707,
        "empty" => 8709,
        "nabla" => 8711,
        "isin" => 8712,
        "notin" => 8713,
        "ni" => 8715,
        "prod" => 8719,
        "sum" => 8721,
        "minus" => 8722,
        "lowast" => 8727,
        "radic" => 8730,
        "prop" => 8733,
        "infin" => 8734,
        "ang" => 8736,
        "and" => 8743,
        "or" => 8744,
        "cap" => 8745,
        "cup" => 8746,
        "int" => 8747,
        "there4" => 8756,
        "sim" => 8764,
        "cong" => 8773,
        "asymp" => 8776,
        "ne" => 8800,
        "equiv" => 8801,
        "le" => 8804,
        "ge" => 8805,
        "sub" => 8834,
        "sup" => 8835,
        "nsub" => 8836,
        "sube" => 8838,
        "supe" => 8839,
        "oplus" => 8853,
        "otimes" => 8855,
        "perp" => 8869,
        "sdot" => 8901,
        "lceil" => 8968,
        "rceil" => 8969,
        "lfloor" => 8970,
        "rfloor" => 8971,
        "lang" => 9001,
        "rang" => 9002,
        "loz" => 9674,
        "spades" => 9824,
        "clubs" => 9827,
        "hearts" => 9829,
        "diams" => 9830,
        _ => return None,
    })
}
