#!/usr/bin/env python3
"""Treina baseline logístico determinístico no Adult/UCI.

Usa somente as seis variáveis numéricas para manter o kernel circuital pequeno.
O artefato inclui exemplos normalizados próximos e distantes da fronteira.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import pandas as pd


FEATURES = [
    "age",
    "fnlwgt",
    "education_num",
    "capital_gain",
    "capital_loss",
    "hours_per_week",
]
COLUMNS = [
    "age",
    "workclass",
    "fnlwgt",
    "education",
    "education_num",
    "marital_status",
    "occupation",
    "relationship",
    "race",
    "sex",
    "capital_gain",
    "capital_loss",
    "hours_per_week",
    "native_country",
    "income",
]


def sigmoid(x: np.ndarray) -> np.ndarray:
    positive = x >= 0
    result = np.empty_like(x)
    result[positive] = 1.0 / (1.0 + np.exp(-x[positive]))
    exp_x = np.exp(x[~positive])
    result[~positive] = exp_x / (1.0 + exp_x)
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument(
        "--output",
        default=Path("results/adult_logistic_model.json"),
        type=Path,
    )
    parser.add_argument("--epochs", default=400, type=int)
    parser.add_argument("--learning-rate", default=0.15, type=float)
    args = parser.parse_args()

    raw = args.input.read_bytes()
    frame = pd.read_csv(
        args.input,
        names=COLUMNS,
        skipinitialspace=True,
        na_values="?",
    ).dropna()
    x = frame[FEATURES].to_numpy(dtype=np.float64)
    y = (frame["income"].str.strip() == ">50K").to_numpy(dtype=np.float64)

    rng = np.random.default_rng(20260705)
    order = rng.permutation(len(frame))
    split = int(len(order) * 0.8)
    train_index, test_index = order[:split], order[split:]
    mean = x[train_index].mean(axis=0)
    std = x[train_index].std(axis=0)
    std[std == 0] = 1.0
    normalized = np.clip((x - mean) / std, -4.0, 4.0)
    train_x = normalized[train_index]
    train_y = y[train_index]

    weights = np.zeros(train_x.shape[1], dtype=np.float64)
    bias = 0.0
    regularization = 1e-4
    for _ in range(args.epochs):
        score = train_x @ weights + bias
        error = sigmoid(score) - train_y
        weights -= args.learning_rate * (
            train_x.T @ error / len(train_x) + regularization * weights
        )
        bias -= args.learning_rate * error.mean()

    test_x = normalized[test_index]
    test_y = y[test_index]
    test_scores = test_x @ weights + bias
    predictions = test_scores >= 0
    accuracy = float((predictions == test_y).mean())

    # Metade dos casos próximos à fronteira, metade uniformemente espalhada.
    nearest = np.argsort(np.abs(test_scores))[:1000]
    spread_positions = np.linspace(0, len(test_index) - 1, 1000, dtype=int)
    selected = np.unique(np.concatenate([nearest, spread_positions]))
    cases = [
        {
            "features": [float(value) for value in test_x[index]],
            "label": int(test_y[index]),
            "reference_score": float(test_scores[index]),
        }
        for index in selected
    ]

    artifact = {
        "schema_version": 1,
        "dataset": {
            "name": "Adult",
            "doi": "10.24432/C5XW20",
            "source": "https://archive.ics.uci.edu/static/public/2/adult.zip",
            "input_sha256": hashlib.sha256(raw).hexdigest(),
            "rows_after_dropna": int(len(frame)),
        },
        "training": {
            "seed": 20260705,
            "epochs": args.epochs,
            "learning_rate": args.learning_rate,
            "regularization": regularization,
            "train_rows": int(len(train_index)),
            "test_rows": int(len(test_index)),
            "test_accuracy": accuracy,
        },
        "feature_names": FEATURES,
        "normalization_mean": [float(value) for value in mean],
        "normalization_std": [float(value) for value in std],
        "weights": [float(value) for value in weights],
        "bias": float(bias),
        "cases": cases,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(artifact, indent=2) + "\n")
    print(
        json.dumps(
            {
                "output": str(args.output),
                "cases": len(cases),
                "test_accuracy": accuracy,
            }
        )
    )


if __name__ == "__main__":
    main()
