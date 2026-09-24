//! Server templates (SPEC §14): the template HTML split around its dynamic parts, joined by
//! `ssr`. Events, refs and client-only properties render nothing.

use std::fmt::Write;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::html::push_js_string;
use crate::ir::{
    Anchor, BindTarget, Child, Embed, ExprChild, Getter, NodeId, Op, PropHtml, Props, Template,
    Value,
};

/// Brackets what a non-sole insert rendered, so hydration can find it again.
const INSERT_OPEN: &str = "<!--[-->";
const INSERT_CLOSE: &str = "<!--]-->";

enum Part<'t, 'a> {
    HydrationKey,
    Set {
        target: BindTarget<'a>,
        value: &'t Value<'a>,
    },
    /// `spread_temp`: index of the variable holding `props` when the children are rendered too.
    Spread {
        props: &'t Props<'a>,
        is_svg: bool,
        spread_temp: Option<usize>,
    },
    SpreadChildren {
        spread_temp: usize,
    },
    Insert {
        value: &'t Child<'a>,
        is_bracketed: bool,
    },
    ClassToggles {
        toggles: &'t Value<'a>,
        is_inside_static_class: bool,
    },
}

struct Placed<'t, 'a> {
    at: u32,
    part: Part<'t, 'a>,
}

enum Placement {
    Attributes,
    Content,
}

fn placement(target: BindTarget<'_>) -> Option<Placement> {
    match target {
        BindTarget::Attr(_)
        | BindTarget::AttrNs(..)
        | BindTarget::Bool(_)
        | BindTarget::Class
        | BindTarget::Style
        | BindTarget::Prop { html: PropHtml::Attr | PropHtml::Bool, .. } => {
            Some(Placement::Attributes)
        }
        BindTarget::ClassToggle(_) => None,
        BindTarget::Prop { html: PropHtml::Text | PropHtml::Html, .. } => Some(Placement::Content),
        BindTarget::Prop { html: PropHtml::None, .. } => None,
    }
}

fn place_set<'t, 'a>(
    parts: &mut std::vec::Vec<Placed<'t, 'a>>,
    template: &Template<'a>,
    id: NodeId,
    target: BindTarget<'a>,
    value: &'t Value<'a>,
) {
    let node = template.nodes[id.index()];
    let at = match placement(target) {
        Some(Placement::Attributes) => node.attributes_end(),
        Some(Placement::Content) => node.content_end(),
        None => return,
    };
    parts.push(Placed { at, part: Part::Set { target, value } });
}

