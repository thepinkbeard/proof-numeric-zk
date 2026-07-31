use num_bigint::{BigInt, BigUint};
use num_rational::BigRational;
use num_traits::{One, Zero};
use numeric_core::{CbdError, CertifiedDyadic, Decision3, NumericProfile};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

fn unit_profile(bits: usize) -> NumericProfile {
    NumericProfile::bounded_unit(bits).unwrap()
}

#[test]
fn exact_grid_point_has_zero_radius() {
    let value =
        CertifiedDyadic::from_exact(unit_profile(16), &BigRational::new(1.into(), 2.into()))
            .unwrap();
    assert_eq!(value.radius(), &BigUint::zero());
    assert_eq!(value.lower(), BigRational::new(1.into(), 2.into()));
    assert_eq!(value.upper(), BigRational::new(1.into(), 2.into()));
}

#[test]
fn inexact_input_gets_minimal_ulp_radius() {
    let exact = BigRational::new(1.into(), 10.into());
    let value = CertifiedDyadic::from_exact(unit_profile(16), &exact).unwrap();
    assert_eq!(value.radius(), &BigUint::one());
    assert!(value.contains(&exact));
    assert!(value.is_canonical_for_interval(&exact, &exact));
}

#[test]
fn threshold_decision_is_three_valued() {
    let profile = unit_profile(16);
    let threshold = BigRational::new(1.into(), 2.into());
    let below = CertifiedDyadic::from_parts(profile.clone(), 20_000.into(), 1_u8.into()).unwrap();
    let above = CertifiedDyadic::from_parts(profile.clone(), 40_000.into(), 1_u8.into()).unwrap();
    let crossing = CertifiedDyadic::from_parts(profile, BigInt::one() << 15, 1_u8.into()).unwrap();
    assert_eq!(below.ge_threshold(&threshold), Decision3::False);
    assert_eq!(above.ge_threshold(&threshold), Decision3::True);
    assert_eq!(crossing.ge_threshold(&threshold), Decision3::Indeterminate);
    assert_eq!(below.le_threshold(&threshold), Decision3::True);
    assert_eq!(above.le_threshold(&threshold), Decision3::False);
    assert_eq!(crossing.le_threshold(&threshold), Decision3::Indeterminate);
}

#[test]
fn linear_score_contains_exact_dot_product() {
    let profile = NumericProfile::new(
        "signed-score",
        1,
        16,
        22,
        BigRational::from_integer((-16).into()),
        BigRational::from_integer(16.into()),
        vec![BigRational::zero()],
        Some(32),
    )
    .unwrap();
    let bias_exact = BigRational::new((-1).into(), 10.into());
    let pairs = [
        (
            BigRational::new(1.into(), 3.into()),
            BigRational::new(3.into(), 2.into()),
        ),
        (
            BigRational::new((-2).into(), 5.into()),
            BigRational::new(7.into(), 4.into()),
        ),
    ];
    let bias = CertifiedDyadic::from_exact(profile.clone(), &bias_exact).unwrap();
    let terms = pairs
        .iter()
        .map(|(value, weight)| {
            Ok((
                CertifiedDyadic::from_exact(profile.clone(), value)?,
                CertifiedDyadic::from_exact(profile.clone(), weight)?,
            ))
        })
        .collect::<Result<Vec<_>, CbdError>>()
        .unwrap();
    let score = CertifiedDyadic::linear_score(&bias, &terms).unwrap();
    let exact = pairs
        .iter()
        .fold(bias_exact, |acc, (value, weight)| acc + value * weight);
    assert!(score.contains(&exact));
}

#[test]
fn profile_escalation_preserves_exact_input() {
    let exact = BigRational::new(1.into(), 10.into());
    let coarse = CertifiedDyadic::from_exact(unit_profile(16), &exact).unwrap();
    let fine = coarse.escalate_exact(&exact).unwrap();
    assert_eq!(fine.profile().fractional_bits(), 32);
    assert!(fine.contains(&exact));
    assert!(fine.upper() - fine.lower() < coarse.upper() - coarse.lower());
}

