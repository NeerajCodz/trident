use praxis::index::SpecializedEngineRegistry;

#[test]
fn specialized_engine_defaults_are_pointer_oriented_and_manifest_tracked() {
    let registry = SpecializedEngineRegistry::shipping_defaults();

    assert!(registry.contracts().len() >= 9);
    assert!(registry.all_pointer_oriented());
    assert!(
        registry
            .contracts()
            .iter()
            .all(|contract| contract.format.validate().is_ok())
    );
}
