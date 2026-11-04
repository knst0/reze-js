const SKILL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/compiler/skills/reze-compiler-diagnostics/SKILL.md"
);

#[test]
fn skill_matches_the_catalog() {
    let skill = reze_compiler::render_skill();
    if std::env::var_os("REZE_UPDATE_SKILL").is_some() {
        std::fs::write(SKILL, skill).unwrap();
    } else {
        assert!(
            std::fs::read_to_string(SKILL).unwrap() == skill,
            "SKILL.md is stale: regenerate with `REZE_UPDATE_SKILL=1 cargo test -p reze_compiler --test skill`"
        );
    }
}