#[test]
fn fused_operation_quantizes_once_and_contains_exact_result() {
    let profile = unit_profile(16);
    let x_exact = BigRational::new(1.into(), 10.into());
    let x = CertifiedDyadic::from_exact(profile.clone(), &x_exact).unwrap();
    let one = CertifiedDyadic::from_exact(profile.clone(), &BigRational::one()).unwrap();
    let r = CertifiedDyadic::from_exact(profile, &BigRational::new(7.into(), 8.into())).unwrap();
    let complement = one.sub(&x).unwrap();
    let zero = CertifiedDyadic::from_exact(r.profile().clone(), &BigRational::zero()).unwrap();
    let product = x.fma(&complement, &zero).unwrap();
    let result = product.mul(&r).unwrap();
    let exact = BigRational::new(7.into(), 8.into()) * &x_exact * (BigRational::one() - x_exact);
    assert!(result.contains(&exact));
}

#[test]
fn logistic_fused_contains_exact_step_and_is_no_wider_than_sequential() {
    let x_exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let x = CertifiedDyadic::from_exact(unit_profile(16), &x_exact).unwrap();
    let fused = x.logistic_step_fused(&r).unwrap();
    let sequential = x.logistic_step_sequential(&r).unwrap();
    let exact = &r * &x_exact * (BigRational::one() - &x_exact);
    assert!(fused.contains(&exact));
    assert!(sequential.contains(&exact));
    assert!(fused.upper() - fused.lower() <= sequential.upper() - sequential.lower());
}

#[test]
fn logistic_lipschitz_variant_is_sound_and_deterministic() {
    let x_exact = BigRational::new(1.into(), 10.into());
    let r = BigRational::new(37.into(), 10.into());
    let x = CertifiedDyadic::from_exact(unit_profile(32), &x_exact).unwrap();
    let output = x.logistic_step_lipschitz(&r).unwrap();
    let exact = &r * &x_exact * (BigRational::one() - &x_exact);
    assert!(output.contains(&exact));
    assert_eq!(output.radius(), &BigUint::from(5_u8));
}

#[test]
fn logistic_local_bound_contains_exact_trajectory() {
    let r = BigRational::new(37.into(), 10.into());
    let mut exact = BigRational::new(1.into(), 10.into());
    let mut state =
        CertifiedDyadic::from_exact(NumericProfile::bounded_unit(32).unwrap(), &exact).unwrap();
    // A fração racional exata dobra aproximadamente o número de bits a cada
    // iteração; 12 passos cobrem composição sem tornar o teste exponencial.
    for _ in 0..12 {
        exact = &r * &exact * (BigRational::one() - &exact);
        state = state.logistic_step_local_bound(&r).unwrap();
        assert!(state.contains(&exact));
    }
}

#[test]
fn logistic_sequential_local_bound_contains_exact_trajectory() {
    let r = BigRational::new(37.into(), 10.into());
    let mut exact = BigRational::new(1.into(), 10.into());
    let mut state =
        CertifiedDyadic::from_exact(NumericProfile::bounded_unit(32).unwrap(), &exact).unwrap();
    for _ in 0..12 {
        exact = &r * &exact * (BigRational::one() - &exact);
        state = state.logistic_step_sequential_local_bound(&r).unwrap();
        assert!(state.contains(&exact));
    }
}

#[test]
fn fused_local_bound_is_no_wider_than_sequential_local_bound() {
    let r = BigRational::new(37.into(), 10.into());
    let exact = BigRational::new(1.into(), 10.into());
    for bits in [16, 32, 48, 64] {
        let input = CertifiedDyadic::from_exact(unit_profile(bits), &exact).unwrap();
        let fused = input.logistic_step_local_bound(&r).unwrap();
        let sequential = input.logistic_step_sequential_local_bound(&r).unwrap();
        assert!(fused.contains(&(&r * &exact * (BigRational::one() - &exact))));
        assert!(sequential.contains(&(&r * &exact * (BigRational::one() - &exact))));
        assert!(fused.upper() - fused.lower() <= sequential.upper() - sequential.lower());
    }
}

