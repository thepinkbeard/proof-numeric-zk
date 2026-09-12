//! Directed interval reference, independent of fixed-point witness generators.
use crate::float_to_big_rational;
use num_rational::BigRational;
use num_traits::{One, Zero};
use rug::{
    float::Round,
    ops::{AddAssignRound, MulAssignRound},
    Float, Integer, Rational,
};

fn convert(q: &BigRational, p: u32, round: Round) -> Float {
    Float::with_val_round(
        p,
        Rational::from((
            Integer::from_str_radix(&q.numer().to_string(), 10).unwrap(),
            Integer::from_str_radix(&q.denom().to_string(), 10).unwrap(),
        )),
        round,
    )
    .0
}

/// Input must be in [0,1], logistic r in [0,4]; affine a in [0,1), c in [0,1].
pub fn directed_dynamic_trajectory(
    x: &BigRational,
    a: &BigRational,
    c: Option<&BigRational>,
    steps: usize,
    bits: u32,
) -> Result<Vec<(BigRational, BigRational)>, &'static str> {
    if !(64..=16384).contains(&bits)
        || x < &BigRational::zero()
        || x > &BigRational::one()
        || a < &BigRational::zero()
        || a > &BigRational::from_integer(4.into())
        || c.is_some_and(|v| {
            a >= &BigRational::one() || v < &BigRational::zero() || v > &BigRational::one()
        })
    {
        return Err("invalid reference domain or precision");
    }
    let mut lo = convert(x, bits, Round::Down);
    let mut hi = convert(x, bits, Round::Up);
    let al = convert(a, bits, Round::Down);
    let ah = convert(a, bits, Round::Up);
    let mut out = Vec::with_capacity(steps + 1);
    for t in 0..=steps {
        out.push((float_to_big_rational(&lo), float_to_big_rational(&hi)));
        if t == steps {
            break;
        }
        if let Some(c) = c {
            lo.mul_assign_round(&al, Round::Down);
            lo.add_assign_round(convert(c, bits, Round::Down), Round::Down);
            hi.mul_assign_round(&ah, Round::Up);
            hi.add_assign_round(convert(c, bits, Round::Up), Round::Up);
        } else {
            let one_minus_hi = Float::with_val_round(bits, 1 - &hi, Round::Down).0;
            let one_minus_lo = Float::with_val_round(bits, 1 - &lo, Round::Up).0;
            lo.mul_assign_round(one_minus_hi, Round::Down);
            lo.mul_assign_round(&al, Round::Down);
            hi.mul_assign_round(one_minus_lo, Round::Up);
            hi.mul_assign_round(&ah, Round::Up);
            // Invariance of [0,1] for 0<=r<=4 is part of this reference contract.
            if lo < 0 {
                lo = Float::with_val(bits, 0);
            }
            if hi > 1 {
                hi = Float::with_val(bits, 1);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directed_encloses_exact_short_trajectories() {
        for r in [
            BigRational::new(39.into(), 10.into()),
            BigRational::from_integer(4.into()),
        ] {
            for j in 0..=16 {
                let x = BigRational::new(j.into(), 16.into());
                let exact = crate::logistic_trajectory(&x, &r, 8).unwrap();
                let intervals = directed_dynamic_trajectory(&x, &r, None, 8, 256).unwrap();
                for (q, (l, u)) in exact.iter().zip(intervals) {
                    assert!(l <= *q && *q <= u);
                }
            }
        }
    }
}
