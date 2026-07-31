//! Relação R1CS para uma etapa logística em Certified Floating Dyadic.
//!
//! O circuito usa um denominador diádico comum determinado pelo menor
//! expoente do corredor. Ele não exige que o centro seja o único RNE possível;
//! exige que o intervalo de saída contenha a imagem do intervalo de entrada.

use ff::PrimeField;
use nova_snark::{
    frontend::{
        num::AllocatedNum, AllocatedBit, ConstraintSystem, LinearCombination, SynthesisError,
    },
    traits::circuit::StepCircuit,
};
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};
use numeric_core::{CertifiedFloat, CfdError, CfdProfile, Decision3};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CfdTopology {
    Sequential,
    Fused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfdCircuitValue {
    pub exponent_lane: u64,
    pub significand: u128,
    pub radius: u128,
    pub zero: bool,
    pub wide: bool,
}

impl CfdCircuitValue {
    pub fn from_certified(value: &CertifiedFloat) -> Result<Self, CfdError> {
        Ok(Self {
            exponent_lane: value.exponent_lane() as u64,
            significand: value
                .significand()
                .to_u128()
                .ok_or(CfdError::NonCanonical)?,
            radius: value.radius().to_u128().ok_or(CfdError::NonCanonical)?,
            zero: value.is_zero(),
            wide: value.is_wide(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfdLogisticStepWitness {
    pub input: CfdCircuitValue,
    pub intermediate: Option<CfdCircuitValue>,
    pub output: CfdCircuitValue,
    pub decision: Decision3,
}

impl CfdLogisticStepWitness {
    pub fn compute(
        profile: &CfdProfile,
        input: &CertifiedFloat,
        r_num: u64,
        r_den: u64,
        topology: CfdTopology,
    ) -> Result<Self, CfdError> {
        Self::compute_with_threshold(
            profile,
            input,
            r_num,
            r_den,
            topology,
            common_denominator(profile) / 2,
        )
    }

    /// Variante do protocolo de Pareto: o limiar é público e já convertido
    /// para o numerador da escala diádica comum do perfil.
    pub fn compute_with_threshold(
        profile: &CfdProfile,
        input: &CertifiedFloat,
        r_num: u64,
        r_den: u64,
        topology: CfdTopology,
        threshold_common: u128,
    ) -> Result<Self, CfdError> {
        assert!(r_den > 0);
        let r = BigRational::new(r_num.into(), r_den.into());
        let (intermediate, output) = match topology {
            CfdTopology::Sequential => {
                let product = input.logistic_product_sequential()?;
                let output = match product.mul_public(&r) {
                    Ok(value) => value,
                    Err(CfdError::OutsideDomain) => CertifiedFloat::wide(profile.clone()),
                    Err(error) => return Err(error),
                };
                (Some(product), output)
            }
            CfdTopology::Fused => (None, input.logistic_step_fused(&r)?),
        };
        let mut input_value = CfdCircuitValue::from_certified(input)?;
        let mut intermediate_value = intermediate
            .as_ref()
            .map(CfdCircuitValue::from_certified)
            .transpose()?;
        let mut output_value = CfdCircuitValue::from_certified(&output)?;

        if !fits_unclipped_unit(profile, &input_value) {
            input_value = wide_value();
        }
        if intermediate_value
            .as_ref()
            .is_some_and(|value| !fits_unclipped_unit(profile, value))
        {
            intermediate_value = Some(wide_value());
            output_value = wide_value();
        }
        if !fits_unclipped_unit(profile, &output_value) {
            output_value = wide_value();
        }
        if input_value.wide {
            output_value = wide_value();
            if topology == CfdTopology::Sequential {
                intermediate_value = Some(wide_value());
            }
        }
        if !input_value.wide {
            match topology {
                CfdTopology::Sequential => {
                    let intermediate_value = intermediate_value
                        .as_mut()
                        .expect("a topologia sequencial possui intermediário");
                    let q = common_denominator(profile);
                    let input_center = common_center(profile, &input_value);
                    let input_radius = common_radius(profile, &input_value);
                    let intermediate_center = common_center(profile, intermediate_value);
                    let product_raw = input_center * (q - input_center);
                    let product_rhs = product_raw.abs_diff(q * intermediate_center)
                        + input_radius * q.abs_diff(2 * input_center)
                        + input_radius * input_radius;
                    if !raise_common_radius(profile, intermediate_value, ceil_div(product_rhs, q)) {
                        *intermediate_value = wide_value();
                        output_value = wide_value();
                    } else {
                        let intermediate_center = common_center(profile, intermediate_value);
                        let intermediate_radius = common_radius(profile, intermediate_value);
                        let output_center = common_center(profile, &output_value);
                        let scale_rhs = (r_num as u128 * intermediate_center)
                            .abs_diff(r_den as u128 * output_center)
                            + r_num as u128 * intermediate_radius;
                        if !raise_common_radius(
                            profile,
                            &mut output_value,
                            ceil_div(scale_rhs, r_den as u128),
                        ) {
                            output_value = wide_value();
                        }
                    }
                }
                CfdTopology::Fused => {
                    let q = common_denominator(profile);
                    let input_center = common_center(profile, &input_value);
                    let input_radius = common_radius(profile, &input_value);
                    let output_center = common_center(profile, &output_value);
                    let raw = r_num as u128 * input_center * (q - input_center);
                    let target = r_den as u128 * q * output_center;
                    let rhs = raw.abs_diff(target)
                        + r_num as u128
                            * (input_radius * q.abs_diff(2 * input_center)
                                + input_radius * input_radius);
                    if !raise_common_radius(
                        profile,
                        &mut output_value,
                        ceil_div(rhs, r_den as u128 * q),
                    ) {
                        output_value = wide_value();
                    }
                }
            }
        }
        let decision = decision_at_common_threshold(profile, &output_value, threshold_common);
        Ok(Self {
            input: input_value,
            intermediate: intermediate_value,
            output: output_value,
            decision,
        })
    }
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    numerator / denominator + u128::from(numerator % denominator != 0)
}

fn raise_common_radius(
    profile: &CfdProfile,
    value: &mut CfdCircuitValue,
    required_common: u128,
) -> bool {
    if value.wide {
        return true;
    }
    let shift = if value.zero {
        1
    } else {
        let minimum = profile.exponent_lanes()[0];
        let exponent = profile.exponent_lanes()[value.exponent_lane as usize];
        1_u128 << (exponent - minimum) as usize
    };
    let radius = ceil_div(required_common, shift);
    if radius >= (1_u128 << profile.radius_bits()) {
        return false;
    }
    value.radius = value.radius.max(radius);
    fits_unclipped_domain(profile, value)
}

fn decision_at_common_threshold(
    profile: &CfdProfile,
    value: &CfdCircuitValue,
    threshold: u128,
) -> Decision3 {
    if value.wide {
        return Decision3::Indeterminate;
    }
    let center = common_center(profile, value);
    let radius = common_radius(profile, value);
    if center.saturating_sub(radius) >= threshold {
        Decision3::True
    } else if center + radius < threshold {
        Decision3::False
    } else {
        Decision3::Indeterminate
    }
}

#[derive(Clone, Debug)]
pub struct CfdLogisticStepCircuit {
    pub profile: CfdProfile,
    pub witness: CfdLogisticStepWitness,
    pub r_num: u64,
    pub r_den: u64,
    pub topology: CfdTopology,
    /// Limiar público na escala diádica comum do perfil.
    pub threshold_common: u128,
}

impl CfdLogisticStepCircuit {
    pub const fn arity_value() -> usize {
        7 // iteration, exponent lane, significand, radius, zero, wide, decision
    }

    fn validate_profile(&self) {
        assert_eq!(self.profile.minimum(), &BigRational::from_integer(0.into()));
        assert_eq!(self.profile.maximum(), &BigRational::one());
        assert!(self.r_den > 0);
        assert!(self.profile.precision_bits() <= 63);
        assert!(self.profile.radius_bits() <= 63);
        let lanes = self.profile.exponent_lanes();
        assert!(!lanes.is_empty() && lanes.len() <= 32);
        assert!(lanes.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(lanes.iter().all(|exponent| *exponent <= 0));
        let span = i32::from(*lanes.last().expect("lane")) - i32::from(lanes[0]);
        assert!((0..=63).contains(&span));
        let q_bits = self.profile.precision_bits() - 1 + (-i32::from(lanes[0])) as usize;
        assert!(q_bits < 63);
        assert!(
            self.threshold_common > 0 && self.threshold_common <= common_denominator(&self.profile),
            "limiar CFD fora da escala comum do perfil"
        );
    }
}

/// Decisão que o circuito exigirá para um valor certificado.
///
/// Usada para montar `z0` sem divergir da relação constrained: o circuito
/// compara na escala diádica comum, e não pelo intervalo racional.
pub fn circuit_decision(
    profile: &CfdProfile,
    value: &CertifiedFloat,
    threshold_common: u128,
) -> Result<Decision3, CfdError> {
    let circuit_value = CfdCircuitValue::from_certified(value)?;
    Ok(decision_at_common_threshold(
        profile,
        &circuit_value,
        threshold_common,
    ))
}

/// Converte um limiar público racional para o numerador da escala diádica
/// comum do perfil. Retorna `None` quando o limiar não é representável nela.
pub fn common_threshold(profile: &CfdProfile, threshold: &BigRational) -> Option<u128> {
    let scaled = threshold * BigRational::from_integer(common_denominator(profile).into());
    if !scaled.is_integer() {
        return None;
    }
    scaled.to_integer().to_u128()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfdStableStepWitness {
    pub input: CfdCircuitValue,
    pub output: CfdCircuitValue,
    pub decision: Decision3,
}

impl CfdStableStepWitness {
    pub fn compute(
        profile: &CfdProfile,
        input: &CertifiedFloat,
        alpha_num: u64,
        alpha_den: u64,
        innovation_num: u64,
        innovation_den: u64,
    ) -> Result<Self, CfdError> {
        Self::compute_with_threshold(
            profile,
            input,
            alpha_num,
            alpha_den,
            innovation_num,
            innovation_den,
            common_denominator(profile),
        )
    }

    /// Variante do protocolo de Pareto: o limiar é público e já convertido
    /// para o numerador da escala diádica comum do perfil.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_with_threshold(
        profile: &CfdProfile,
        input: &CertifiedFloat,
        alpha_num: u64,
        alpha_den: u64,
        innovation_num: u64,
        innovation_den: u64,
        threshold_common: u128,
    ) -> Result<Self, CfdError> {
        assert!(alpha_den > 0 && innovation_den > 0);
        let alpha = BigRational::new(alpha_num.into(), alpha_den.into());
        let innovation_exact = BigRational::new(innovation_num.into(), innovation_den.into());
        let innovation = CertifiedFloat::from_exact(profile.clone(), &innovation_exact)?;
        let output = input.stable_affine_step(&alpha, &innovation)?;
        let mut input_value = CfdCircuitValue::from_certified(input)?;
        let mut output_value = CfdCircuitValue::from_certified(&output)?;
        if !fits_unclipped_domain(profile, &input_value) {
            input_value = wide_value();
        }
        if !fits_unclipped_domain(profile, &output_value) || input_value.wide {
            output_value = wide_value();
        }
        let decision = decision_at_common_threshold(profile, &output_value, threshold_common);
        Ok(Self {
            input: input_value,
            output: output_value,
            decision,
        })
    }
}

#[derive(Clone, Debug)]
pub struct CfdStableStepCircuit {
    pub profile: CfdProfile,
    pub witness: CfdStableStepWitness,
    pub alpha_num: u64,
    pub alpha_den: u64,
    pub innovation_num: u64,
    pub innovation_den: u64,
    /// Limiar público na escala diádica comum do perfil.
    pub threshold_common: u128,
}

impl CfdStableStepCircuit {
    pub const fn arity_value() -> usize {
        7
    }

    fn validate_profile(&self) {
        assert_eq!(self.profile.minimum(), &BigRational::from_integer(0.into()));
        assert_eq!(self.profile.maximum(), &BigRational::from_integer(2.into()));
        assert!(self.alpha_den > 0 && self.innovation_den > 0);
        assert!(self.profile.precision_bits() <= 63);
        assert!(self.profile.radius_bits() <= 63);
        let lanes = self.profile.exponent_lanes();
        assert!(!lanes.is_empty() && lanes.len() <= 32);
        assert!(lanes.windows(2).all(|pair| pair[0] < pair[1]));
        let span = i32::from(*lanes.last().expect("lane")) - i32::from(lanes[0]);
        assert!((0..=63).contains(&span));
        assert!(
            self.threshold_common > 0 && self.threshold_common <= domain_max_common(&self.profile),
            "limiar CFD stable fora da escala comum do perfil"
        );
    }
}

#[derive(Clone)]
struct AllocatedCfd<F: PrimeField> {
    common_center: AllocatedNum<F>,
    common_radius: AllocatedNum<F>,
    wide: AllocatedNum<F>,
    center_witness: u128,
    radius_witness: u128,
}

impl<F: PrimeField> StepCircuit<F> for CfdLogisticStepCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        self.validate_profile();
        assert_eq!(z.len(), Self::arity_value());
        let input = allocate_cfd(
            cs.namespace(|| "cfd_input"),
            &self.profile,
            [&z[1], &z[2], &z[3], &z[4], &z[5]],
            &self.witness.input,
        )?;
        enforce_decision(
            cs.namespace(|| "input_decision"),
            &self.profile,
            &input,
            &z[6],
            decision_from_field(z[6].get_value()),
            self.threshold_common,
        )?;

        let output_fields = allocate_output_fields(
            cs.namespace(|| "cfd_output_fields"),
            &self.witness.output,
            self.witness.decision,
        )?;
        let output = allocate_cfd(
            cs.namespace(|| "cfd_output"),
            &self.profile,
            [
                &output_fields[0],
                &output_fields[1],
                &output_fields[2],
                &output_fields[3],
                &output_fields[4],
            ],
            &self.witness.output,
        )?;
        enforce_absorbing_wide(
            cs.namespace(|| "input_wide_absorbing"),
            &input.wide,
            &output.wide,
        );

        match self.topology {
            CfdTopology::Fused => enforce_fused_enclosure(
                cs.namespace(|| "fused_enclosure"),
                &self.profile,
                &input,
                &output,
                self.r_num,
                self.r_den,
            )?,
            CfdTopology::Sequential => {
                let intermediate_witness = self.witness.intermediate.as_ref().ok_or_else(|| {
                    SynthesisError::Unsatisfiable("missing CFD intermediate".into())
                })?;
                let intermediate_fields = allocate_private_fields(
                    cs.namespace(|| "cfd_intermediate_fields"),
                    intermediate_witness,
                )?;
                let intermediate = allocate_cfd(
                    cs.namespace(|| "cfd_intermediate"),
                    &self.profile,
                    [
                        &intermediate_fields[0],
                        &intermediate_fields[1],
                        &intermediate_fields[2],
                        &intermediate_fields[3],
                        &intermediate_fields[4],
                    ],
                    intermediate_witness,
                )?;
                enforce_absorbing_wide(
                    cs.namespace(|| "input_to_intermediate_wide"),
                    &input.wide,
                    &intermediate.wide,
                );
                enforce_absorbing_wide(
                    cs.namespace(|| "intermediate_to_output_wide"),
                    &intermediate.wide,
                    &output.wide,
                );
                enforce_product_enclosure(
                    cs.namespace(|| "product_enclosure"),
                    &self.profile,
                    &input,
                    &intermediate,
                )?;
                enforce_public_scale_enclosure(
                    cs.namespace(|| "scale_enclosure"),
                    &self.profile,
                    &intermediate,
                    &output,
                    self.r_num,
                    self.r_den,
                )?;
            }
        }
        enforce_decision(
            cs.namespace(|| "output_decision"),
            &self.profile,
            &output,
            &output_fields[5],
            self.witness.decision,
            self.threshold_common,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );
        Ok(vec![
            next_iteration,
            output_fields[0].clone(),
            output_fields[1].clone(),
            output_fields[2].clone(),
            output_fields[3].clone(),
            output_fields[4].clone(),
            output_fields[5].clone(),
        ])
    }
}

impl<F: PrimeField> StepCircuit<F> for CfdStableStepCircuit {
    fn arity(&self) -> usize {
        Self::arity_value()
    }

    fn synthesize<CS: ConstraintSystem<F>>(
        &self,
        cs: &mut CS,
        z: &[AllocatedNum<F>],
    ) -> Result<Vec<AllocatedNum<F>>, SynthesisError> {
        self.validate_profile();
        assert_eq!(z.len(), Self::arity_value());
        let q = common_denominator(&self.profile);
        let input = allocate_cfd(
            cs.namespace(|| "stable_input"),
            &self.profile,
            [&z[1], &z[2], &z[3], &z[4], &z[5]],
            &self.witness.input,
        )?;
        enforce_decision(
            cs.namespace(|| "stable_input_decision"),
            &self.profile,
            &input,
            &z[6],
            decision_from_field(z[6].get_value()),
            self.threshold_common,
        )?;
        let output_fields = allocate_output_fields(
            cs.namespace(|| "stable_output_fields"),
            &self.witness.output,
            self.witness.decision,
        )?;
        let output = allocate_cfd(
            cs.namespace(|| "stable_output"),
            &self.profile,
            [
                &output_fields[0],
                &output_fields[1],
                &output_fields[2],
                &output_fields[3],
                &output_fields[4],
            ],
            &self.witness.output,
        )?;
        enforce_absorbing_wide(
            cs.namespace(|| "stable_wide_absorbing"),
            &input.wide,
            &output.wide,
        );

        let innovation_scaled = self.innovation_num as u128 * q;
        assert_eq!(innovation_scaled % self.innovation_den as u128, 0);
        let innovation_common = innovation_scaled / self.innovation_den as u128;
        let raw = self.alpha_num as u128 * input.center_witness
            + self.alpha_den as u128 * innovation_common;
        let target = self.alpha_den as u128 * output.center_witness;
        let center_error = allocate_abs_linear(
            cs.namespace(|| "stable_center_error"),
            raw as i128 - target as i128,
            |lc| {
                lc + (F::from(self.alpha_num), input.common_center.get_variable())
                    + (
                        field_from_u128::<F>(self.alpha_den as u128 * innovation_common),
                        CS::one(),
                    )
                    - (F::from(self.alpha_den), output.common_center.get_variable())
            },
            bit_length(4 * domain_max_common(&self.profile)),
        )?;
        let lhs = self.alpha_den as u128 * output.radius_witness;
        let rhs = raw.abs_diff(target) + self.alpha_num as u128 * input.radius_witness;
        enforce_inequality_sum(
            cs.namespace(|| "stable_enclosure"),
            &output.wide,
            lhs,
            bit_length(8 * domain_max_common(&self.profile)),
            |lc| {
                lc + (F::from(self.alpha_den), output.common_radius.get_variable())
                    - center_error.get_variable()
                    - (F::from(self.alpha_num), input.common_radius.get_variable())
            },
            rhs,
        )?;
        enforce_decision(
            cs.namespace(|| "stable_output_decision"),
            &self.profile,
            &output,
            &output_fields[5],
            self.witness.decision,
            self.threshold_common,
        )?;

        let next_iteration = AllocatedNum::alloc(cs.namespace(|| "stable_next_iteration"), || {
            z[0].get_value()
                .map(|value| value + F::ONE)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        cs.enforce(
            || "stable increment iteration",
            |lc| lc,
            |lc| lc,
            |lc| lc + z[0].get_variable() + CS::one() - next_iteration.get_variable(),
        );
        Ok(vec![
            next_iteration,
            output_fields[0].clone(),
            output_fields[1].clone(),
            output_fields[2].clone(),
            output_fields[3].clone(),
            output_fields[4].clone(),
            output_fields[5].clone(),
        ])
    }
}

fn allocate_output_fields<F, CS>(
    mut cs: CS,
    witness: &CfdCircuitValue,
    decision: Decision3,
) -> Result<Vec<AllocatedNum<F>>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let mut fields = allocate_private_fields(cs.namespace(|| "numeric_fields"), witness)?;
    fields.push(AllocatedNum::alloc(cs.namespace(|| "decision"), || {
        Ok(F::from(decision_code(decision)))
    })?);
    Ok(fields)
}

fn allocate_private_fields<F, CS>(
    mut cs: CS,
    witness: &CfdCircuitValue,
) -> Result<Vec<AllocatedNum<F>>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    [
        witness.exponent_lane as u128,
        witness.significand,
        witness.radius,
        u128::from(witness.zero),
        u128::from(witness.wide),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, value)| {
        AllocatedNum::alloc(cs.namespace(|| format!("field_{index}")), || {
            Ok(field_from_u128(value))
        })
    })
    .collect()
}

fn allocate_cfd<F, CS>(
    mut cs: CS,
    profile: &CfdProfile,
    fields: [&AllocatedNum<F>; 5],
    witness: &CfdCircuitValue,
) -> Result<AllocatedCfd<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let [lane, significand, radius, zero, wide] = fields;
    let lane_bits = bit_length((profile.exponent_lanes().len() - 1) as u128);
    range_check(
        cs.namespace(|| "lane_range"),
        lane,
        witness.exponent_lane as u128,
        lane_bits,
    )?;
    let significand_bits = range_check(
        cs.namespace(|| "significand_range"),
        significand,
        witness.significand,
        profile.precision_bits(),
    )?;
    range_check(
        cs.namespace(|| "radius_range"),
        radius,
        witness.radius,
        profile.radius_bits(),
    )?;
    enforce_boolean(cs.namespace(|| "zero_boolean"), zero);
    enforce_boolean(cs.namespace(|| "wide_boolean"), wide);
    cs.enforce(
        || "zero and wide exclusive",
        |lc| lc + zero.get_variable(),
        |lc| lc + wide.get_variable(),
        |lc| lc,
    );
    let normal_witness = !witness.zero && !witness.wide;
    let normal = allocate_boolean_num(cs.namespace(|| "normal"), normal_witness)?;
    cs.enforce(
        || "normal = 1-zero-wide",
        |lc| lc,
        |lc| lc,
        |lc| lc + CS::one() - zero.get_variable() - wide.get_variable() - normal.get_variable(),
    );
    let inactive = AllocatedNum::alloc(cs.namespace(|| "inactive"), || {
        Ok(if witness.zero || witness.wide {
            F::ONE
        } else {
            F::ZERO
        })
    })?;
    cs.enforce(
        || "inactive = zero+wide",
        |lc| lc,
        |lc| lc,
        |lc| lc + zero.get_variable() + wide.get_variable() - inactive.get_variable(),
    );
    cs.enforce(
        || "inactive significand is zero",
        |lc| lc + inactive.get_variable(),
        |lc| lc + significand.get_variable(),
        |lc| lc,
    );
    cs.enforce(
        || "inactive lane is zero",
        |lc| lc + inactive.get_variable(),
        |lc| lc + lane.get_variable(),
        |lc| lc,
    );
    cs.enforce(
        || "wide radius is zero",
        |lc| lc + wide.get_variable(),
        |lc| lc + radius.get_variable(),
        |lc| lc,
    );
    let top = significand_bits
        .last()
        .ok_or_else(|| SynthesisError::Unsatisfiable("missing significand bit".into()))?;
    cs.enforce(
        || "normal significand has hidden bit",
        |lc| lc,
        |lc| lc,
        |lc| lc + top.get_variable() - normal.get_variable(),
    );

    let lanes = profile.exponent_lanes();
    let lane_selectors = lanes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            allocate_boolean_num(
                cs.namespace(|| format!("lane_{index}_selected")),
                normal_witness && witness.exponent_lane == index as u64,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    cs.enforce(
        || "lane selector sum",
        |lc| lc,
        |lc| lc,
        |lc| {
            lane_selectors
                .iter()
                .fold(lc, |acc, selector| acc + selector.get_variable())
                - normal.get_variable()
        },
    );
    cs.enforce(
        || "lane index reconstruction",
        |lc| lc,
        |lc| lc,
        |lc| {
            lane_selectors
                .iter()
                .enumerate()
                .fold(lc - lane.get_variable(), |acc, (index, selector)| {
                    acc + (F::from(index as u64), selector.get_variable())
                })
        },
    );

    let minimum_exponent = lanes[0];
    let mut center_lc = LinearCombination::zero();
    let mut radius_lc = LinearCombination::zero();
    for (index, selector) in lane_selectors.iter().enumerate() {
        let selected_sig = selector.mul(
            cs.namespace(|| format!("selected_significand_{index}")),
            significand,
        )?;
        let selected_radius =
            selector.mul(cs.namespace(|| format!("selected_radius_{index}")), radius)?;
        let shift = 1_u128 << (lanes[index] - minimum_exponent) as usize;
        center_lc = center_lc + (field_from_u128::<F>(shift), selected_sig.get_variable());
        radius_lc = radius_lc + (field_from_u128::<F>(shift), selected_radius.get_variable());
    }
    let zero_radius = zero.mul(cs.namespace(|| "zero_radius"), radius)?;
    radius_lc = radius_lc + zero_radius.get_variable();
    let center_witness = common_center(profile, witness);
    let radius_witness = common_radius(profile, witness);
    let common_center = AllocatedNum::alloc(cs.namespace(|| "common_center"), || {
        Ok(field_from_u128(center_witness))
    })?;
    cs.enforce(
        || "common center reconstruction",
        |lc| lc,
        |lc| lc,
        |_| center_lc - common_center.get_variable(),
    );
    let common_radius = AllocatedNum::alloc(cs.namespace(|| "common_radius"), || {
        Ok(field_from_u128(radius_witness))
    })?;
    cs.enforce(
        || "common radius reconstruction",
        |lc| lc,
        |lc| lc,
        |_| radius_lc - common_radius.get_variable(),
    );

    let q = common_denominator(profile);
    let domain_max = domain_max_common(profile);
    let domain_gap_witness = if witness.wide {
        0
    } else {
        domain_max.saturating_sub(center_witness + radius_witness)
    };
    let lower_gap_witness = if witness.wide {
        0
    } else {
        center_witness.saturating_sub(radius_witness)
    };
    let (upper_gap, _) = allocate_ranged(
        cs.namespace(|| "upper_domain_gap"),
        domain_gap_witness,
        bit_length(domain_max),
    )?;
    let (lower_gap, _) = allocate_ranged(
        cs.namespace(|| "lower_domain_gap"),
        lower_gap_witness,
        bit_length(q),
    )?;
    cs.enforce(
        || "nonwide upper endpoint in domain",
        |lc| lc + normal.get_variable() + zero.get_variable(),
        |lc| {
            lc + (field_from_u128::<F>(domain_max), CS::one())
                - common_center.get_variable()
                - common_radius.get_variable()
                - upper_gap.get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "nonwide lower endpoint in domain",
        |lc| lc + normal.get_variable() + zero.get_variable(),
        |lc| {
            lc + common_center.get_variable()
                - common_radius.get_variable()
                - lower_gap.get_variable()
        },
        |lc| lc,
    );
    Ok(AllocatedCfd {
        common_center,
        common_radius,
        wide: wide.clone(),
        center_witness,
        radius_witness,
    })
}

fn enforce_product_enclosure<F, CS>(
    mut cs: CS,
    profile: &CfdProfile,
    input: &AllocatedCfd<F>,
    output: &AllocatedCfd<F>,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let q = common_denominator(profile);
    let complement = AllocatedNum::alloc(cs.namespace(|| "center_complement"), || {
        Ok(field_from_u128(q - input.center_witness))
    })?;
    cs.enforce(
        || "center complement",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (field_from_u128::<F>(q), CS::one())
                - input.common_center.get_variable()
                - complement.get_variable()
        },
    );
    let product = input
        .common_center
        .mul(cs.namespace(|| "center product"), &complement)?;
    let raw_diff = input.center_witness * (q - input.center_witness);
    let target = q * output.center_witness;
    let (center_error, _) = allocate_absolute_difference(
        cs.namespace(|| "product center error"),
        product.get_variable(),
        F::ONE,
        output.common_center.get_variable(),
        field_from_u128(q),
        raw_diff as i128 - target as i128,
        bit_length(q * q),
    )?;
    let distance = allocate_abs_linear(
        cs.namespace(|| "local derivative distance"),
        q as i128 - 2 * input.center_witness as i128,
        |lc| {
            lc + (field_from_u128::<F>(q), CS::one())
                - (F::from(2), input.common_center.get_variable())
        },
        bit_length(q),
    )?;
    let local = input
        .common_radius
        .mul(cs.namespace(|| "radius times local derivative"), &distance)?;
    let quadratic = input
        .common_radius
        .mul(cs.namespace(|| "radius squared"), &input.common_radius)?;
    let lhs = q * output.radius_witness;
    let rhs = raw_diff.abs_diff(target)
        + input.radius_witness * (q.abs_diff(2 * input.center_witness))
        + input.radius_witness * input.radius_witness;
    enforce_inequality_sum(
        cs.namespace(|| "product enclosure inequality"),
        &output.wide,
        lhs,
        bit_length(q * q),
        |lc| {
            lc + (field_from_u128::<F>(q), output.common_radius.get_variable())
                - center_error.get_variable()
                - local.get_variable()
                - quadratic.get_variable()
        },
        rhs,
    )
}

fn enforce_public_scale_enclosure<F, CS>(
    mut cs: CS,
    profile: &CfdProfile,
    input: &AllocatedCfd<F>,
    output: &AllocatedCfd<F>,
    r_num: u64,
    r_den: u64,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let q = common_denominator(profile);
    let raw = r_num as u128 * input.center_witness;
    let target = r_den as u128 * output.center_witness;
    let (center_error, _) = allocate_absolute_difference(
        cs.namespace(|| "scale center error"),
        input.common_center.get_variable(),
        F::from(r_num),
        output.common_center.get_variable(),
        F::from(r_den),
        raw as i128 - target as i128,
        bit_length(4 * q),
    )?;
    let lhs = r_den as u128 * output.radius_witness;
    let rhs = raw.abs_diff(target) + r_num as u128 * input.radius_witness;
    enforce_inequality_sum(
        cs.namespace(|| "scale enclosure inequality"),
        &output.wide,
        lhs,
        bit_length(8 * q),
        |lc| {
            lc + (F::from(r_den), output.common_radius.get_variable())
                - center_error.get_variable()
                - (F::from(r_num), input.common_radius.get_variable())
        },
        rhs,
    )
}

fn enforce_fused_enclosure<F, CS>(
    mut cs: CS,
    profile: &CfdProfile,
    input: &AllocatedCfd<F>,
    output: &AllocatedCfd<F>,
    r_num: u64,
    r_den: u64,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let q = common_denominator(profile);
    let complement = AllocatedNum::alloc(cs.namespace(|| "fused complement"), || {
        Ok(field_from_u128(q - input.center_witness))
    })?;
    cs.enforce(
        || "fused complement relation",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + (field_from_u128::<F>(q), CS::one())
                - input.common_center.get_variable()
                - complement.get_variable()
        },
    );
    let product = input
        .common_center
        .mul(cs.namespace(|| "fused center product"), &complement)?;
    let raw = r_num as u128 * input.center_witness * (q - input.center_witness);
    let target = r_den as u128 * q * output.center_witness;
    let (center_error, _) = allocate_absolute_difference(
        cs.namespace(|| "fused center error"),
        product.get_variable(),
        F::from(r_num),
        output.common_center.get_variable(),
        field_from_u128(r_den as u128 * q),
        raw as i128 - target as i128,
        bit_length(8 * q * q),
    )?;
    let distance = allocate_abs_linear(
        cs.namespace(|| "fused derivative distance"),
        q as i128 - 2 * input.center_witness as i128,
        |lc| {
            lc + (field_from_u128::<F>(q), CS::one())
                - (F::from(2), input.common_center.get_variable())
        },
        bit_length(q),
    )?;
    let local = input
        .common_radius
        .mul(cs.namespace(|| "fused local term"), &distance)?;
    let quadratic = input
        .common_radius
        .mul(cs.namespace(|| "fused quadratic"), &input.common_radius)?;
    let lhs = r_den as u128 * q * output.radius_witness;
    let rhs = raw.abs_diff(target)
        + r_num as u128
            * (input.radius_witness * q.abs_diff(2 * input.center_witness)
                + input.radius_witness * input.radius_witness);
    enforce_inequality_sum(
        cs.namespace(|| "fused enclosure inequality"),
        &output.wide,
        lhs,
        bit_length(16 * q * q),
        |lc| {
            lc + (
                field_from_u128::<F>(r_den as u128 * q),
                output.common_radius.get_variable(),
            ) - center_error.get_variable()
                - (F::from(r_num), local.get_variable())
                - (F::from(r_num), quadratic.get_variable())
        },
        rhs,
    )
}

fn enforce_inequality_sum<F, CS, L>(
    mut cs: CS,
    wide: &AllocatedNum<F>,
    lhs: u128,
    bits: usize,
    expression_without_gap: L,
    rhs: u128,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
    L: FnOnce(LinearCombination<F>) -> LinearCombination<F>,
{
    let gap_witness = lhs.saturating_sub(rhs);
    let (gap, _) = allocate_ranged(cs.namespace(|| "enclosure gap"), gap_witness, bits)?;
    let expression = expression_without_gap(LinearCombination::zero()) - gap.get_variable();
    cs.enforce(
        || "enclosure holds unless output is wide",
        |lc| lc + CS::one() - wide.get_variable(),
        |_| expression,
        |lc| lc,
    );
    Ok(())
}

fn allocate_absolute_difference<F, CS>(
    mut cs: CS,
    left: nova_snark::frontend::Variable,
    left_coefficient: F,
    right: nova_snark::frontend::Variable,
    right_coefficient: F,
    signed_witness: i128,
    bits: usize,
) -> Result<(AllocatedNum<F>, AllocatedNum<F>), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let absolute = signed_witness.unsigned_abs();
    let (value, _) = allocate_ranged(cs.namespace(|| "absolute value"), absolute, bits)?;
    let positive = allocate_boolean_num(cs.namespace(|| "absolute sign"), signed_witness >= 0)?;
    cs.enforce(
        || "selected absolute difference",
        |lc| lc + value.get_variable(),
        |lc| lc + (F::from(2), positive.get_variable()) - CS::one(),
        |lc| lc + (left_coefficient, left) - (right_coefficient, right),
    );
    Ok((value, positive))
}

fn allocate_abs_linear<F, CS, L>(
    mut cs: CS,
    signed_witness: i128,
    expression: L,
    bits: usize,
) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
    L: FnOnce(LinearCombination<F>) -> LinearCombination<F>,
{
    let (absolute, _) = allocate_ranged(
        cs.namespace(|| "linear absolute value"),
        signed_witness.unsigned_abs(),
        bits,
    )?;
    let positive =
        allocate_boolean_num(cs.namespace(|| "linear absolute sign"), signed_witness >= 0)?;
    cs.enforce(
        || "linear absolute relation",
        |lc| lc + absolute.get_variable(),
        |lc| lc + (F::from(2), positive.get_variable()) - CS::one(),
        |_| expression(LinearCombination::zero()),
    );
    Ok(absolute)
}

fn enforce_decision<F, CS>(
    mut cs: CS,
    profile: &CfdProfile,
    value: &AllocatedCfd<F>,
    decision: &AllocatedNum<F>,
    witness: Decision3,
    threshold: u128,
) -> Result<(), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let choices = [Decision3::False, Decision3::True, Decision3::Indeterminate]
        .into_iter()
        .map(|candidate| {
            allocate_boolean_num(
                cs.namespace(|| format!("decision_{candidate:?}")),
                witness == candidate,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    cs.enforce(
        || "decision one hot",
        |lc| lc,
        |lc| lc,
        |lc| {
            choices
                .iter()
                .fold(lc - CS::one(), |acc, bit| acc + bit.get_variable())
        },
    );
    cs.enforce(
        || "decision code",
        |lc| lc,
        |lc| lc,
        |lc| {
            lc + choices[1].get_variable() + (F::from(2), choices[2].get_variable())
                - decision.get_variable()
        },
    );
    cs.enforce(
        || "wide implies indeterminate",
        |lc| lc + value.wide.get_variable(),
        |lc| lc + decision.get_variable() - (F::from(2), CS::one()),
        |lc| lc,
    );
    let q = common_denominator(profile);
    let lower = value.center_witness.saturating_sub(value.radius_witness);
    let upper = value.center_witness + value.radius_witness;
    let candidates = [
        threshold.saturating_sub(1).saturating_sub(upper),
        lower.saturating_sub(threshold),
        threshold.saturating_sub(1).saturating_sub(lower),
        upper.saturating_sub(threshold),
    ];
    let gaps = candidates
        .into_iter()
        .enumerate()
        .map(|(index, gap)| {
            allocate_ranged(
                cs.namespace(|| format!("decision_gap_{index}")),
                gap,
                bit_length(q),
            )
            .map(|pair| pair.0)
        })
        .collect::<Result<Vec<_>, _>>()?;
    cs.enforce(
        || "false proves upper below threshold",
        |lc| lc + choices[0].get_variable(),
        |lc| {
            lc + (field_from_u128::<F>(threshold - 1), CS::one())
                - value.common_center.get_variable()
                - value.common_radius.get_variable()
                - gaps[0].get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "true proves lower at threshold",
        |lc| lc + choices[1].get_variable(),
        |lc| {
            lc + value.common_center.get_variable()
                - value.common_radius.get_variable()
                - (field_from_u128::<F>(threshold), CS::one())
                - gaps[1].get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "indeterminate lower below threshold",
        |lc| lc + choices[2].get_variable() - value.wide.get_variable(),
        |lc| {
            lc + (field_from_u128::<F>(threshold - 1), CS::one())
                - value.common_center.get_variable()
                + value.common_radius.get_variable()
                - gaps[2].get_variable()
        },
        |lc| lc,
    );
    cs.enforce(
        || "indeterminate upper at threshold",
        |lc| lc + choices[2].get_variable() - value.wide.get_variable(),
        |lc| {
            lc + value.common_center.get_variable() + value.common_radius.get_variable()
                - (field_from_u128::<F>(threshold), CS::one())
                - gaps[3].get_variable()
        },
        |lc| lc,
    );
    Ok(())
}

fn enforce_absorbing_wide<F, CS>(mut cs: CS, input: &AllocatedNum<F>, output: &AllocatedNum<F>)
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    cs.enforce(
        || "wide is absorbing",
        |lc| lc + input.get_variable(),
        |lc| lc + CS::one() - output.get_variable(),
        |lc| lc,
    );
}

fn fits_unclipped_unit(profile: &CfdProfile, value: &CfdCircuitValue) -> bool {
    assert_eq!(domain_max_common(profile), common_denominator(profile));
    fits_unclipped_domain(profile, value)
}

fn fits_unclipped_domain(profile: &CfdProfile, value: &CfdCircuitValue) -> bool {
    if value.wide {
        return true;
    }
    let maximum = domain_max_common(profile);
    let center = common_center(profile, value);
    let radius = common_radius(profile, value);
    radius <= center && center + radius <= maximum
}

fn wide_value() -> CfdCircuitValue {
    CfdCircuitValue {
        exponent_lane: 0,
        significand: 0,
        radius: 0,
        zero: false,
        wide: true,
    }
}

fn common_denominator(profile: &CfdProfile) -> u128 {
    let minimum_exponent = profile.exponent_lanes()[0];
    1_u128 << (profile.precision_bits() - 1 + (-i32::from(minimum_exponent)) as usize)
}

fn domain_max_common(profile: &CfdProfile) -> u128 {
    let scaled = profile.maximum() * BigRational::from_integer(common_denominator(profile).into());
    assert!(
        scaled.is_integer(),
        "domínio CFD deve alinhar à escala comum"
    );
    scaled
        .to_integer()
        .to_u128()
        .expect("domínio CFD cabe em u128")
}

fn common_center(profile: &CfdProfile, value: &CfdCircuitValue) -> u128 {
    if value.zero || value.wide {
        0
    } else {
        let exponent = profile.exponent_lanes()[value.exponent_lane as usize];
        value.significand << (exponent - profile.exponent_lanes()[0]) as usize
    }
}

fn common_radius(profile: &CfdProfile, value: &CfdCircuitValue) -> u128 {
    if value.wide {
        0
    } else if value.zero {
        value.radius
    } else {
        let exponent = profile.exponent_lanes()[value.exponent_lane as usize];
        value.radius << (exponent - profile.exponent_lanes()[0]) as usize
    }
}

fn decision_code(decision: Decision3) -> u64 {
    match decision {
        Decision3::False => 0,
        Decision3::True => 1,
        Decision3::Indeterminate => 2,
    }
}

fn decision_from_field<F: PrimeField>(value: Option<F>) -> Decision3 {
    match value {
        Some(value) if value == F::ONE => Decision3::True,
        Some(value) if value == F::from(2) => Decision3::Indeterminate,
        _ => Decision3::False,
    }
}

fn field_from_u128<F: PrimeField>(value: u128) -> F {
    let high = F::from((value >> 64) as u64);
    let mut two64 = F::ONE;
    for _ in 0..64 {
        two64 = two64.double();
    }
    high * two64 + F::from(value as u64)
}

fn bit_length(value: u128) -> usize {
    if value == 0 {
        1
    } else {
        (u128::BITS - value.leading_zeros()) as usize
    }
}

fn range_check<F, CS>(
    mut cs: CS,
    number: &AllocatedNum<F>,
    witness: u128,
    bits: usize,
) -> Result<Vec<AllocatedBit>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    assert!(bits <= 127);
    let allocated = (0..bits)
        .map(|index| {
            AllocatedBit::alloc(
                cs.namespace(|| format!("bit_{index}")),
                Some(((witness >> index) & 1) == 1),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut reconstruction = LinearCombination::zero();
    let mut coefficient = F::ONE;
    for bit in &allocated {
        reconstruction = reconstruction + (coefficient, bit.get_variable());
        coefficient = coefficient.double();
    }
    cs.enforce(
        || "range reconstruction",
        |lc| lc,
        |lc| lc,
        |_| reconstruction - number.get_variable(),
    );
    Ok(allocated)
}

fn allocate_ranged<F, CS>(
    mut cs: CS,
    witness: u128,
    bits: usize,
) -> Result<(AllocatedNum<F>, Vec<AllocatedBit>), SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| "ranged number"), || {
        Ok(field_from_u128(witness))
    })?;
    let bits = range_check(cs.namespace(|| "range"), &number, witness, bits)?;
    Ok((number, bits))
}

fn enforce_boolean<F, CS>(mut cs: CS, number: &AllocatedNum<F>)
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    cs.enforce(
        || "boolean",
        |lc| lc + number.get_variable(),
        |lc| lc + CS::one() - number.get_variable(),
        |lc| lc,
    );
}

fn allocate_boolean_num<F, CS>(mut cs: CS, witness: bool) -> Result<AllocatedNum<F>, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
{
    let number = AllocatedNum::alloc(cs.namespace(|| "boolean number"), || {
        Ok(if witness { F::ONE } else { F::ZERO })
    })?;
    enforce_boolean(cs.namespace(|| "boolean constraint"), &number);
    Ok(number)
}