#[test]
fn fusion_is_not_pointwise_narrower_after_rounding_and_clipping() {
    let exact = BigRational::new(21_508.into(), 1_000_000_u32.into());
    let r = BigRational::new(1.into(), 10.into());
    let input = CertifiedDyadic::from_exact(unit_profile(16), &exact).unwrap();
    let fused = input.logistic_step_local_bound(&r).unwrap();
    let sequential = input.logistic_step_sequential_local_bound(&r).unwrap();
    assert!(fused.radius() > sequential.radius());
    assert!(fused.upper() - fused.lower() > sequential.upper() - sequential.lower());
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        rng_seed: RngSeed::Fixed(0x4342_4459_4144_2026),
        .. ProptestConfig::default()
    })]

    #[test]
    fn encoding_contains_every_unit_rational(numerator in 0_u32..=1_000_000) {
        let exact = BigRational::new(numerator.into(), 1_000_000_u32.into());
        let encoded = CertifiedDyadic::from_exact(unit_profile(16), &exact).unwrap();
        prop_assert!(encoded.contains(&exact));
        prop_assert!(encoded.is_canonical_for_interval(&exact, &exact));
    }

    #[test]
    fn add_sub_mul_contain_exact_results(
        left in 0_u16..=u16::MAX,
        right in 0_u16..=u16::MAX
    ) {
        let profile = unit_profile(16);
        let left_exact = BigRational::new(left.into(), u16::MAX.into());
        let right_exact = BigRational::new(right.into(), u16::MAX.into());
        let a = CertifiedDyadic::from_exact(profile.clone(), &left_exact).unwrap();
        let b = CertifiedDyadic::from_exact(profile, &right_exact).unwrap();

        let product = a.mul(&b).unwrap();
        prop_assert!(product.contains(&(left_exact.clone() * right_exact.clone())));

        if left_exact.clone() + right_exact.clone() <= BigRational::one() {
            let sum = a.add(&b).unwrap();
            prop_assert!(sum.contains(&(left_exact.clone() + right_exact.clone())));
        }

        if left_exact >= right_exact {
            let difference = a.sub(&b).unwrap();
            prop_assert!(difference.contains(&(left_exact - right_exact)));
        }
    }

    #[test]
    fn logistic_fused_contains_exact_result(
        numerator in 0_u32..=1_000_000,
        r_tenths in 0_u8..=40
    ) {
        let exact_x = BigRational::new(numerator.into(), 1_000_000_u32.into());
        let r = BigRational::new(r_tenths.into(), 10.into());
        let encoded = CertifiedDyadic::from_exact(unit_profile(16), &exact_x).unwrap();
        let result = encoded.logistic_step_fused(&r).unwrap();
        let exact_result = &r * &exact_x * (BigRational::one() - &exact_x);
        prop_assert!(result.contains(&exact_result));
    }

    #[test]
    fn logistic_sequential_local_contains_exact_result(
        numerator in 0_u32..=1_000_000,
        r_tenths in 0_u8..=40
    ) {
        let exact_x = BigRational::new(numerator.into(), 1_000_000_u32.into());
        let r = BigRational::new(r_tenths.into(), 10.into());
        let encoded = CertifiedDyadic::from_exact(unit_profile(16), &exact_x).unwrap();
        let result = encoded.logistic_step_sequential_local_bound(&r).unwrap();
        let fused = encoded.logistic_step_local_bound(&r).unwrap();
        let exact_result = &r * &exact_x * (BigRational::one() - &exact_x);
        prop_assert!(result.contains(&exact_result));
        prop_assert!(fused.contains(&exact_result));
    }
}
