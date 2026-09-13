"""Decision accounting shared by qualification reports.

Certification is a property of an explicitly identified contract, never an
inference from agreement, proof validity, or missing metadata.
"""
from fractions import Fraction
from functools import lru_cache
import statistics


CERTIFIED_FAMILIES = frozenset({
    "cbd-local", "cbd-sequential", "cfd", "cdad-sequential",
    "cdad-margin-directed", "cdad-preview-all",
})
OBSERVED_FAMILIES = frozenset({"fixed-truncate", "fixed-rne", "fp32"})
CERTIFIED_POLICIES = frozenset({
    "fixed", "sequential", "margin-directed", "preview-all",
    "baseline-cbd", "baseline-cbd-sequential", "baseline-cfd",
})
OUTCOMES = ("certified_same", "observed_same", "observed_flip",
            "abstention", "reference_unresolved")


@lru_cache(maxsize=2048)
def rational(value):
    return Fraction(value)


def interval_decision(lower, upper, threshold):
    if lower > upper:
        raise ValueError("reversed enclosure")
    if lower >= threshold:
        return "True"
    if upper < threshold:
        return "False"
    return "Indeterminate"


def validate_numeric_row(row):
    """Recompute both predicates from exact endpoints; no float decisions."""
    family = row["family"]
    if family not in CERTIFIED_FAMILIES | OBSERVED_FAMILIES:
        raise ValueError(f"unknown numeric family: {family}")
    certified = row.get("certified")
    if type(certified) is not bool or certified != (family in CERTIFIED_FAMILIES):
        raise ValueError("missing or inconsistent certification metadata")
    lo, hi, tau = (rational(row[k]) for k in ("lower", "upper", "threshold"))
    ref_lo, ref_hi = (rational(row[k]) for k in ("reference_lower", "reference_upper"))
    decision = interval_decision(lo, hi, tau)
    ref_label = interval_decision(ref_lo, ref_hi, tau)
    ref_decision = None if ref_label == "Indeterminate" else ref_label == "True"
    if row["decision"] != decision or row["reference_decision"] is not ref_decision:
        raise ValueError("decision disagrees with exact enclosure")
    flip = None if decision == "Indeterminate" or ref_decision is None else (decision == "True") != ref_decision
    if row["predicate_flip"] is not flip:
        raise ValueError("predicate_flip disagrees with exact decisions")
    if certified and (hi < ref_lo or lo > ref_hi):
        raise ValueError("certified and reference enclosures are disjoint")
    if certified and flip is True:
        raise ValueError("certified contradiction invalidates analysis")
    if ref_decision is None:
        outcome = "reference_unresolved"
    elif decision == "Indeterminate":
        outcome = "abstention"
    elif certified:
        outcome = "certified_same"
    else:
        outcome = "observed_flip" if flip else "observed_same"
    return outcome, lo, hi, tau


def timing_is_certified(row):
    flag = row.get("certified_family")
    if flag is not None and type(flag) is not bool:
        raise ValueError("certified_family must be a boolean")
    if flag is True and row["policy"] not in CERTIFIED_POLICIES:
        raise ValueError("unrecognized certified timing policy")
    return flag is True and row["policy"] in CERTIFIED_POLICIES


def summarize_timing(rows):
    """A timing repeat is not an independent population opportunity."""
    if not rows:
        raise ValueError("empty timing group")
    certified_decisions = observed_decisions = flips = 0
    for row in rows:
        if row["decision"] not in {"True", "False", "Indeterminate"}:
            raise ValueError("invalid timing decision")
        certified = timing_is_certified(row)
        definitive = row["decision"] != "Indeterminate"
        ref = row.get("reference_decision")
        if ref not in ("True", "False"):
            raise ValueError("timing reference must be conclusive")
        flip = definitive and row["decision"] != ref
        if row.get("predicate_flip") is not flip:
            raise ValueError("timing flip disagrees with decisions")
        if certified and flip:
            raise ValueError("certified timing contradiction invalidates report")
        certified_decisions += certified and definitive
        observed_decisions += not certified and definitive
        flips += flip
    return {
        "repeats": len(rows),
        "mean_total": statistics.mean(r["complete_wall_seconds"] for r in rows),
        "mean_no_setup": statistics.mean(r["setup_excluded_seconds"] for r in rows),
        "mean_primary_constraints": statistics.mean(r["primary_constraints"] for r in rows),
        "certified_decisions": certified_decisions,
        "observed_decisions": observed_decisions,
        "flips": flips,
        "certified_family": all(timing_is_certified(r) for r in rows),
    }
