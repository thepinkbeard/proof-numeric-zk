# Certified Floating Dyadics: IEEE-Shaped Certified Arithmetic for Iterative Proofs

## Abstract

Exact floating-point circuits provide familiar semantics but impose
normalization, alignment and rounding constraints at every operation. Fixed
point is cheaper but binds precision and range to a global scale. We introduce
Certified Floating Dyadics (CFD), an IEEE-shaped representation for iterative
verifiable computation that carries a normalized floating-point center and an
integer uncertainty radius in units in the last place. Honest execution rounds
centers to nearest with ties to even, while the proof relation certifies
interval enclosure and threshold-decision safety rather than bitwise operation
equivalence. Public exponent corridors make variable scaling explicit and an
absorbing `wide` state prevents silent truncation when a profile is exhausted.
In a Nova/R1CS prototype, a p=24 sequential logistic-map step uses 1,003 raw
constraints, 41.7% fewer than a restricted exact FP32 step with the same
operation boundaries (1,721). A stable affine recurrence uses 846 constraints,
40.3% fewer than exact FP32 (1,418). Across 43 profiles and trajectories of up
to 1,000 steps, no definitive certified decision disagrees with rational/MPFR
reference execution. Coverage is complete for the stable workload and the
periodic logistic regime, but uncertainty becomes `wide` after 13–41 steps in
the measured chaotic regimes. This negative result shows that exponent
corridors alone do not control uncertainty growth and motivates relative or
logarithmic certificate designs.

## Intended contribution

1. A canonical IEEE-shaped value and public profile whose zero-radius p=24
   subset is bijective with supported binary32 normals and `+0`.
2. An enclosure relation that separates deterministic honest RNE execution
   from the weaker, safety-preserving statement required inside IVC.
3. R1CS gadgets with one-hot public exponent corridors, ULP radii, absorbing
   fallback and trivalent decisions propagated through Nova folds.
4. A homogeneous comparison against exact restricted FP32 and CBD/fixed-point
   baselines, reporting both raw constraints and decision availability.
5. Evidence that CFD beats exact FP32 cost in two workloads, together with the
   negative finding that it does not yet dominate CBD in chaotic coverage.

## Main theorem

If the initial CFD interval contains the exact initial value and every folded
transition encloses the full image of its input interval, every later CFD state
contains the exact trajectory. Consequently, a certified `True` or `False`
decision agrees with the exact threshold predicate; all interval crossings are
`Indeterminate`.

The theorem follows directly by induction and is independent of the honest
center-selection algorithm. A malicious center cannot create a false decision
because it remains constrained by enclosure; it can only enlarge uncertainty
or select `wide`, reducing availability.

## Experimental questions

- **RQ1 — Soundness:** are there enclosure violations or false definitive
  decisions under property tests, adversarial witnesses, reduced-format
  enumeration and long trajectories?
- **RQ2 — Circuit cost:** does sequential CFD reduce raw constraints by at
  least 30% relative to exact restricted FP32 at identical operation
  boundaries?
- **RQ3 — Availability:** which profiles maximize definitive decisions per
  constraint before radius saturation?
- **RQ4 — Causality:** how much of the saving comes from certified semantics,
  and how much from fusing normalization at checkpoints?

## Current answer

RQ1 and RQ2 are supported for the implemented tests and workloads. RQ3 is not
yet positive against the complete CBD frontier: the p=24/9-lane profile is
cheaper than FP32 but can be dominated by CBD-32 in chaotic definitive horizon.
RQ4 is quantified by the 1,003-constraint sequential circuit versus the
777-constraint fused ablation.

The paper should therefore claim “IEEE-shaped certified arithmetic for
iterative proofs”, not IEEE 754 compliance or universal Pareto dominance.

**Atualização após a fronteira (0020).** RQ1 passou de "não achamos
contraexemplo" a enumeração exaustiva em formato reduzido. RQ3 tem resposta, e
ela é por regime, não global: em regimes contrativos o prêmio de certificação é
de 2,2 a 2,6×; em caos com limiar `1/2` ou `3/4` nenhuma configuração não
certificada é sound, e o prêmio não é definível por falta de comparação. O
prêmio de certificação não é constante da representação — é função de quão perto
do limiar o problema chega.

A afirmação central do capítulo deslocou-se de CFD para a métrica: a unidade
correta de comparação é custo esperado por decisão correta, e é ela que resolve
o impasse entre custo e cobertura.

## Required work before submission

Estado em 2026-07-30. O capítulo consolidado está em
[chapter-certified-frontier.md](chapter-certified-frontier.md); os itens abaixo
foram fechados nas rodadas 0017–0023.

- [x] implement reduced-format exhaustive enumeration — 6 bits, sem
  contraexemplo, contra 10 de 65 do ponto fixo na mesma grade
  ([0022](progress/0022-validacao-e-consolidacao.md));
- [x] run Berkeley TestFloat/SoftFloat differential validation for honest
  centers — passa em host x86_64 Linux ([0022](progress/0022-validacao-e-consolidacao.md));
- [x] constrain and measure `next_profile` reproving links to the same
  initial-state commitment — 1.281 constraints, e o vínculo domina os perfis
  baratos ([0021](progress/0021-escalonamento-em-circuito.md));
- [x] compare cost per definitive decision against CBD-16/32/48/64 and fixed
  point — fronteira com 694 pontos, veredito por regime
  ([0020](progress/0020-fronteira-pareto.md));
- [x] add confidence intervals and repeated timing runs on the declared machine —
  30 rodadas pareadas; tempo não separa as representações a 10 folds, e o motivo
  está medido ([0023](progress/0023-medicao-estatistica.md));
- [x] threat-to-validity — seção 7 do capítulo.

Restante:

- circuitos de produto interno para Adult e ECEF, que hoje têm cobertura nas
  quatro famílias mas nenhum custo medido;
- expose public per-step innovations in the affine circuit and evaluate varied
  innovation sequences;
- run the complete p/radius/lanes matrix with 100 and 1,000 Nova folds;
- complete related work with primary citations.
