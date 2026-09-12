//! Witness generation for the audited rational relation. The verifier lives in
//! numeric-core and does not call this generator, MPFR or the Legendre solver.
use crate::{gauss_legendre_relation, prepare_gauss_legendre};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};
use numeric_core::{
    CertificateError, FixedPointFormat, FunctionId, SigmoidCertificate, SigmoidRelationContract,
};

pub fn prepare_sigmoid_contract(
    format: FixedPointFormat,
    order: usize,
) -> Result<SigmoidRelationContract, CertificateError> {
    format.validate()?;
    if format.fractional_bits > 120 || format.integer_bits > 120 {
        return Err(CertificateError::Invalid("unsupported certificate width"));
    }
    if ![2, 4, 8, 16].contains(&order) {
        return Err(CertificateError::Invalid("order outside frozen ladder"));
    }
    let prepared = prepare_gauss_legendre(FunctionId::Sigmoid, order)
        .map_err(|_| CertificateError::Invalid("node generation failed"))?;
    let nodes = prepared
        .nodes
        .iter()
        .map(|x| format.encode(x).map(|x| x.value.decode()))
        .collect::<Result<Vec<_>, _>>()?;
    let weights = prepared
        .weights
        .iter()
        .map(|x| format.encode(x).map(|x| x.value.decode()))
        .collect::<Result<Vec<_>, _>>()?;
    let contract = SigmoidRelationContract {
        version: 1,
        nodes,
        weights,
        moment_degree: (2 * format.fractional_bits + 8).min(128),
        term_slack: format.ulp() * BigInt::from(2),
        sum_slack: format.ulp() * BigInt::from(16),
        format,
    };
    contract.validate()?;
    Ok(contract)
}

pub fn generate_sigmoid_certificate(
    contract: &SigmoidRelationContract,
    input: &BigRational,
) -> Result<SigmoidCertificate, CertificateError> {
    contract.validate()?;
    let xq = contract.format.encode(input)?.value.decode();
    let relation = |y: &BigRational| {
        gauss_legendre_relation(
            FunctionId::Sigmoid,
            &xq,
            y,
            &contract.nodes,
            &contract.weights,
        )
        .map_err(|_| CertificateError::Invalid("invalid implicit relation"))
    };
    let mut lower = BigRational::new(1.into(), 4.into());
    let mut upper = BigRational::new(3.into(), 4.into());
    if relation(&lower)? > BigRational::zero() || relation(&upper)? < BigRational::zero() {
        return Err(CertificateError::Invalid(
            "implicit solution is outside output domain",
        ));
    }
    let mut y = (&lower + &upper) / BigInt::from(2);
    for _ in 0..usize::from(contract.format.fractional_bits) + 40 {
        y = (&lower + &upper) / BigInt::from(2);
        let residual = relation(&y)?;
        if residual.is_zero() {
            break;
        }
        if residual < BigRational::zero() {
            lower = y.clone();
        } else {
            upper = y.clone();
        }
    }
    let output = contract.format.encode(&y)?.value.decode();
    let a = &output * BigInt::from(2) - BigRational::one();
    let terms = contract
        .nodes
        .iter()
        .map(|r| {
            contract
                .format
                .encode(&(&a / (&a * r + BigRational::one())))
                .map(|x| x.value.decode())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut certificate = SigmoidCertificate {
        contract_hash: contract.hash_hex(),
        canonical_input: input.clone(),
        output,
        terms,
        claimed_radius: BigRational::one(),
    };
    let (_, breakdown) = contract.check(input, &certificate)?;
    let scaled = breakdown.required_radius * contract.format.scale();
    let radius = (scaled.numer() + scaled.denom() - BigInt::one()) / scaled.denom();
    certificate.claimed_radius = BigRational::new(radius, contract.format.scale());
    contract.check(input, &certificate)?;
    Ok(certificate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeric_core::{CheckedEnclosure, Decision3, OverflowMode, QuantizationRounding};
    use numeric_reference::evaluate_transcendental;
    fn format(bits: u16) -> FixedPointFormat {
        FixedPointFormat {
            integer_bits: 8,
            fractional_bits: bits,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        }
    }
    #[test]
    fn checked_sigmoid_and_composition_enclose_directed_reference() {
        for bits in [8, 16] {
            let contract = prepare_sigmoid_contract(format(bits), 4).unwrap();
            let mut aggregate = CheckedEnclosure::exact(BigRational::zero());
            let mut reference_sum = BigRational::zero();
            for i in [-4, -1, 1, 4] {
                let x = BigRational::new(i.into(), 4.into());
                let certificate = generate_sigmoid_certificate(&contract, &x).unwrap();
                let (checked, _) = contract.check(&x, &certificate).unwrap();
                let reference = evaluate_transcendental(FunctionId::Sigmoid, &x, 512).unwrap();
                assert!(checked.contains(&reference.lower_rational()));
                assert!(checked.contains(&reference.upper_rational()));
                aggregate = aggregate.add(&checked);
                reference_sum += reference.nearest_rational();
                for tau in [
                    BigRational::new(2.into(), 5.into()),
                    BigRational::new(3.into(), 5.into()),
                ] {
                    match checked.decision(&tau) {
                        Decision3::True => assert!(reference.lower_rational() >= tau),
                        Decision3::False => assert!(reference.upper_rational() < tau),
                        Decision3::Indeterminate => {}
                    }
                }
            }
            let aggregate = aggregate.scale(&BigRational::new(1.into(), 4.into()));
            assert!(aggregate.contains(&(reference_sum / BigInt::from(4))));
        }
    }
    #[test]
    fn reduced_witness_enumeration_covers_malicious_terms() {
        let mut contract = prepare_sigmoid_contract(format(4), 2).unwrap();
        contract.term_slack = BigRational::new(1.into(), 16.into());
        contract.sum_slack = BigRational::new(1.into(), 8.into());
        let x = BigRational::zero();
        let mut accepted = 0;
        // All output grid points in [1/4,3/4], all two-term grid witnesses
        // in [-1,1]. Wider terms cannot satisfy this contract for these inputs.
        for raw_y in 4..=12 {
            for h0 in -16..=16 {
                for h1 in -16..=16 {
                    let candidate = SigmoidCertificate {
                        contract_hash: contract.hash_hex(),
                        canonical_input: x.clone(),
                        output: BigRational::new(raw_y.into(), 16.into()),
                        terms: vec![
                            BigRational::new(h0.into(), 16.into()),
                            BigRational::new(h1.into(), 16.into()),
                        ],
                        claimed_radius: BigRational::one(),
                    };
                    if let Ok((_, bound)) = contract.check(&x, &candidate) {
                        accepted += 1;
                        let scaled = bound.required_radius * contract.format.scale();
                        let raw =
                            (scaled.numer() + scaled.denom() - BigInt::one()) / scaled.denom();
                        let candidate = SigmoidCertificate {
                            claimed_radius: BigRational::new(raw, contract.format.scale()),
                            ..candidate
                        };
                        let (enclosure, _) = contract.check(&x, &candidate).unwrap();
                        assert!(enclosure.contains(&BigRational::new(1.into(), 2.into())));
                    }
                }
            }
        }
        assert!(accepted > 1);
    }
}
