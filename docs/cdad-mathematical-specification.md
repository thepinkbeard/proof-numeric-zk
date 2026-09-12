# Certified Decision-Adaptive Dyadic (CDAD) v1

## 1. Numeric state

A public CDAD profile is

```text
P = (id, version, [F0,...,Fk], [L,U], T, policy, commitment_policy)
```

where the precision ladder is strictly increasing, `T` is a non-empty set of
public thresholds, rounding is round-to-nearest-ties-to-even and overflow is
rejected. The v1 ladder is `[16,32,48,64]` and the input-link policy is the
existing Poseidon circuit.

A value is `(M,R,F,profile_id)` with `R >= 0`, `F` in the public ladder and

```text
gamma(M,R,F) = [(M-R) 2^-F, (M+R) 2^-F] intersect [L,U].
```

The arithmetic and local Taylor enclosure are exactly CBD's. CDAD changes how
a profile is selected, not the theorem proved by an individual profile.

## 2. Decision semantics

For predicate `x >= t`:

```text
True          iff lower(gamma) >= t
False         iff upper(gamma) < t
Indeterminate otherwise.
```

Consequently every definitive result is sound whenever the exact value belongs
to `gamma`. The selection controller never changes this rule.

## 3. Policies

- `fixed`: evaluate only `F0`; this is the fixed-profile ablation.
- `sequential`: prove `F0`, then every successive rung after an abstention.
- `margin-directed`: after an abstention, estimate a candidate from the radius
  and center-to-threshold margin. The prover speculatively recomputes certified
  higher-precision enclosures locally and proves the first candidate that is
  definitive, or the maximum rung. Speculative evaluations are not proofs and
  do not authorize a result.

Every proof after the first is linked to the same committed input. A retry is
counted as one numeric proof plus one measured Poseidon link. Local speculative
evaluation is reported separately from proof cost.

## 4. Soundness claim

Let each fixed-precision evaluator return an enclosure containing the same exact
mathematical opportunity. CDAD returns a definitive Boolean only from one such
enclosure. Therefore:

```text
CDAD=True  => exact >= threshold
CDAD=False => exact < threshold.
```

The policy may select an unnecessarily expensive profile or exhaust the ladder;
neither case invalidates the implication. It affects cost and availability only.

## 5. Explicit non-goals

- CDAD v1 is not IEEE 754 and excludes NaN, infinity and subnormals.
- It does not guarantee a definitive result at 64 bits.
- A Nova instance has one compiled precision. Cross-precision adaptation uses
  separately linked proofs; heterogeneous folding is future work.
- The signed-score gadget constrains one fixed-precision evaluation. A
  specialized vector commitment linking every Adult/ECEF feature across
  retries is not part of v1; the existing scalar input-link policy is the cost
  baseline and the protocol-level requirement.
- Chaotic trajectories may amplify real rounding uncertainty faster than any
  fixed finite ladder can control.
