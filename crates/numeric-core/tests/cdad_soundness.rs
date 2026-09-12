use num_rational::BigRational;
use num_traits::{One, Zero};
use numeric_core::{
    CdadError, CertifiedDyadic, CommitmentPolicy, Decision3, DecisionAdaptiveProfile,
    PrecisionPolicy,
};

fn reduced_profile(policy: PrecisionPolicy) -> DecisionAdaptiveProfile {
    DecisionAdaptiveProfile::new(
        format!("cdad-reduced-{}", policy.tag()),
        1,
        vec![2, 4, 6],
        BigRational::zero(),
        BigRational::one(),
        vec![
            BigRational::new(1.into(), 4.into()),
            BigRational::new(1.into(), 2.into()),
            BigRational::new(3.into(), 4.into()),
        ],
        policy,
        CommitmentPolicy::ExistingPoseidonLink,
    )
    .unwrap()
}

#[test]
fn exhaustive_reduced_logistic_decisions_are_sound() {
    let r = BigRational::new(7.into(), 2.into());
    for policy in [
        PrecisionPolicy::Fixed,
        PrecisionPolicy::Sequential,
        PrecisionPolicy::MarginDirected,
    ] {
        let profile = reduced_profile(policy);
        for numerator in 0..=64 {
            let x0 = BigRational::new(numerator.into(), 64.into());
            for steps in 0..=3 {
                let mut exact = x0.clone();
                for _ in 0..steps {
                    exact = &r * &exact * (BigRational::one() - &exact);
                }
                for threshold in profile.thresholds() {
                    let result = profile
                        .evaluate(threshold, |numeric| {
                            let mut value = CertifiedDyadic::from_exact(numeric.clone(), &x0)?;
                            for _ in 0..steps {
                                value = value.logistic_step_local_bound(&r)?;
                            }
                            Ok(value)
                        })
                        .unwrap();
                    assert!(result.value.contains(&exact));
                    match result.decision {
                        Decision3::True => assert!(exact >= *threshold),
                        Decision3::False => assert!(exact < *threshold),
                        Decision3::Indeterminate => {}
                    }
                }
            }
        }
    }
}

#[test]
fn foreign_numeric_profile_is_rejected() {
    let profile = reduced_profile(PrecisionPolicy::Sequential);
    let foreign = numeric_core::NumericProfile::bounded_unit(16).unwrap();
    let result = profile.evaluate(&BigRational::new(1.into(), 2.into()), |_| {
        CertifiedDyadic::from_exact(foreign.clone(), &BigRational::new(1.into(), 3.into()))
    });
    assert!(matches!(result, Err(CdadError::InnerProfileMismatch)));
}
