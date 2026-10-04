use reze_compiler::profile_hash;

#[test]
fn profile_hash_is_fnv1a64() {
    assert_eq!(profile_hash(""), "cbf29ce484222325");
}
