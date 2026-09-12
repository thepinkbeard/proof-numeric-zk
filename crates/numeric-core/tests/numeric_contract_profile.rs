use numeric_core::{NumericContractProfile, SemanticClass};

#[test]
fn tracked_public_profiles_are_valid_and_keep_rigor_labels() {
    let reference = NumericContractProfile::from_json(include_bytes!(
        "../../../profiles/non_polynomial/v1/mpfr256_exp_reference.json"
    ))
    .unwrap();
    let bounded = NumericContractProfile::from_json(include_bytes!(
        "../../../profiles/non_polynomial/v1/q16_16_exp_taylor_9_floor.json"
    ))
    .unwrap();
    let observed = NumericContractProfile::from_json(include_bytes!(
        "../../../profiles/non_polynomial/v1/q16_16_sigmoid_gl_4_artifact_rne.json"
    ))
    .unwrap();

    assert_eq!(
        reference.semantic_class(),
        SemanticClass::ExactReferenceSemantics
    );
    assert_eq!(
        bounded.semantic_class(),
        SemanticClass::BoundedApproximation
    );
    assert_eq!(observed.semantic_class(), SemanticClass::UnboundedSemantics);
    assert_ne!(bounded.profile_hash_hex(), observed.profile_hash_hex());
}
