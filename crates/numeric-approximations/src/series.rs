use crate::ApproximationError;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};
use numeric_core::FunctionId;

pub fn taylor_coefficients(
    function: FunctionId,
    degree: usize,
) -> Result<Vec<BigRational>, ApproximationError> {
    if degree == 0 {
        return Err(ApproximationError::InvalidOrder);
    }
    match function {
        FunctionId::Exp => {
            let mut coefficients = Vec::with_capacity(degree + 1);
            let mut factorial = BigInt::one();
            for index in 0..=degree {
                if index > 0 {
                    factorial *= index;
                }
                coefficients.push(BigRational::new(BigInt::one(), factorial.clone()));
            }
            Ok(coefficients)
        }
        FunctionId::Sigmoid => {
            // sigma' = sigma(1-sigma), c0=1/2.
            let mut coefficients = vec![BigRational::new(1.into(), 2.into())];
            for n in 0..degree {
                let convolution = (0..=n).fold(BigRational::zero(), |accumulator, index| {
                    accumulator + &coefficients[index] * &coefficients[n - index]
                });
                let next = (&coefficients[n] - convolution)
                    / BigRational::from_integer(BigInt::from(n + 1));
                coefficients.push(next);
            }
            Ok(coefficients)
        }
        _ => Err(ApproximationError::UnsupportedFunction),
    }
}

pub fn pade_diagonal_coefficients(
    function: FunctionId,
    order: usize,
) -> Result<(Vec<BigRational>, Vec<BigRational>), ApproximationError> {
    if order == 0 {
        return Err(ApproximationError::InvalidOrder);
    }
    let series = taylor_coefficients(function, 2 * order)?;
    let mut matrix = vec![vec![BigRational::zero(); order]; order];
    let mut rhs = vec![BigRational::zero(); order];
    for row in 0..order {
        let k = order + 1 + row;
        rhs[row] = -series[k].clone();
        for column in 0..order {
            let j = column + 1;
            matrix[row][column] = series[k - j].clone();
        }
    }
    let solved = solve_linear_system(matrix, rhs)?;
    let mut denominator = Vec::with_capacity(order + 1);
    denominator.push(BigRational::one());
    denominator.extend(solved);
    let mut numerator = Vec::with_capacity(order + 1);
    for k in 0..=order {
        let coefficient = (0..=k).fold(BigRational::zero(), |accumulator, j| {
            accumulator + &denominator[j] * &series[k - j]
        });
        numerator.push(coefficient);
    }
    Ok((numerator, denominator))
}

fn solve_linear_system(
    mut matrix: Vec<Vec<BigRational>>,
    mut rhs: Vec<BigRational>,
) -> Result<Vec<BigRational>, ApproximationError> {
    let size = rhs.len();
    for pivot in 0..size {
        let selected = (pivot..size)
            .find(|row| !matrix[*row][pivot].is_zero())
            .ok_or(ApproximationError::SingularPadeSystem)?;
        if selected != pivot {
            matrix.swap(selected, pivot);
            rhs.swap(selected, pivot);
        }
        let divisor = matrix[pivot][pivot].clone();
        for column in pivot..size {
            matrix[pivot][column] /= &divisor;
        }
        rhs[pivot] /= divisor;
        for row in 0..size {
            if row == pivot {
                continue;
            }
            let factor = matrix[row][pivot].clone();
            if factor.is_zero() {
                continue;
            }
            for column in pivot..size {
                let product = &factor * &matrix[pivot][column];
                matrix[row][column] -= product;
            }
            let product = factor * &rhs[pivot];
            rhs[row] -= product;
        }
    }
    Ok(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeric_core::{FixedPointFormat, OverflowMode, QuantizationRounding};

    #[test]
    fn sigmoid_degree_nine_matches_known_sparse_series() {
        let coefficients = taylor_coefficients(FunctionId::Sigmoid, 9).unwrap();
        assert_eq!(coefficients[0], BigRational::new(1.into(), 2.into()));
        assert_eq!(coefficients[1], BigRational::new(1.into(), 4.into()));
        assert_eq!(coefficients[2], BigRational::zero());
        assert_eq!(coefficients[3], BigRational::new((-1).into(), 48.into()));
        assert_eq!(coefficients[5], BigRational::new(1.into(), 480.into()));
        assert_eq!(
            coefficients[7],
            BigRational::new((-17).into(), 80640.into())
        );
        assert_eq!(coefficients[9], BigRational::new(31.into(), 1451520.into()));
    }

    #[test]
    fn exp_pade_two_two_is_the_classical_approximant() {
        let (p, q) = pade_diagonal_coefficients(FunctionId::Exp, 2).unwrap();
        assert_eq!(
            p,
            vec![
                BigRational::one(),
                BigRational::new(1.into(), 2.into()),
                BigRational::new(1.into(), 12.into()),
            ]
        );
        assert_eq!(
            q,
            vec![
                BigRational::one(),
                BigRational::new((-1).into(), 2.into()),
                BigRational::new(1.into(), 12.into()),
            ]
        );
    }

    #[test]
    fn q16_sigmoid_pade_three_matches_official_artifact_constants() {
        let (p, q) = pade_diagonal_coefficients(FunctionId::Sigmoid, 3).unwrap();
        let format = FixedPointFormat {
            integer_bits: 16,
            fractional_bits: 16,
            signed: true,
            rounding: QuantizationRounding::NearestTiesToEven,
            overflow: OverflowMode::Reject,
        };
        let p_raw: Vec<BigInt> = p
            .iter()
            .map(|value| format.encode(value).unwrap().value.raw().clone())
            .collect();
        let q_raw: Vec<BigInt> = q
            .iter()
            .map(|value| format.encode(value).unwrap().value.raw().clone())
            .collect();
        assert_eq!(
            p_raw,
            vec![32768.into(), 16384.into(), 3277.into(), 273.into()]
        );
        assert_eq!(q_raw, vec![65536.into(), 0.into(), 6554.into(), 0.into()]);
    }
}
