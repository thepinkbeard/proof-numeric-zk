#!/usr/bin/env python3
"""Modelo local auditável para custo FP32 no mapa logístico.

Este script não mede um gadget R1CS. Ele produz uma tabela de estimativas para
um subconjunto FP32 positivo/normal usado apenas como baseline metodológico.
Os números separam dois cenários:

- packed_integer_optimistic: mantissas tratadas como inteiros com uma relação
  multiplicativa por multiplicação FP32, ainda exigindo decomposições/ranges.
- bit_level_schoolbook: multiplicação de mantissas por produtos bit-a-bit
  schoolbook, útil como teto simples e reprodutível para uma implementação
  bit-level direta.

Ambos ignoram NaN, infinito, subnormal, sinais gerais e todos os caminhos de
overflow/underflow. Portanto são modelos de subconjunto, não custos IEEE 754
completos.
"""

import csv
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULTS = ROOT / "results"
RESULTS.mkdir(exist_ok=True)


def row(
    model,
    mantissa_mult_relations,
    bit_boolean_constraints,
    range_or_reconstruction_constraints,
    normalization_constraints,
    rounding_constraints,
    exponent_constraints,
    classification_constraints,
    notes,
):
    total = (
        mantissa_mult_relations
        + bit_boolean_constraints
        + range_or_reconstruction_constraints
        + normalization_constraints
        + rounding_constraints
        + exponent_constraints
        + classification_constraints
    )
    return {
        "baseline": "fp32_positive_normal_subset",
        "model": model,
        "operations_per_logistic_step": "one_subtract_two_multiplies",
        "mantissa_mult_relations": mantissa_mult_relations,
        "bit_boolean_constraints": bit_boolean_constraints,
        "range_or_reconstruction_constraints": range_or_reconstruction_constraints,
        "normalization_constraints": normalization_constraints,
        "rounding_constraints": rounding_constraints,
        "exponent_constraints": exponent_constraints,
        "classification_constraints": classification_constraints,
        "estimated_constraints_per_step": total,
        "notes": notes,
    }


MANTISSA_BITS = 24
EXPONENT_BITS = 8
FP32_BITS = 32

# Logistic step order used in this project:
#   a = r * x
#   b = 1 - x
#   y = a * b
MULTIPLIES = 2
SUBTRACTS = 1

# Shared bit exposure for x, b, a, y. This is deliberately small: a real gadget
# often exposes more intermediate flags and selected candidates.
SHARED_FP_DECOMPOSITIONS = 4 * FP32_BITS
SHARED_RECONSTRUCTIONS = 4

# Positive normal classification: exponent nonzero and not all-ones for x, a,
# b, y. Implemented here as a low local model: two boolean flags per value plus
# their consistency hooks. This excludes NaN/Inf/subnormal logic beyond the
# positive-normal subset.
POSITIVE_NORMAL_CLASSIFICATION = 4 * 4

# Exponent arithmetic and comparisons: two multiply exponent additions, one
# subtract alignment/comparison, and three biased exponent result checks.
EXPONENT_MODEL = MULTIPLIES * (2 * EXPONENT_BITS + 3) + SUBTRACTS * (
    2 * EXPONENT_BITS + 8
)

# Normalization for two products and one subtraction. The model uses one-hot
# leading-bit/shift selection buckets, not a fully optimized lookup argument.
NORMALIZATION_MODEL = MULTIPLIES * (MANTISSA_BITS + 4) + SUBTRACTS * (
    MANTISSA_BITS + 8
)

# Round-to-nearest-even needs guard, round, sticky and tie/even selection for
# each operation.
ROUNDING_MODEL = (MULTIPLIES + SUBTRACTS) * 12

packed = row(
    "packed_integer_optimistic",
    mantissa_mult_relations=MULTIPLIES,
    bit_boolean_constraints=SHARED_FP_DECOMPOSITIONS,
    range_or_reconstruction_constraints=SHARED_RECONSTRUCTIONS
    + MULTIPLIES * 2  # product/remainder range hooks
    + SUBTRACTS * 2,  # borrow/difference range hooks
    normalization_constraints=NORMALIZATION_MODEL,
    rounding_constraints=ROUNDING_MODEL,
    exponent_constraints=EXPONENT_MODEL,
    classification_constraints=POSITIVE_NORMAL_CLASSIFICATION,
    notes=(
        "Optimistic packed-integer subset model; not a measured R1CS gadget; "
        "excludes NaN/Inf/subnormal/general sign paths."
    ),
)

bit_level = row(
    "bit_level_schoolbook",
    mantissa_mult_relations=MULTIPLIES * MANTISSA_BITS * MANTISSA_BITS,
    bit_boolean_constraints=SHARED_FP_DECOMPOSITIONS
    + MULTIPLIES * (2 * MANTISSA_BITS * MANTISSA_BITS),
    range_or_reconstruction_constraints=SHARED_RECONSTRUCTIONS
    + MULTIPLIES * (2 * MANTISSA_BITS)
    + SUBTRACTS * MANTISSA_BITS,
    normalization_constraints=NORMALIZATION_MODEL,
    rounding_constraints=ROUNDING_MODEL,
    exponent_constraints=EXPONENT_MODEL,
    classification_constraints=POSITIVE_NORMAL_CLASSIFICATION,
    notes=(
        "Direct schoolbook bit-level subset model; not optimized; "
        "excludes NaN/Inf/subnormal/general sign paths."
    ),
)

def generate():
    path = RESULTS / "fp32_constraint_model.csv"
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(packed.keys()))
        writer.writeheader()
        writer.writerows([packed, bit_level])
    return path


if __name__ == "__main__":
    print(generate())
