use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::Signed;
use numeric_core::{CompensatedNumber, LimbNumber, NumericError, RoundingMode};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

type L16x2 = LimbNumber<16, 2>;

#[test]
fn endpoints_are_canonical() {
    assert_eq!(
        L16x2::zero().to_big_rational(),
        BigRational::from_integer(0.into())
    );
    assert_eq!(
        L16x2::one().to_big_rational(),
        BigRational::from_integer(1.into())
    );
    assert_eq!(L16x2::new([1, 0], true), Err(NumericError::NonCanonicalOne));
}

#[test]
fn exact_decimal_quantization_has_rational_error() {
    let tenth = BigRational::new(1.into(), 10.into());
    let result = L16x2::from_rational(&tenth, RoundingMode::NearestTiesToEven).unwrap();
    assert_eq!(result.exact_error, result.value.to_big_rational() - tenth);
}

#[test]
fn tie_modes_are_distinct() {
    type Tiny = LimbNumber<2, 1>;
    let one_eighth = BigRational::new(1.into(), 8.into());
    let even = Tiny::from_rational(&one_eighth, RoundingMode::NearestTiesToEven).unwrap();
    let away = Tiny::from_rational(&one_eighth, RoundingMode::NearestTiesAway).unwrap();
    assert_eq!(even.value, Tiny::zero());
    assert_eq!(away.value.limbs(), &[1]);
}

#[test]
fn convolution_matches_aggregate_product_at_boundaries() {
    let a = L16x2::new([u16::MAX as u64, u16::MAX as u64], false).unwrap();
    let b = L16x2::new([u16::MAX as u64, 1], false).unwrap();
    assert_eq!(a.convolution_product(&b), a.numerator() * b.numerator());
}

#[test]
fn ratio_multiplication_preserves_integer_relation() {
    let x = L16x2::from_rational(
        &BigRational::new(9.into(), 100.into()),
        RoundingMode::Truncate,
    )
    .unwrap()
    .value;
    let result = x
        .mul_ratio(&37_u8.into(), &10_u8.into(), RoundingMode::Truncate)
        .unwrap();
    assert_eq!(
        x.numerator() * BigInt::from(37).to_biguint().unwrap(),
        result.value.numerator() * &result.divisor + result.discarded
    );
    assert_eq!(result.divisor, 10_u8.into());
}

#[test]
fn compensated_semantics_and_bound_are_exact() {
    type Comp = CompensatedNumber<16, 2>;
    let input = BigRational::new(1.into(), 10.into());
    let encoded = Comp::from_rational(&input, RoundingMode::NearestTiesToEven).unwrap();
    assert_eq!(encoded.exact_error, encoded.value.to_big_rational() - input);
    let scale = BigInt::from(L16x2::scale());
    assert!(encoded.value.residual().abs() <= scale / 2);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        rng_seed: RngSeed::Fixed(0x4e4f_5641_2026_0629),
        .. ProptestConfig::default()
    })]

    #[test]
    fn encode_decode_obeys_truncation_bound(n in 0_u64..=u32::MAX as u64) {
        let x = BigRational::new(BigInt::from(n), BigInt::from(u32::MAX));
        let quantized = L16x2::from_rational(&x, RoundingMode::Truncate).unwrap();
        let error = -quantized.exact_error;
        prop_assert!(error >= BigRational::from_integer(0.into()));
        prop_assert!(error < BigRational::new(1.into(), BigInt::from(1_u64 << 32)));
    }

    #[test]
    fn convolution_is_exact(
        a0 in any::<u16>(), a1 in any::<u16>(),
        b0 in any::<u16>(), b1 in any::<u16>()
    ) {
        let a = L16x2::new([a0.into(), a1.into()], false).unwrap();
        let b = L16x2::new([b0.into(), b1.into()], false).unwrap();
        prop_assert_eq!(a.convolution_product(&b), a.numerator() * b.numerator());
    }

    #[test]
    fn multiplication_matches_stated_integer_relation(
        a0 in any::<u16>(), a1 in any::<u16>(),
        b0 in any::<u16>(), b1 in any::<u16>()
    ) {
        let a = L16x2::new([a0.into(), a1.into()], false).unwrap();
        let b = L16x2::new([b0.into(), b1.into()], false).unwrap();
        let result = a.mul(&b, RoundingMode::Truncate).unwrap();
        let scale = L16x2::scale();
        prop_assert_eq!(
            a.numerator() * b.numerator(),
            result.value.numerator() * scale + result.discarded
        );
    }
}
