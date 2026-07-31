use num_bigint::BigUint;
use num_rational::BigRational;
use num_traits::{One, Zero};
use numeric_core::{CertifiedFloat, CfdOperation, CfdProfile, Decision3};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

fn unit_profile(p: usize) -> CfdProfile {
    CfdProfile::bounded_unit(p).unwrap()
}

#[test]
fn profile_hash_is_deterministic_and_commits_to_corridor() {
    let first = unit_profile(24);
    let second = unit_profile(24);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.profile_hash(), second.profile_hash());

    let changed = CfdProfile::new(
        "cfd-unit-p24",
        1,
        24,
        vec![-15, -1, 0],
        16,
        BigRational::zero(),
        BigRational::one(),
        vec![CfdOperation::Multiply],
        vec![BigRational::new(1.into(), 2.into())],
        None,
    )
    .unwrap();
    assert_ne!(first.profile_hash(), changed.profile_hash());
}

#[test]
fn inexact_decimal_is_enclosed_in_ulps() {
    let exact = BigRational::new(1.into(), 10.into());
    let value = CertifiedFloat::from_exact(unit_profile(24), &exact).unwrap();
    assert!(!value.is_wide());
    assert!(value.contains(&exact));
    assert!(value.radius() <= &BigUint::one());
    assert!(value.significand().bits() == 24);
}

#[test]
fn wide_is_canonical_absorbing_and_indeterminate() {
    let profile = unit_profile(24);
    let wide = CertifiedFloat::wide(profile.clone());
    let exact = CertifiedFloat::from_exact(profile, &BigRational::new(1.into(), 2.into())).unwrap();
    assert_eq!(wide.mul(&exact).unwrap(), wide);
    assert_eq!(wide.add(&exact).unwrap(), wide);
    assert_eq!(
        wide.ge_threshold(&BigRational::new(1.into(), 2.into())),
        Decision3::Indeterminate
    );
}

#[test]
fn value_outside_exponent_corridor_becomes_wide_not_clamped() {
    let profile = CfdProfile::new(
        "narrow",
        1,
        24,
        vec![-16],
        16,
        BigRational::zero(),
        BigRational::one(),
        vec![CfdOperation::Multiply],
        vec![BigRational::new(1.into(), 2.into())],
        None,
    )
    .unwrap();
    let exact = BigRational::new(1.into(), 2.into());
    let value = CertifiedFloat::from_exact(profile, &exact).unwrap();
    assert!(value.is_wide());
    assert!(value.contains(&exact));
}

#[test]
fn binary32_normals_and_positive_zero_round_trip() {
    let profile = CfdProfile::new(
        "binary32-normal",
        1,
        24,
        (-126..=127).collect(),
        16,
        BigRational::from_integer((-4).into()),
        BigRational::from_integer(4.into()),
        vec![CfdOperation::Add],
        vec![BigRational::zero()],
        None,
    )
    .unwrap();
    for bits in [
        0_u32,
        0x3dcc_cccd,
        0x3f00_0000,
        0x3f80_0000,
        0x4000_0000,
        0xbf80_0000,
    ] {
        let value = CertifiedFloat::from_binary32_bits(profile.clone(), bits).unwrap();
        assert_eq!(value.to_binary32_bits().unwrap(), bits);
        assert_eq!(value.radius(), &BigUint::zero());
    }
}

#[test]
fn sequential_and_fused_logistic_steps_enclose_exact_result() {
    let exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let input = CertifiedFloat::from_exact(unit_profile(24), &exact).unwrap();
    let sequential = input.logistic_step_sequential(&r).unwrap();
    let fused = input.logistic_step_fused(&r).unwrap();
    let reference = &r * &exact * (BigRational::one() - &exact);
    assert!(sequential.contains(&reference));
    assert!(fused.contains(&reference));
}

