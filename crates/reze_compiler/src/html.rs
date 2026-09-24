//! HTML and JS string tables and escaping.

/// Events delegated to the document (SPECIFICATION §3.5); must match `@rezejs/dom`.
pub fn is_delegated_event(name: &str) -> bool {
    matches!(
        name,
        "click"
            | "input"
            | "change"
            | "submit"
            | "keydown"
            | "keyup"
            | "pointerdown"
            | "pointerup"
            | "pointermove"
            | "focusin"
            | "focusout"
    )
}

/// DOM properties set as properties rather than attributes (SPECIFICATION §3.4).
pub fn is_property(name: &str) -> bool {
    matches!(name, "value" | "checked" | "selected" | "textContent" | "innerHTML")
}

/// Elements without content or closing tag.
pub fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "search"
            | "source"
            | "wbr"
    )
}

/// SVG elements that need an `<svg>` wrapper to parse as SVG when they root a template.
/// Names shared with HTML (`a`, `script`, `style`, `title`) parse as HTML.
pub fn is_svg_element(tag: &str) -> bool {
    matches!(
        tag,
        "animate"
            | "animateMotion"
            | "animateTransform"
            | "circle"
            | "clipPath"
            | "cursor"
            | "defs"
            | "desc"
            | "discard"
            | "ellipse"
            | "feBlend"
            | "feColorMatrix"
            | "feComponentTransfer"
            | "feComposite"
            | "feConvolveMatrix"
            | "feDiffuseLighting"
            | "feDisplacementMap"
            | "feDistantLight"
            | "feDropShadow"
            | "feFlood"
            | "feFuncA"
            | "feFuncB"
            | "feFuncG"
            | "feFuncR"
            | "feGaussianBlur"
            | "feImage"
            | "feMerge"
            | "feMergeNode"
            | "feMorphology"
            | "feOffset"
            | "fePointLight"
            | "feSpecularLighting"
            | "feSpotLight"
            | "feTile"
            | "feTurbulence"
            | "filter"
            | "foreignObject"
            | "g"
            | "hatch"
            | "hatchpath"
            | "image"
            | "line"
            | "linearGradient"
            | "marker"
            | "mask"
            | "metadata"
            | "mpath"
            | "path"
            | "pattern"
            | "polygon"
            | "polyline"
            | "radialGradient"
            | "rect"
            | "set"
            | "solidcolor"
            | "stop"
            | "switch"
            | "symbol"
            | "text"
            | "textPath"
            | "tspan"
            | "use"
            | "view"
    )
}

/// Namespace URI for a namespaced attribute prefix.
pub fn attribute_namespace(prefix: &str) -> Option<&'static str> {
    match prefix {
        "xlink" => Some("http://www.w3.org/1999/xlink"),
        "xml" => Some("http://www.w3.org/XML/1998/namespace"),
        _ => None,
    }
}

pub fn escape_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
}

pub fn escape_attribute(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// A double-quoted JS string literal.
pub fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An object literal key: bare when it is an identifier name, quoted otherwise.
pub fn property_key(name: &str) -> String {
    let mut chars = name.chars();
    let ident = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if ident { name.to_string() } else { js_string(name) }
}

/// Member access on `object`: `.name` for identifier names, `["name"]` otherwise.
pub fn member(object: &str, name: &str) -> String {
    let key = property_key(name);
    if key.starts_with('"') { format!("{object}[{key}]") } else { format!("{object}.{key}") }
}

/// JSX text as it renders: lines trimmed, whitespace-only lines with a line break dropped,
/// the rest joined by single spaces (the Babel/TypeScript rule).
pub fn clean_jsx_text(value: &str) -> String {
    let lines: Vec<&str> = value.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let last_non_empty =
        lines.iter().rposition(|l| l.contains(|c| c != ' ' && c != '\t')).unwrap_or(0);
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut line = *line;
        if i != 0 {
            line = line.trim_start_matches([' ', '\t']);
        }
        if i != lines.len() - 1 {
            line = line.trim_end_matches([' ', '\t']);
        }
        if line.is_empty() {
            continue;
        }
        out.push_str(&line.replace('\t', " "));
        if i != last_non_empty {
            out.push(' ');
        }
    }
    out
}

/// Decodes the character references JSX text and attribute strings may hold: `&#123;`,
/// `&#x7B;` and the XHTML named entities. Anything else, a bare `&` included, stays as written.
pub fn decode_entities(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains('&') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest[1..]
            .char_indices()
            .take(10)
            .find(|&(_, c)| c == ';')
            .and_then(|(end, _)| Some((entity(&rest[1..end + 1])?, end + 2)));
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
    let code = if let Some(hex) = name.strip_prefix("#x") {
        u32::from_str_radix(hex, 16).ok()?
    } else if let Some(dec) = name.strip_prefix('#') {
        dec.parse().ok()?
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
