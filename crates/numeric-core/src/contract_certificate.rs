//! Rational certificate checker. No MPFR, sampling, or evidence labels are
//! trusted here. The caller must bind the canonical inputs and contract to its
//! public statement; this module is not itself a cryptographic proof.
use crate::{Decision3, FixedPointFormat};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("invalid certificate: {0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Fixed(#[from] crate::FixedPointError),
}

/// Can be constructed only from exact values or checked arithmetic rules.
/// Deliberately has no Deserialize implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedEnclosure {
    lower: BigRational,
    upper: BigRational,
}

impl CheckedEnclosure {
    /// Every precision is derived from the same caller-supplied canonical value.
    pub fn quantized_input(
        value: &BigRational,
        format: &FixedPointFormat,
    ) -> Result<Self, CertificateError> {
        let center = format.encode(value)?.value.decode();
        let radius = format.absolute_quantization_bound();
        Ok(Self {
            lower: &center - &radius,
            upper: &center + &radius,
        })
    }

    /// Outward dyadic enclosure after an operation, with canonical RNE center
    /// according to the public format and upward-rounded nonnegative radius.
    pub fn regrid(&self, format: &FixedPointFormat) -> Result<Self, CertificateError> {
        let center = format
            .encode(&((&self.lower + &self.upper) / BigInt::from(2)))?
            .value
            .decode();
        let radius = (&center - &self.lower).max(&self.upper - &center);
        let scaled = radius * format.scale();
        let raw = (scaled.numer() + scaled.denom() - BigInt::one()) / scaled.denom();
        let radius = BigRational::new(raw, format.scale());
        // Both lanes must fit the declared format; reject rather than wrap.
        format.encode(&radius)?;
        Ok(Self {
            lower: &center - &radius,
            upper: &center + &radius,
        })
    }
    pub fn exact(value: BigRational) -> Self {
        Self {
            lower: value.clone(),
            upper: value,
        }
    }
    pub fn lower(&self) -> &BigRational {
        &self.lower
    }
    pub fn upper(&self) -> &BigRational {
        &self.upper
    }
    pub fn contains(&self, value: &BigRational) -> bool {
        self.lower <= *value && *value <= self.upper
    }
    pub fn decision(&self, threshold: &BigRational) -> Decision3 {
        if self.lower >= *threshold {
            Decision3::True
        } else if self.upper < *threshold {
            Decision3::False
        } else {
            Decision3::Indeterminate
        }
    }
    pub fn add(&self, rhs: &Self) -> Self {
        Self {
            lower: &self.lower + &rhs.lower,
            upper: &self.upper + &rhs.upper,
        }
    }
    pub fn sub(&self, rhs: &Self) -> Self {
        Self {
            lower: &self.lower - &rhs.upper,
            upper: &self.upper - &rhs.lower,
        }
    }
    pub fn scale(&self, coefficient: &BigRational) -> Self {
        let a = &self.lower * coefficient;
        let b = &self.upper * coefficient;
        Self {
            lower: a.clone().min(b.clone()),
            upper: a.max(b),
        }
    }
    pub fn square(&self) -> Self {
        let a = &self.lower * &self.lower;
        let b = &self.upper * &self.upper;
        let lower = if self.lower <= BigRational::zero() && self.upper >= BigRational::zero() {
            BigRational::zero()
        } else {
            a.clone().min(b.clone())
        };
        Self {
            lower,
            upper: a.max(b),
        }
    }
    /// A local additive perturbation is propagated through subsequent operations.
    /// This is useful for adversarial relation models, not just honest rounding.
    pub fn with_local_error(&self, epsilon: &BigRational) -> Result<Self, CertificateError> {
        require(!epsilon.is_negative(), "negative local error")?;
        Ok(Self {
            lower: &self.lower - epsilon,
            upper: &self.upper + epsilon,
        })
    }
}

/// Exact baseline on the same canonical coordinates; no square root or
/// quantization of intermediate products. Units are inherited from the caller.
pub fn exact_ecef_squared_distance(
    left: &[BigRational; 3],
    right: &[BigRational; 3],
) -> BigRational {
    left.iter()
        .zip(right)
        .map(|(a, b)| {
            let d = a - b;
            &d * &d
        })
        .sum()
}