#[test]
fn stable_affine_composition_encloses_rational_trajectory() {
    let profile = CfdProfile::stable_score(24).unwrap();
    let alpha = BigRational::new(1.into(), 2.into());
    let innovation_exact = BigRational::new(3.into(), 5.into());
    let innovation = CertifiedFloat::from_exact(profile.clone(), &innovation_exact).unwrap();
    let mut exact = BigRational::zero();
    let mut state = CertifiedFloat::from_exact(profile, &exact).unwrap();
    for _ in 0..20 {
        exact = &alpha * exact + &innovation_exact;
        state = state.stable_affine_step(&alpha, &innovation).unwrap();
        assert!(state.contains(&exact));
    }
}

#[test]
fn threshold_uses_the_whole_enclosure() {
    let profile = unit_profile(24);
    let threshold = BigRational::new(1.into(), 2.into());
    let crossing = CertifiedFloat::from_parts(
        profile,
        false,
        7,
        BigUint::one() << 23,
        BigUint::one(),
        false,
        false,
    )
    .unwrap();
    assert_eq!(crossing.center(), threshold);
    assert_eq!(crossing.ge_threshold(&threshold), Decision3::Indeterminate);
}

#[test]
fn exhaustive_reduced_format_has_no_unsound_enclosure_or_decision() {
    let profile = CfdProfile::new(
        "cfd-exhaustive-p4",
        1,
        4,
        (-3..=0).collect(),
        3,
        BigRational::zero(),
        BigRational::one(),
        vec![CfdOperation::Add, CfdOperation::Multiply],
        vec![BigRational::new(1.into(), 2.into())],
        None,
    )
    .unwrap();
    let mut values =
        vec![CertifiedFloat::from_exact(profile.clone(), &BigRational::zero()).unwrap()];
    for lane in 0..profile.exponent_lanes().len() {
        for significand in 8_u32..16 {
            for radius in 0_u32..8 {
                if let Ok(value) = CertifiedFloat::from_parts(
                    profile.clone(),
                    false,
                    lane,
                    significand.into(),
                    radius.into(),
                    false,
                    false,
                ) {
                    if value.lower() >= BigRational::zero() && value.upper() <= BigRational::one() {
                        values.push(value);
                    }
                }
            }
        }
    }
    let threshold = BigRational::new(1.into(), 2.into());
    for left in &values {
        match left.ge_threshold(&threshold) {
            Decision3::True => assert!(left.lower() >= threshold.clone()),
            Decision3::False => assert!(left.upper() < threshold.clone()),
            Decision3::Indeterminate => {}
        }
        for right in &values {
            let product = left.mul(right).unwrap();
            assert!(product.lower() <= left.lower() * right.lower());
            assert!(product.upper() >= left.upper() * right.upper());
            if left.upper() + right.upper() <= BigRational::one() {
                let sum = left.add(right).unwrap();
                assert!(sum.lower() <= left.lower() + right.lower());
                assert!(sum.upper() >= left.upper() + right.upper());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        rng_seed: RngSeed::Fixed(0x4346_4459_4144_2026),
        .. ProptestConfig::default()
    })]

    #[test]
    fn encoding_encloses_unit_rationals(numerator in 0_u32..=1_000_000) {
        let exact = BigRational::new(numerator.into(), 1_000_000_u32.into());
        let encoded = CertifiedFloat::from_exact(unit_profile(24), &exact).unwrap();
        prop_assert!(encoded.contains(&exact));
    }

    #[test]
    fn arithmetic_encloses_exact_results(left in 0_u16..=u16::MAX, right in 0_u16..=u16::MAX) {
        let profile = unit_profile(24);
        let a_exact = BigRational::new(left.into(), u16::MAX.into());
        let b_exact = BigRational::new(right.into(), u16::MAX.into());
        let a = CertifiedFloat::from_exact(profile.clone(), &a_exact).unwrap();
        let b = CertifiedFloat::from_exact(profile, &b_exact).unwrap();
        prop_assert!(a.mul(&b).unwrap().contains(&(a_exact.clone() * b_exact.clone())));
        if a_exact.clone() + b_exact.clone() <= BigRational::one() {
            prop_assert!(a.add(&b).unwrap().contains(&(a_exact + b_exact)));
        }
    }
}
