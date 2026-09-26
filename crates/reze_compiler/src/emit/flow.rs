use super::{Emitter, Helper};
use crate::code::Code;
use crate::ir::{Branch, Flow, Render};

impl<'a> Emitter<'a, '_> {
    pub(super) fn flow(&mut self, out: &mut Code, flow: &Flow<'a>) {
        match flow {
            Flow::Show(branch) => self.branch(out, branch),
            Flow::Switch { whens, children, fallback } => {
                let choose = self.helper(Helper::Choose);
                out.push(choose);
                out.push("([");
                for (i, when) in whens.iter().enumerate() {
                    out.push(if i > 0 { ", () => " } else { "() => " });
                    self.embed(out, when);
                }
                out.push("], [");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        out.push(", ");
                    }
                    self.render(out, child);
                }
                out.push("]");
                self.fallback(out, fallback.as_ref());
                out.push(")");
            }
            Flow::For { each, map, fallback, key, selectors } => {
                if !selectors.is_empty() {
                    out.push("((");
                    for (i, selector) in selectors.iter().enumerate() {
                        if i > 0 {
                            out.push(", ");
                        }
                        out.push(selector.name);
                    }
                    out.push(") => ");
                }
                let list = self.helper(Helper::List);
                out.push(list);
                out.push("(() => ");
                self.embed(out, each);
                out.push(", ");
                self.embed(out, map);
                match (fallback, key) {
                    (Some(fallback), _) => self.fallback(out, Some(fallback)),
                    (None, Some(_)) => out.push(", undefined"),
                    (None, None) => {}
                }
                if let Some(key) = key {
                    out.push(", ");
                    self.embed(out, key);
                }
                out.push(")");
                if !selectors.is_empty() {
                    let selector_helper = self.helper(Helper::Selector);
                    out.push(")(");
                    for (i, selector) in selectors.iter().enumerate() {
                        if i > 0 {
                            out.push(", ");
                        }
                        out.push(selector_helper);
                        out.push("(");
                        self.src(out, selector.source);
                        out.push(")");
                    }
                    out.push(")");
                }
            }
        }
    }

    fn branch(&mut self, out: &mut Code, branch: &Branch<'a>) {
        let helper = self.helper(Helper::Branch);
        out.push(helper);
        out.push("(() => ");
        self.embed(out, &branch.when);
        out.push(", ");
        self.render(out, &branch.child);
        self.fallback(out, branch.fallback.as_ref());
        out.push(")");
    }

    fn fallback(&mut self, out: &mut Code, fallback: Option<&Render<'a>>) {
        if let Some(fallback) = fallback {
            out.push(", ");
            self.render(out, fallback);
        }
    }

    fn render(&mut self, out: &mut Code, render: &Render<'a>) {
        match render {
            Render::Function(embed) => self.embed(out, embed),
            Render::Child(child) => {
                out.push("() => ");
                self.child(out, child);
            }
        }
    }
}
