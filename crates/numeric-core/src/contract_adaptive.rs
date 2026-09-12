//! Selection policies over checked enclosures. Every evaluation (including
//! speculative work) is recorded. Proof execution belongs to the backend.
use crate::{CertificateError, CheckedEnclosure, Decision3};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::Signed;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const CONTRACT_PRECISION_LADDER: [u16; 4] = [16, 32, 48, 64];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ContractPrecisionPolicy {
    Fixed,
    Sequential,
    MarginDirected,
    PreviewAll,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContractEvaluation {
    pub precision_bits: u16,
    pub decision: Decision3,
    pub elapsed_ns: u128,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub struct CheckedAdaptiveDecision {
    pub enclosure: CheckedEnclosure,
    pub selected_precision: u16,
    pub evaluations: Vec<ContractEvaluation>,
    pub total_selection_ns: u128,
}

pub fn select_checked_precision(
    policy: ContractPrecisionPolicy,
    fixed_bits: u16,
    threshold: &BigRational,
    mut evaluate: impl FnMut(u16) -> Result<CheckedEnclosure, CertificateError>,
) -> Result<CheckedAdaptiveDecision, CertificateError> {
    if !CONTRACT_PRECISION_LADDER.contains(&fixed_bits) {
        return Err(CertificateError::Invalid(
            "fixed precision outside public ladder",
        ));
    }
    let started = Instant::now();
    let mut evaluations = Vec::new();
    let mut values = Vec::new();
    let mut index = if policy == ContractPrecisionPolicy::Fixed {
        CONTRACT_PRECISION_LADDER
            .iter()
            .position(|b| *b == fixed_bits)
            .unwrap()
    } else {
        0
    };
    loop {
        let bits = CONTRACT_PRECISION_LADDER[index];
        let tick = Instant::now();
        let enclosure = evaluate(bits)?;
        let elapsed_ns = tick.elapsed().as_nanos();
        let decision = enclosure.decision(threshold);
        evaluations.push(ContractEvaluation {
            precision_bits: bits,
            decision,
            elapsed_ns,
            selected: false,
        });
        values.push(enclosure.clone());
        if policy == ContractPrecisionPolicy::Fixed
            || index == 3
            || (policy != ContractPrecisionPolicy::PreviewAll
                && decision != Decision3::Indeterminate)
        {
            break;
        }
        let mut next = index + 1;
        if policy == ContractPrecisionPolicy::MarginDirected {
            let center = (enclosure.lower() + enclosure.upper()) / BigInt::from(2);
            let margin = (center - threshold).abs();
            let radius = (enclosure.upper() - enclosure.lower()) / BigInt::from(2);
            // This prediction is only a performance heuristic. The next
            // enclosure must be checked even if the prediction says stable.
            while next < 3
                && &radius / BigInt::from(1u64 << (CONTRACT_PRECISION_LADDER[next] - bits))
                    >= margin
            {
                next += 1;
            }
        }
        index = next;
    }
    let chosen = if policy == ContractPrecisionPolicy::PreviewAll {
        evaluations
            .iter()
            .position(|e| e.decision != Decision3::Indeterminate)
            .unwrap_or(evaluations.len() - 1)
    } else {
        evaluations.len() - 1
    };
    evaluations[chosen].selected = true;
    Ok(CheckedAdaptiveDecision {
        enclosure: values.swap_remove(chosen),
        selected_precision: evaluations[chosen].precision_bits,
        evaluations,
        total_selection_ns: started.elapsed().as_nanos(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_previews_are_counted_even_after_a_stable_low_precision() {
        let mut calls = Vec::new();
        let result = select_checked_precision(
            ContractPrecisionPolicy::PreviewAll,
            16,
            &BigRational::from_integer(0.into()),
            |bits| {
                calls.push(bits);
                Ok(CheckedEnclosure::exact(BigRational::from_integer(1.into())))
            },
        )
        .unwrap();
        assert_eq!(calls, CONTRACT_PRECISION_LADDER);
        assert_eq!(result.selected_precision, 16);
        assert_eq!(result.evaluations.len(), 4);
        assert_eq!(result.evaluations.iter().filter(|e| e.selected).count(), 1);
    }
    #[test]
    fn exhausted_prediction_never_authorizes_a_decision() {
        let result = select_checked_precision(
            ContractPrecisionPolicy::MarginDirected,
            16,
            &BigRational::from_integer(0.into()),
            |_| {
                CheckedEnclosure::exact(BigRational::from_integer(0.into()))
                    .with_local_error(&BigRational::from_integer(1.into()))
            },
        )
        .unwrap();
        assert_eq!(result.selected_precision, 64);
        assert_eq!(
            result
                .enclosure
                .decision(&BigRational::from_integer(0.into())),
            Decision3::Indeterminate
        );
        assert_eq!(result.evaluations.len(), 2);
    }
}
