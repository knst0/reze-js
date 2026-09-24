//! The synchronous shape of an async component (SPEC §7.9): one fetch `effect` per `await`,
//! linked by an epoch, and a `memo` running the rest once every step settled.

use std::fmt::Write;

use super::{Emitter, Helper};
use crate::code::Code;
use crate::ir::{AsyncComponent, AsyncHead, AsyncStep, ReturnType};

struct StepNames<'a> {
    value: &'a str,
    set_value: &'a str,
    settled: &'a str,
    set_settled: &'a str,
    read: &'a str,
}

struct Names<'a> {
    owner: &'a str,
    error: &'a str,
    set_error: &'a str,
    epoch: &'a str,
    promise: &'a str,
    mine: &'a str,
    thrown: &'a str,
    signal: &'a str,
    steps: std::vec::Vec<StepNames<'a>>,
}

impl<'a> Emitter<'a, '_> {
    pub(super) fn async_component(&mut self, out: &mut Code, component: &AsyncComponent<'a>) {
        match &component.head {
            AsyncHead::Function { id, type_parameters } => {
                out.push("function ");
                if let Some(id) = id {
                    self.src(out, *id);
                }
                if let Some(type_parameters) = type_parameters {
                    self.src(out, *type_parameters);
                }
            }
            AsyncHead::Arrow { type_parameters } => {
                if let Some(type_parameters) = type_parameters {
                    self.src(out, *type_parameters);
                }
            }
        }
        self.embed(out, &component.params);
        match component.return_type {
            ReturnType::None => {}
            ReturnType::Unwrapped(inner) => {
                out.push(": ");
                self.src(out, inner);
            }
            ReturnType::Verbatim(annotation) => {
                out.push(" ");
                self.src(out, annotation);
            }
        }
        out.push(if matches!(component.head, AsyncHead::Arrow { .. }) { " => " } else { " " });
        self.async_body(out, component);
    }

    fn async_names(&mut self, steps: usize) -> Names<'a> {
        Names {
            owner: self.fresh("_owner$"),
            error: self.fresh("_err$"),
            set_error: self.fresh("_setErr$"),
            epoch: self.fresh("_epoch$"),
            promise: self.fresh("_p$"),
            mine: self.fresh("_my$"),
            thrown: self.fresh("_ex$"),
            signal: self.helper(Helper::Signal),
            steps: (0..steps)
                .map(|_| StepNames {
                    value: self.fresh("_val$"),
                    set_value: self.fresh("_setVal$"),
                    settled: self.fresh("_settled$"),
                    set_settled: self.fresh("_setSettled$"),
                    read: self.fresh("_t$"),
                })
                .collect(),
        }
    }

    fn async_body(&mut self, out: &mut Code, component: &AsyncComponent<'a>) {
        let get_owner = self.helper(Helper::GetOwner);
        let names = self.async_names(component.steps.len());
        let on_cleanup = self.helper(Helper::OnCleanup);
        let track_pending = self.helper(Helper::TrackPending);
        let effect = self.helper(Helper::Effect);
        let track_async = self.helper(Helper::TrackAsync);
        let memo = self.helper(Helper::Memo);
        let unknown = if self.is_typescript { "undefined as any" } else { "undefined" };
        let Names { owner, error, set_error, epoch, promise, mine, thrown, signal, .. } = names;

        let _ = writeln!(out, "{{\nconst {owner} = {get_owner}();");
        if let Some(props_rest) = &component.props_rest {
            self.embed(out, props_rest);
            out.push("\n");
        }
        for step in &names.steps {
            let _ =
                writeln!(out, "const [{}, {}] = {signal}({unknown});", step.value, step.set_value);
            let _ =
                writeln!(out, "const [{}, {}] = {signal}(false);", step.settled, step.set_settled);
        }
        let _ = writeln!(out, "const [{error}, {set_error}] = {signal}({unknown});");
        let _ = writeln!(out, "let {epoch} = 0;");
        let _ = writeln!(out, "{on_cleanup}(() => {{ {epoch}++; }});");
        let last = names.steps.last().map_or("", |step| step.settled);
        let _ = writeln!(out, "{track_pending}(() => !{last}() && {error}() === undefined);");
        for (index, step) in component.steps.iter().enumerate() {
            let _ = writeln!(out, "{effect}(() => {{");
            if index > 0 {
                self.await_prelude(out, &component.steps[..index], &names.steps, "return;");
            }
            for statement in &step.before {
                self.embed(out, statement);
                out.push("\n");
            }
            let own = &names.steps[index];
            let _ = writeln!(out, "{set_error}(undefined);\n{}(false);", own.set_settled);
            let _ = write!(out, "const {promise} = Promise.resolve(");
            self.embed(out, &step.argument);
            out.push(");\n");
            let _ = writeln!(out, "const {mine} = ++{epoch};");
            let _ = writeln!(
                out,
                "{track_async}({owner}, {promise}, () => {mine} === {epoch}, (_v) => {{ {}(_v); {}(true); }}, {set_error});",
                own.set_value, own.set_settled,
            );
            out.push("});\n");
        }
        let _ = writeln!(out, "return {memo}(() => {{");
        let _ = writeln!(
            out,
            "const {thrown} = {error}();\nif ({thrown} !== undefined) throw {thrown};"
        );
        self.await_prelude(out, &component.steps, &names.steps, "return undefined;");
        for statement in &component.tail {
            self.embed(out, statement);
            out.push("\n");
        }
        out.push("return ");
        self.embed(out, &component.result);
        out.push(";\n});\n}");
    }

    /// Bails out until every step so far settled, then redeclares their bindings.
    fn await_prelude(
        &mut self,
        out: &mut Code,
        steps: &[AsyncStep<'a>],
        names: &[StepNames<'a>],
        exit: &str,
    ) {
        out.push("if (");
        for (index, step) in names[..steps.len()].iter().enumerate() {
            if index > 0 {
                out.push(" || ");
            }
            let _ = write!(out, "!{}()", step.settled);
        }
        let _ = writeln!(out, ") {exit}");
        for (step, name) in steps.iter().zip(names) {
            let _ = write!(out, "const {} = {}();\n{} ", name.read, name.value, step.kind);
            self.src(out, step.pattern);
            if let Some(annotation) = step.annotation {
                self.src(out, annotation);
            }
            let _ = writeln!(out, " = {};", name.read);
        }
    }
}
