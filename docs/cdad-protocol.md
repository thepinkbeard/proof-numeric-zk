# Frozen CDAD v1 experimental protocol

Date: 2026-08-30. Experiment-profile schema: v2 for CDAD, v1 preserved for all
previous families.

## Matrix

- Policies: `fixed`, `sequential`, `margin-directed`.
- Precisions: 16, 32, 48 and 64 fractional bits.
- Workloads: logistic map, stable affine recurrence, Adult logistic score and
  ECEF proximity.
- Logistic parameters: `x0=1/10`, `r={7/2,37/10,39/10,4}` and thresholds
  `{1/4,1/2,3/4}`.
- Baselines: fixed point, CBD, CFD and restricted exact FP32 from the existing
  Pareto protocol.

## Accounting

Each opportunity records initial/final precision, proof attempts, linked
retries, skipped rungs, residual abstention and an attempted-precision histogram.
The adaptive cost is derived only from measured fixed-precision CDAD circuits
and the measured Poseidon link. Logistic/affine use the step kernel; Adult and
ECEF use their measured six- and three-term signed-score kernels:

```text
total = sum_F attempts[F] * C_CDAD[F] + retries * C_link.
```

The Pareto relation uses three axes: cost per correct definitive decision,
incorrect-definitive rate and availability. Certified and merely observed
families remain on separate fronts.

## Acceptance

- Any incorrect definitive CDAD decision is a defect and invalidates the run.
- All certified states must enclose the rational/MPFR reference.
- The directed policy passes the go/no-go only if it is no worse than sequential
  in coverage and strictly cheaper for at least one frozen workload/regime.
- A negative long-horizon chaotic result is retained rather than tuned away.