/// Public contract for a NEW rational sigmoid relation, not a reconstruction
/// of the Sridhar circuit. Quantized nodes are exact rational constants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigmoidRelationContract {
    pub version: u16,
    pub format: FixedPointFormat,
    pub nodes: Vec<BigRational>,
    pub weights: Vec<BigRational>,
    pub moment_degree: u16,
    /// |(2*y*r_i-r_i+1)*h_i - (2*y-1)| <= term_slack
    pub term_slack: BigRational,
    /// |sum_i w_i*h_i - Q(x)| <= sum_slack (no rounded weighted sum).
    pub sum_slack: BigRational,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SigmoidCertificate {
    pub contract_hash: String,
    pub canonical_input: BigRational,
    pub output: BigRational,
    pub terms: Vec<BigRational>,
    pub claimed_radius: BigRational,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundBreakdown {
    pub approximation: BigRational,
    pub verification: BigRational,
    pub input: BigRational,
    pub required_radius: BigRational,
}

impl SigmoidRelationContract {
    pub fn hash_hex(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("serializable rational contract");
        format!("{:x}", Sha256::digest(bytes))
    }
    pub fn validate(&self) -> Result<(), CertificateError> {
        require(self.version == 1, "unknown relation version")?;
        self.format.validate()?;
        require(
            self.format.fractional_bits <= 120 && self.format.integer_bits <= 120,
            "unsupported certificate width",
        )?;
        require(
            !self.nodes.is_empty()
                && self.nodes.len() <= 32
                && self.nodes.len() == self.weights.len(),
            "invalid constants length",
        )?;
        require(self.moment_degree <= 128, "moment degree too large")?;
        require(
            !self.term_slack.is_negative() && !self.sum_slack.is_negative(),
            "negative slack",
        )?;
        for (node, weight) in self.nodes.iter().zip(&self.weights) {
            require(node.abs() <= BigRational::one(), "node outside [-1,1]")?;
            require(weight > &BigRational::zero(), "nonpositive weight")?;
            require_grid(&self.format, node)?;
            require_grid(&self.format, weight)?;
        }
        Ok(())
    }

    /// Bound integral_{-1}^1 a/(1+a*t) dt minus the quantized rule for
    /// |a|<=1/2. Moment defects include ALL node and weight errors. The
    /// remaining geometric series is bounded uniformly by
    /// (2+sum|w|) * b^(d+2)/(1-b). No Legendre-root oracle is trusted.
    pub fn quadrature_error_bound(&self) -> Result<BigRational, CertificateError> {
        self.validate()?;
        let b = ratio(1, 2);
        let mut power = b.clone();
        let mut node_powers = vec![BigRational::one(); self.nodes.len()];
        let mut error = BigRational::zero();
        for k in 0..=self.moment_degree {
            let integral_moment = if k % 2 == 0 {
                ratio(2, i64::from(k) + 1)
            } else {
                BigRational::zero()
            };
            let moment: BigRational = self
                .weights
                .iter()
                .zip(&node_powers)
                .map(|(w, p)| w * p)
                .sum();
            error += &power * (integral_moment - moment).abs();
            power *= &b;
            for (p, r) in node_powers.iter_mut().zip(&self.nodes) {
                *p *= r;
            }
        }
        let mass: BigRational = self.weights.iter().cloned().sum();
        Ok(error + (ratio(2, 1) + mass) * power / (BigRational::one() - b))
    }

    pub fn check(
        &self,
        expected_input: &BigRational,
        certificate: &SigmoidCertificate,
    ) -> Result<(CheckedEnclosure, BoundBreakdown), CertificateError> {
        self.check_with_bound(expected_input, certificate, &self.quadrature_error_bound()?)
    }

    /// Amortize only the public moment calculation. Every witness, input,
    /// domain, hash and claimed radius is still checked by the same kernel.
    /// The cached bound is private and cannot be supplied by an untrusted caller.
    pub fn check_batch(
        &self,
        expected_inputs: &[BigRational],
        certificates: &[SigmoidCertificate],
    ) -> Result<Vec<(CheckedEnclosure, BoundBreakdown)>, CertificateError> {
        require(
            expected_inputs.len() == certificates.len(),
            "batch length mismatch",
        )?;
        let bound = self.quadrature_error_bound()?;
        expected_inputs
            .iter()
            .zip(certificates)
            .map(|(x, c)| self.check_with_bound(x, c, &bound))
            .collect()
    }

    fn check_with_bound(
        &self,
        expected_input: &BigRational,
        certificate: &SigmoidCertificate,
        quadrature_bound: &BigRational,
    ) -> Result<(CheckedEnclosure, BoundBreakdown), CertificateError> {
        self.validate()?;
        require(
            certificate.contract_hash == self.hash_hex(),
            "contract hash mismatch",
        )?;
        require(
            certificate.canonical_input == *expected_input,
            "canonical input mismatch",
        )?;
        require(
            expected_input.abs() <= BigRational::one(),
            "input outside [-1,1]",
        )?;
        require(
            certificate.output >= ratio(1, 4) && certificate.output <= ratio(3, 4),
            "output outside [1/4,3/4]",
        )?;
        require(
            certificate.terms.len() == self.nodes.len(),
            "wrong witness length",
        )?;
        require_grid(&self.format, &certificate.output)?;
        require_grid(&self.format, &certificate.claimed_radius)?;
        let a = ratio(2, 1) * &certificate.output - BigRational::one();
        let mut sum = BigRational::zero();
        for ((r, w), h) in self.nodes.iter().zip(&self.weights).zip(&certificate.terms) {
            require_grid(&self.format, h)?;
            let denominator = &a * r + BigRational::one();
            require(denominator >= ratio(1, 2), "unsafe denominator")?;
            require(
                (&denominator * h - &a).abs() <= self.term_slack,
                "term relation failed",
            )?;
            sum += w * h;
        }
        let quantized = self.format.encode(expected_input)?;
        require(
            (sum - quantized.value.decode()).abs() <= self.sum_slack,
            "sum relation failed",
        )?;
        let mass: BigRational = self.weights.iter().cloned().sum();
        // logit's derivative is >=4 on (0,1): inverse conversion factor 1/4.
        let approximation = quadrature_bound / ratio(4, 1);
        let verification = (ratio(2, 1) * mass * &self.term_slack + &self.sum_slack) / ratio(4, 1);
        let input = quantized.exact_error.abs() / ratio(4, 1);
        let required_radius = &approximation + &verification + &input;
        require(
            certificate.claimed_radius >= required_radius,
            "understated radius",
        )?;
        let enclosure = CheckedEnclosure {
            lower: (&certificate.output - &certificate.claimed_radius).max(BigRational::zero()),
            upper: (&certificate.output + &certificate.claimed_radius).min(BigRational::one()),
        };
        Ok((
            enclosure,
            BoundBreakdown {
                approximation,
                verification,
                input,
                required_radius,
            },
        ))
    }
}

fn require_grid(format: &FixedPointFormat, value: &BigRational) -> Result<(), CertificateError> {
    require(
        format.encode(value)?.value.decode() == *value,
        "value is not on the declared grid",
    )
}
fn require(condition: bool, message: &'static str) -> Result<(), CertificateError> {
    if condition {
        Ok(())
    } else {
        Err(CertificateError::Invalid(message))
    }
}
fn ratio(a: i64, b: i64) -> BigRational {
    BigRational::new(BigInt::from(a), BigInt::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OverflowMode, QuantizationRounding};
    fn contract() -> SigmoidRelationContract {
        SigmoidRelationContract {
            version: 1,
            format: FixedPointFormat {
                integer_bits: 8,
                fractional_bits: 8,
                signed: true,
                rounding: QuantizationRounding::NearestTiesToEven,
                overflow: OverflowMode::Reject,
            },
            nodes: vec![ratio(-1, 2), ratio(1, 2)],
            weights: vec![ratio(1, 1), ratio(1, 1)],
            moment_degree: 8,
            term_slack: ratio(1, 128),
            sum_slack: ratio(1, 16),
        }
    }
    #[test]
    fn forged_claims_inputs_constants_and_witnesses_fail() {
        let c = contract();
        let good = SigmoidCertificate {
            contract_hash: c.hash_hex(),
            canonical_input: ratio(0, 1),
            output: ratio(1, 2),
            terms: vec![ratio(0, 1); 2],
            claimed_radius: ratio(1, 4),
        };
        assert!(c
            .check(&ratio(0, 1), &good)
            .unwrap()
            .0
            .contains(&ratio(1, 2)));
        let recovered: SigmoidCertificate =
            serde_json::from_slice(&serde_json::to_vec(&good).unwrap()).unwrap();
        let recovered_contract: SigmoidRelationContract =
            serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
        assert_eq!(c.hash_hex(), recovered_contract.hash_hex());
        assert!(recovered_contract.check(&ratio(0, 1), &recovered).is_ok());
        let mut wrong_precision = c.clone();
        wrong_precision.format.fractional_bits = 16;
        assert!(wrong_precision.check(&ratio(0, 1), &good).is_err());
        let mut bad = good.clone();
        bad.claimed_radius = ratio(0, 1);
        assert!(c.check(&ratio(0, 1), &bad).is_err());
        let mut bad = good.clone();
        bad.terms[0] = ratio(1, 1);
        assert!(c.check(&ratio(0, 1), &bad).is_err());
        let mut bad = good.clone();
        bad.output = ratio(1, 1);
        assert!(c.check(&ratio(0, 1), &bad).is_err());
        assert!(c.check(&ratio(1, 2), &good).is_err());
        let mut changed = c.clone();
        changed.nodes[0] = ratio(-1, 4);
        assert!(changed.check(&ratio(0, 1), &good).is_err());
        changed.weights[0] = ratio(-1, 1);
        assert!(changed.quadrature_error_bound().is_err());
    }
    #[test]
    fn batch_checker_preserves_checks_and_rejects_wrong_lengths() {
        let c = contract();
        let good = SigmoidCertificate {
            contract_hash: c.hash_hex(),
            canonical_input: ratio(0, 1),
            output: ratio(1, 2),
            terms: vec![ratio(0, 1); 2],
            claimed_radius: ratio(1, 4),
        };
        let one = c.check(&ratio(0, 1), &good).unwrap();
        let batch = c.check_batch(&[ratio(0, 1)], &[good.clone()]).unwrap();
        assert_eq!(batch[0].0, one.0);
        assert_eq!(batch[0].1.required_radius, one.1.required_radius);
        assert!(c.check_batch(&[], &[good.clone()]).is_err());
        assert!(c.check_batch(&[ratio(1, 2)], &[good.clone()]).is_err());
        let mut bad = good;
        bad.claimed_radius = ratio(0, 1);
        assert!(c.check_batch(&[ratio(0, 1)], &[bad]).is_err());
    }
    #[test]
    fn local_slack_is_amplified_by_composition() {
        let start = CheckedEnclosure::exact(ratio(0, 1));
        let result = start
            .with_local_error(&ratio(1, 1024))
            .unwrap()
            .scale(&ratio(1024, 1));
        assert_eq!(result.lower(), &ratio(-1, 1));
        assert_eq!(result.upper(), &ratio(1, 1));
        assert_eq!(result.decision(&ratio(1, 2)), Decision3::Indeterminate);
        assert!(start.with_local_error(&ratio(-1, 1)).is_err());
    }
    #[test]
    fn ecef_exact_baseline_preserves_boundary() {
        let left = [ratio(3, 1), ratio(4, 1), ratio(0, 1)];
        let right = [ratio(0, 1), ratio(0, 1), ratio(0, 1)];
        let d = exact_ecef_squared_distance(&left, &right);
        assert_eq!(d, ratio(25, 1));
        assert_eq!(
            CheckedEnclosure::exact(d).decision(&ratio(25, 1)),
            Decision3::True
        );
    }
}
