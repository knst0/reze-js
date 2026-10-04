use reze_compiler::{Code, CompileTarget, Options, Severity, compile};

#[test]
fn nonclient_targets_require_identity_even_without_transformable_source() {
    for (target, name) in [(CompileTarget::Hydrate, "hydrate"), (CompileTarget::Html, "html")] {
        for module_id in [None, Some(String::new())] {
            for source in ["const value = 1;", "const view = <div />;", "const ="] {
                let options = Options { target, module_id: module_id.clone(), ..Options::default() };
                let errors = compile(source, "test.tsx", &options).err().expect("missing identity");
                assert_eq!(errors.len(), 1);
                let error = &errors[0];
                assert_eq!(error.code, Code::MissingModuleId);
                assert_eq!(error.severity, Severity::Error);
                assert_eq!(error.file, "test.tsx");
                assert_eq!((error.start.offset, error.start.line, error.start.column), (0, 1, 0));
                assert_eq!(error.data.get("target").map(String::as_str), Some(name));
            }
        }
    }
}

#[test]
fn modules_without_transformations_remain_unchanged_for_each_target() {
    for target in [CompileTarget::Client, CompileTarget::Hydrate, CompileTarget::Html] {
        let options = Options {
            target,
            module_id: Some("src/constant.ts".into()),
            ..Options::default()
        };
        assert!(compile("export const value = 1;", "constant.ts", &options).unwrap().is_none());
    }
}
