use std::path::PathBuf;

use reze_compiler::diagnostic::catalog::CATALOG;
use reze_compiler::facts::ModuleFacts;
use reze_compiler::{
    Code, Diagnostic, LinkOptions, Linked, ModuleInput, Options, OutsideModule, SummaryOptions,
    compile, link, render_skill, summarize, verify,
};

fn skill_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/compiler/skills/compiler-diagnostics/SKILL.md")
}

#[test]
fn skill_guide_matches_the_catalog() {
    let rendered = render_skill();
    let path = skill_path();
    if std::env::var_os("REZE_UPDATE_SKILL").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == rendered,
        "{} is stale; run `REZE_UPDATE_SKILL=1 cargo test -p reze-compiler --test catalog`",
        path.display()
    );
}

enum Trigger {
    Module(&'static str),
    Program { file: &'static str },
    StaleFacts,
    Outside { closed: bool, suspense: bool },
}

const PAGE: &str = "import { Counter } from \"./counter\";\nexport function Page() { return <main><Counter start={1} island:load=\"visible\" /></main>; }";
const COUNTER: &str = "export function Counter(props) { return <button island:load=\"idle\" onClick={() => alert(props.start)} />; }";
fn trigger(code: Code) -> Trigger {
    let source = match code {
        Code::ParseError => "const a = <div>;",
        Code::ClassAlias => "const a = <div className=\"x\" />;",
        Code::ChildrenPropIgnored => "const a = <div children={x()}><b /></div>;",
        Code::KeyOnElement => "const a = <li key={id} />;",
        Code::DuplicateAttribute => "const a = <a href=\"/a\" href={u()} />;",
        Code::UnknownAttribute => "const a = <div clas=\"x\" />;",
        Code::EventNameLowercase => "const a = <button onclick={() => save()} />;",
        Code::SignalNotCalled => {
            "import { signal } from \"reze-js\";\nconst [n, setN] = signal(0); setN(1);\nconst a = <input value={n} />;"
        }
        Code::PropsDestructured => "function Greeting({ name = f() }) { return <p>{name}</p>; }",
        Code::PropsRewritten => "function Greeting({ name }) { return <p>{name}</p>; }",
        Code::InlineEach => "const a = <For each={[1, 2]}>{(n) => n}</For>;",
        Code::AsyncComponentShape => "async function A() { await ready(); return <p />; }",
        Code::AsyncReturnType => {
            "async function A(): Promise { const x = await f(); return <p>{x}</p>; }"
        }
        Code::SignalFolded => {
            "import { signal } from \"reze-js\";\nconst [t] = signal(\"x\");\nconst a = <p>{t()}</p>;"
        }
        Code::DeadBranchRemoved => "const a = <div>{false && <b />}</div>;",
        Code::ComputedInlined => {
            "import { computed } from \"reze-js\";\nfunction A() { const d = computed(() => x() + 1); return <p>{d()}</p>; }"
        }
        Code::StoreUnproxied => {
            "import { store } from \"reze-js\";\nconst [s] = store({ a: 1 });\nconst a = <p>{s.a}</p>;"
        }
        Code::StaticComponent | Code::Island | Code::LazyIsland => {
            return Trigger::Program { file: "/page.tsx" };
        }
        Code::IslandDirectiveIgnored => return Trigger::Program { file: "/counter.tsx" },
        Code::ClientComponent => return Trigger::Program { file: "/counter.tsx" },
        Code::FactsStale => return Trigger::StaleFacts,
        Code::ProgramOpenImport => return Trigger::Outside { closed: true, suspense: true },
        Code::FeatureFlagMismatch => return Trigger::Outside { closed: false, suspense: false },
    };
    Trigger::Module(source)
}

/// `/page.tsx` renders the client `/counter.tsx` as an island; the facts of `file`.
fn island_program_facts(file: &str) -> (ModuleFacts, &'static str) {
    let files = [("/page.tsx", PAGE), ("/counter.tsx", COUNTER)];
    let modules = files
        .iter()
        .map(|(id, source)| {
            let summary = summarize(source, id, &SummaryOptions::default()).unwrap();
            let resolved = summary
                .specifiers
                .iter()
                .map(|s| (s == "./counter").then(|| "/counter.tsx".to_string()))
                .collect();
            ModuleInput { id: id.to_string(), summary, resolved, is_entry: false }
        })
        .collect::<Vec<_>>();
    let linked = link(&modules, &LinkOptions { optimize: true, islands: true, root: "/".into() });
    let source = files.iter().find(|(id, _)| *id == file).unwrap().1;
    (linked.facts[file].clone(), source)
}

fn diagnostics(trigger: Trigger) -> Vec<Diagnostic> {
    let outcome = match trigger {
        Trigger::Module(source) => compile(source, "case.tsx", &Options::default()),
        Trigger::Program { file } => {
            let (facts, source) = island_program_facts(file);
            compile(source, file, &Options { facts: Some(facts), ..Options::default() })
        }
        Trigger::StaleFacts => {
            let (facts, _) = island_program_facts("/page.tsx");
            compile(
                "const changed = <p />;",
                "/page.tsx",
                &Options { facts: Some(facts), ..Options::default() },
            )
        }
        Trigger::Outside { closed, suspense } => {
            let linked = Linked {
                facts: Default::default(),
                features: [("suspense".to_string(), suspense)].into_iter().collect(),
                closed: if closed { vec!["/state.ts".to_string()] } else { Vec::new() },
            };
            let source = "import { Suspense } from \"reze-js\";\nimport { title } from \"./state\";\nexport { Suspense, title };";
            let outside = OutsideModule {
                id: "/outside.ts".to_string(),
                summary: Some(
                    summarize(source, "/outside.ts", &SummaryOptions::default()).unwrap(),
                ),
                imported: vec!["/state.ts".to_string()],
            };
            return verify(&linked, &[outside]);
        }
    };
    match outcome {
        Ok(out) => out.expect("rewrites something").diagnostics,
        Err(errors) => errors,
    }
}

#[test]
fn every_catalog_code_is_emitted_with_its_catalog_severity() {
    for entry in CATALOG {
        let diagnostics = diagnostics(trigger(entry.code));
        let found = diagnostics.iter().find(|d| d.code == entry.code);
        let found = found.unwrap_or_else(|| panic!("{} not emitted: {diagnostics:?}", entry.name));
        assert_eq!(found.severity, entry.severity, "{}", entry.name);
        assert!(found.message.starts_with(&format!("[{}] ", entry.name)), "{}", found.message);
    }
}
