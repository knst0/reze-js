use std::path::PathBuf;

use reze_compiler::diagnostic::catalog::CATALOG;
use reze_compiler::{Code, Options, compile, render_skill};

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

fn trigger(code: Code) -> &'static str {
    match code {
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
        Code::PropsDestructured => "function Greeting({ name }) { return <p>{name}</p>; }",
        Code::InlineEach => "const a = <For each={[1, 2]}>{(n) => n}</For>;",
        Code::AsyncComponentShape => "async function A() { await ready(); return <p />; }",
        Code::AsyncReturnType => {
            "async function A(): Promise { const x = await f(); return <p>{x}</p>; }"
        }
        Code::SignalFolded => {
            "import { signal } from \"reze-js\";\nconst [t] = signal(\"x\");\nconst a = <p>{t()}</p>;"
        }
        Code::DeadBranchRemoved => "const a = <div>{false && <b />}</div>;",
    }
}

#[test]
fn every_catalog_code_is_emitted_with_its_catalog_severity() {
    for entry in CATALOG {
        let diagnostics = match compile(trigger(entry.code), "case.tsx", &Options::default()) {
            Ok(out) => out.expect("has JSX").diagnostics,
            Err(errors) => errors,
        };
        let found = diagnostics.iter().find(|d| d.code == entry.code);
        let found = found.unwrap_or_else(|| panic!("{} not emitted: {diagnostics:?}", entry.name));
        assert_eq!(found.severity, entry.severity, "{}", entry.name);
        assert!(found.message.starts_with(&format!("[{}] ", entry.name)), "{}", found.message);
    }
}
