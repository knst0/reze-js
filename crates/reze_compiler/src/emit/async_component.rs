//! The synchronous shape of an async component (SPEC §7.9): one fetch `effect` per `await`,
//! linked by an epoch, and a `memo` running the rest once every step settled. Server and
//! hydrate also stream it (SPEC §16.8): the component captures its boundary on entry, each
//! await loads through `ssrAwait`/`streamValue`, and `streamOutput` hands out the `memo`.

use std::fmt::Write;

use super::{Emitter, Helper};
use crate::Target;
use crate::code::Code;
use crate::ir::{AsyncComponent, AsyncHead, AsyncStep, Embed, ReturnType};

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
    /// Bumped each time every step settled: the result memo re-runs only then.
    done: &'a str,
    set_done: &'a str,
    steps: std::vec::Vec<StepNames<'a>>,
}

struct Streaming<'a> {
    capture: &'a str,
    boundary: &'a str,
    /// `ssrAwait` on the server, `streamValue` when hydrating.
    load: &'a str,
    output: &'a str,
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
            done: self.fresh("_done$"),
            set_done: self.fresh("_setDone$"),
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
        let streaming = self.streaming();
        let names = self.async_names(component.steps.len());
        let on_cleanup = self.helper(Helper::OnCleanup);
        let track_pending = self.helper(Helper::TrackPending);
        let effect = self.helper(Helper::Effect);
        let track_async = self.helper(Helper::TrackAsync);
        let memo = self.helper(Helper::Memo);
        let unknown = if self.is_typescript { "undefined as any" } else { "undefined" };
        let untrack = self.helper(Helper::Untrack);
        let Names {
            owner, error, set_error, epoch, promise, mine, thrown, signal, done, set_done, ..
        } = names;

        let _ = writeln!(out, "{{\nconst {owner} = {get_owner}();");
        if let Some(streaming) = &streaming {
            let _ = writeln!(out, "const {} = {}();", streaming.boundary, streaming.capture);
        }
        if let Some(props_entry) = &component.props_entry {
            self.embed(out, props_entry);
            out.push("\n");
        }
        for step in &names.steps {
            let _ =
                writeln!(out, "const [{}, {}] = {signal}({unknown});", step.value, step.set_value);
            let _ =
                writeln!(out, "const [{}, {}] = {signal}(false);", step.settled, step.set_settled);
        }
        let _ = writeln!(out, "const [{error}, {set_error}] = {signal}({unknown});");
        let _ = writeln!(out, "const [{done}, {set_done}] = {signal}(0);");
        let _ = writeln!(out, "let {epoch} = 0;");
        let _ = writeln!(out, "{on_cleanup}(() => {{ {epoch}++; }});");
        let _ = write!(out, "{track_pending}(() => (");
        for (index, step) in names.steps.iter().enumerate() {
            if index > 0 {
                out.push(" || ");
            }
            let _ = write!(out, "!{}()", step.settled);
        }
        let _ = writeln!(out, ") && {error}() === undefined);");
        let last_index = component.steps.len() - 1;
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
            let _ = write!(out, "const {promise} = ");
            match &streaming {
                None => {
                    out.push("Promise.resolve(");
                    self.embed(out, &step.argument);
                    out.push(")");
                }
                Some(streaming) => {
                    let is_server = self.target == Target::Server;
                    if is_server {
                        out.push("Promise.resolve(");
                    }
                    let _ = write!(out, "{}({}, {index}, ", streaming.load, streaming.boundary);
                    self.await_loader(out, &step.argument);
                    out.push(if is_server { "))" } else { ")" });
                }
            }
            out.push(";\n");
            let _ = writeln!(out, "const {mine} = ++{epoch};");
            let bump = if index == last_index {
                format!(" {set_done}((n) => n + 1);")
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "{track_async}({owner}, {promise}, () => {mine} === {epoch}, (_v) => {{ {}(_v); {}(true);{bump} }}, {set_error});",
                own.set_value, own.set_settled,
            );
            out.push("});\n");
        }
        match &streaming {
            None => {
                let _ = writeln!(out, "return {memo}(() => {{");
            }
            Some(streaming) => {
                let _ = writeln!(
                    out,
                    "return {}({}, {memo}(() => {{",
                    streaming.output, streaming.boundary
                );
            }
        }
        let _ = writeln!(
            out,
            "const {thrown} = {error}();\nif ({thrown} !== undefined) throw {thrown};"
        );
        let _ = writeln!(out, "if ({done}() === 0) return undefined;");
        for (step, name) in component.steps.iter().zip(&names.steps) {
            let _ = write!(out, "const {} = {untrack}({});\n{} ", name.read, name.value, step.kind);
            self.src(out, step.pattern);
            if let Some(annotation) = step.annotation {
                self.src(out, annotation);
            }
            let _ = writeln!(out, " = {};", name.read);
        }
        for statement in &component.tail {
            self.embed(out, statement);
            out.push("\n");
        }
        out.push("return ");
        self.embed(out, &component.result);
        out.push(if streaming.is_some() { ";\n}));\n}" } else { ";\n});\n}" });
    }

    /// The boundary capture and the helpers of server and hydrate streaming (SPEC §16.8).
    fn streaming(&mut self) -> Option<Streaming<'a>> {
        let load = match self.target {
            Target::Client => return None,
            Target::Server => Helper::SsrAwait,
            Target::Hydrate => Helper::StreamValue,
        };
        Some(Streaming {
            capture: self.helper(Helper::StreamBoundary),
            boundary: self.fresh("_boundary$"),
            load: self.helper(load),
            output: self.helper(Helper::StreamOutput),
        })
    }

    /// `() => e`, parenthesized when `e` is an object literal.
    fn await_loader(&mut self, out: &mut Code, argument: &Embed<'a>) {
        let is_object = self.source[argument.span.start as usize..].starts_with('{');
        out.push(if is_object { "() => (" } else { "() => " });
        self.embed(out, argument);
        if is_object {
            out.push(")");
        }
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