impl<'a> Emitter<'a, '_> {
    pub(super) fn server_template(&mut self, out: &mut Code, template: &Template<'a>) {
        let node = |id: NodeId| template.nodes[id.index()];
        let mut parts: std::vec::Vec<Placed<'_, 'a>> = std::vec::Vec::new();
        parts.push(Placed { at: node(NodeId::ROOT).attributes_end(), part: Part::HydrationKey });
        let mut memo_tests: std::vec::Vec<Option<&Embed<'a>>> =
            vec![None; template.memo_count as usize];
        let mut spread_props: std::vec::Vec<&Props<'a>> = std::vec::Vec::new();
        for op in &template.ops {
            match op {
                Op::Set { node: id, target, value } => {
                    place_set(&mut parts, template, *id, *target, value)
                }
                Op::Event { .. } | Op::Ref { .. } => {}
                Op::Spread { node: id, props, is_svg, has_children } => {
                    let spread_temp = (!has_children).then(|| {
                        spread_props.push(props);
                        spread_props.len() - 1
                    });
                    parts.push(Placed {
                        at: node(*id).attributes_end(),
                        part: Part::Spread { props, is_svg: *is_svg, spread_temp },
                    });
                    if let Some(spread_temp) = spread_temp {
                        parts.push(Placed {
                            at: node(*id).content_end(),
                            part: Part::SpreadChildren { spread_temp },
                        });
                    }
                }
                Op::Memo { id, test } => memo_tests[id.0 as usize] = Some(test),
                Op::ServerClass { node: id, toggles, inside } => parts.push(Placed {
                    at: inside.unwrap_or_else(|| node(*id).attributes_end()),
                    part: Part::ClassToggles { toggles, is_inside_static_class: inside.is_some() },
                }),
                Op::Insert { parent, value, anchor, .. } => {
                    let (at, is_bracketed) = match anchor {
                        Anchor::Only => (node(*parent).content_end(), false),
                        Anchor::End => (node(*parent).content_end(), true),
                        Anchor::Before(next) => (node(*next).start(), true),
                    };
                    parts.push(Placed { at, part: Part::Insert { value, is_bracketed } });
                }
            }
        }
        for bind in &template.binds {
            place_set(&mut parts, template, bind.node, bind.target, &bind.value);
        }
        parts.sort_by_key(|placed| placed.at);

        let name = self.split_template(template.html, &parts);
        let spread_names: std::vec::Vec<&'a str> =
            spread_props.iter().map(|_| self.fresh("_s$")).collect();
        if !spread_props.is_empty() {
            out.push("(() => {\n  var ");
            for (i, (props, spread_name)) in spread_props.iter().zip(&spread_names).enumerate() {
                if i > 0 {
                    out.push(",\n    ");
                }
                let _ = write!(out, "{spread_name} = ");
                self.props(out, props);
            }
            out.push(";\n  return ");
        }
        let ssr = self.helper(Helper::Ssr);
        let _ = write!(out, "{ssr}({name}");
        for placed in &parts {
            out.push(", ");
            self.server_part(out, &placed.part, &memo_tests, &spread_names);
        }
        out.push(")");
        if !spread_props.is_empty() {
            out.push(";\n})()");
        }
    }

    /// Declares the static strings between `parts` and returns their name.
    fn split_template(&mut self, html: &'a str, parts: &[Placed<'_, 'a>]) -> &'a str {
        let mut strings = std::vec::Vec::with_capacity(parts.len() + 1);
        let mut current = String::new();
        let mut position = 0;
        for placed in parts {
            current.push_str(&html[position..placed.at as usize]);
            position = placed.at as usize;
            let is_bracketed = matches!(placed.part, Part::Insert { is_bracketed: true, .. });
            if is_bracketed {
                current.push_str(INSERT_OPEN);
            }
            strings.push(self.alloc.alloc_str(&current));
            current.clear();
            if is_bracketed {
                current.push_str(INSERT_CLOSE);
            }
        }
        current.push_str(&html[position..]);
        strings.push(self.alloc.alloc_str(&current));
        self.string_template_name(strings)
    }

    fn server_part(
        &mut self,
        out: &mut Code,
        part: &Part<'_, 'a>,
        memo_tests: &[Option<&Embed<'a>>],
        spread_names: &[&'a str],
    ) {
        match part {
            Part::HydrationKey => {
                let key = self.helper(Helper::SsrHydrationKey);
                let _ = write!(out, "{key}()");
            }
            Part::Set { target, value } => self.server_set(out, *target, value),
            Part::ClassToggles { toggles, is_inside_static_class } => {
                let helper = self.helper(if *is_inside_static_class {
                    Helper::SsrClassTokens
                } else {
                    Helper::SsrClass
                });
                out.push(helper);
                out.push("(");
                self.value(out, toggles);
                out.push(")");
            }
            Part::Spread { props, is_svg, spread_temp } => {
                let spread = self.helper(Helper::SsrSpread);
                let _ = write!(out, "{spread}(");
                match spread_temp {
                    Some(index) => out.push(spread_names[*index]),
                    None => self.props(out, props),
                }
                let _ = write!(out, ", {is_svg})");
            }
            Part::SpreadChildren { spread_temp } => {
                let child = self.helper(Helper::SsrChild);
                let _ = write!(out, "{child}({}.children)", spread_names[*spread_temp]);
            }
            Part::Insert { value, .. } => {
                let child = self.helper(Helper::SsrChild);
                let _ = write!(out, "{child}(");
                self.server_insert_value(out, value, memo_tests);
                out.push(")");
            }
        }
    }

    fn server_set(&mut self, out: &mut Code, target: BindTarget<'a>, value: &Value<'a>) {
        let (helper, name) = match target {
            BindTarget::Attr(name)
            | BindTarget::AttrNs(_, name)
            | BindTarget::Prop { name, html: PropHtml::Attr } => (Helper::SsrAttribute, Some(name)),
            BindTarget::Bool(name) | BindTarget::Prop { name, html: PropHtml::Bool } => {
                (Helper::SsrBoolAttribute, Some(name))
            }
            BindTarget::Class => (Helper::SsrClass, None),
            BindTarget::Style => (Helper::SsrStyle, None),
            BindTarget::Prop { html: PropHtml::Text, .. } => (Helper::SsrChild, None),
            BindTarget::Prop { html: PropHtml::Html, .. } => (Helper::SsrRaw, None),
            BindTarget::Prop { html: PropHtml::None, .. } | BindTarget::ClassToggle(_) => return,
        };
        let helper = self.helper(helper);
        out.push(helper);
        out.push("(");
        if let Some(name) = name {
            push_js_string(&mut out.text, name);
            out.push(", ");
        }
        self.value(out, value);
        out.push(")");
    }

    /// The value an insert renders, evaluated in place: getters are not wrapped, and a
    /// conditional reads its test directly instead of through a memo.
    fn server_insert_value(
        &mut self,
        out: &mut Code,
        value: &Child<'a>,
        memo_tests: &[Option<&Embed<'a>>],
    ) {
        match value {
            Child::Expr(ExprChild::Getter(Getter::Thunk { body, .. })) => self.embed(out, body),
            Child::Expr(ExprChild::Getter(Getter::Call(callee))) => {
                self.src(out, *callee);
                out.push("()");
            }
            Child::Expr(ExprChild::Conditional(conditional)) => {
                let test = memo_tests[conditional.memo.0 as usize]
                    .expect("the template's `Op::Memo` precedes its conditional");
                out.push("(");
                self.embed(out, test);
                out.push(") ? ");
                self.embed(out, &conditional.consequent);
                out.push(" : ");
                match &conditional.alternate {
                    Some(alternate) => self.embed(out, alternate),
                    None => out.push("null"),
                }
            }
            other => self.child(out, other, &[]),
        }
    }
}
