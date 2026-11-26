"""Score three frozen propagation arms on a common held-out capture manifest."""

import argparse
from collections import Counter, defaultdict
from datetime import date
import json
import math
from pathlib import Path
import re
from statistics import fmean

from space_weather import replay_timestamp
from stations import BAND_FREQUENCIES


ARMS = ("band_hour", "p533", "weather")
DIGEST = re.compile(r"[0-9a-f]{64}")


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def require_text(record, key):
    value = record.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"missing or invalid {key}")
    return value


def optional_number(record, key):
    value = record.get(key)
    if value is not None and not number(value):
        raise ValueError(f"invalid {key}")
    return value


def _validate_manifest(document):
    if not isinstance(document, dict) or document.get("schema") != 1:
        raise ValueError("expected held-out manifest schema 1")
    models = document.get("model_sha256", {})
    if not isinstance(models, dict) or set(models) != set(ARMS) or any(
        not isinstance(value, str) or DIGEST.fullmatch(value) is None for value in models.values()
    ):
        raise ValueError("expected frozen SHA-256 for band_hour, p533, and weather")
    profile = document.get("profile")
    if not isinstance(profile, dict):
        raise ValueError("expected one common receiver profile")
    require_text(profile, "mode")
    require_text(profile, "filter")
    bandwidth = profile.get("noise_bandwidth_hz")
    if not number(bandwidth) or bandwidth <= 0:
        raise ValueError("expected positive measurement bandwidth")
    split = document.get("holdout", {})
    if not isinstance(split, dict) or split.get("kind") not in ("date", "receiver"):
        raise ValueError("holdout kind must be date or receiver")
    key = "training_utc_dates" if split["kind"] == "date" else "training_receivers"
    values = split.get(key)
    if not isinstance(values, list) or not values or any(not isinstance(value, str) or not value for value in values):
        raise ValueError(f"expected nonempty {key}")
    comparison_set = {date.fromisoformat(value) for value in values} if key == "training_utc_dates" else set(values)
    if not isinstance(document.get("records"), list):
        raise ValueError("expected capture manifest records")
    return profile, split["kind"], comparison_set


def score(document):
    profile, held_out_by, training = _validate_manifest(document)
    seen = {}
    counts = Counter()
    eligible = []
    for row in document["records"]:
        if not isinstance(row, dict):
            raise ValueError("invalid capture record")
        station = require_text(row, "transmitter")
        receiver = require_text(row, "receiver")
        identity = (station, receiver, require_text(row, "transmission_id"))
        if identity in seen:
            if seen[identity] != row:
                raise ValueError(f"conflicting duplicate observation {identity}")
            counts["duplicate"] += 1
            continue
        seen[identity] = row
        when = replay_timestamp(row.get("utc"))
        if (when.date() if held_out_by == "date" else receiver) in training:
            raise ValueError(f"holdout overlaps training data for {identity}")
        if row.get("profile") != profile:
            raise ValueError(f"receiver mode, filter, or bandwidth differs for {identity}")
        band = require_text(row, "band")
        if band not in BAND_FREQUENCIES or station == receiver:
            raise ValueError(f"invalid path or band for {identity}")
        confirmed = row.get("confirmed_tx")
        usable = row.get("eligible_capture")
        if type(confirmed) is not bool or type(usable) is not bool:
            raise ValueError("expected explicit confirmed_tx and eligible_capture")
        if not confirmed or not usable:
            counts["unconfirmed" if not confirmed else "ineligible_capture"] += 1
            continue
        exact = row.get("recovered_exact")
        if type(exact) is not bool:
            raise ValueError("capture-eligible confirmed attempts require an exact recovery outcome")
        disposition = row.get("snr_status")
        snr = optional_number(row, "measured_snr_db")
        upper = optional_number(row, "snr_upper_db")
        if (
            disposition not in ("measured", "below_floor", "unknown")
            or (snr is not None) != (disposition == "measured")
            or (upper is not None) != (disposition == "below_floor")
        ):
            raise ValueError("measured SNR, below-floor limits and unknown status must be recorded separately")
        predictions = row.get("predictions")
        if not isinstance(predictions, dict) or set(predictions) != set(ARMS):
            raise ValueError("all three frozen arms must score the same eligible attempts")
        for arm, prediction in predictions.items():
            if not isinstance(prediction, dict) or not number(prediction.get("probability")) or not 0 <= prediction["probability"] <= 1:
                raise ValueError(f"invalid recovery probability for {arm}")
            optional_number(prediction, "snr_db")
        counts["eligible"] += 1
        counts["exact"] += int(exact)
        counts[f"snr_{disposition}"] += 1
        group = (station, receiver, when.date(), when.hour)
        eligible.append((group, band, exact, snr, predictions))

    scores = {}
    for arm in ARMS:
        snr_errors = []
        brier = []
        groups = defaultdict(lambda: defaultdict(list))
        for group, band, exact, observed_snr, predictions in eligible:
            prediction = predictions[arm]
            probability = prediction["probability"]
            if observed_snr is not None and prediction.get("snr_db") is not None:
                snr_errors.append(prediction["snr_db"] - observed_snr)
            brier.append((probability - exact) ** 2)
            groups[group][band].append((exact, probability))

        matches = []
        tied_observations = 0
        for bands in groups.values():
            if len(bands) < 2:
                continue
            observed = {band: fmean(exact for exact, _ in rows) for band, rows in bands.items()}
            highest = max(observed.values())
            best_observed = {band for band, value in observed.items() if value == highest}
            if len(best_observed) == len(observed):
                tied_observations += 1
                continue
            predicted = {band: fmean(probability for _, probability in rows) for band, rows in bands.items()}
            top = max(predicted.values())
            best_predicted = {band for band, value in predicted.items() if value == top}
            matches.append(len(best_observed & best_predicted) / len(best_predicted))

        scores[arm] = {
            "brier": fmean(brier) if brier else None,
            "snr_bias_db": fmean(snr_errors) if snr_errors else None,
            "snr_mae_db": fmean(abs(error) for error in snr_errors) if snr_errors else None,
            "snr_pairs": len(snr_errors),
            "best_band_fraction": fmean(matches) if matches else None,
            "best_band_groups": len(matches),
            "all_bands_observed_ties": tied_observations,
        }

    if eligible:
        for arm in ("p533", "weather"):
            scores[arm]["paired_brier_difference_vs_band_hour"] = scores[arm]["brier"] - scores["band_hour"]["brier"]
    return {
        "profile": profile,
        "holdout_kind": held_out_by,
        "model_sha256": document["model_sha256"],
        "observations": len(seen),
        "counts": {key: counts[key] for key in (
            "eligible", "exact", "unconfirmed", "ineligible_capture", "duplicate",
            "snr_measured", "snr_below_floor", "snr_unknown",
        )},
        "unique_utc_dates": len({group[2] for group, *_ in eligible}),
        "unique_paths": len({group[:2] for group, *_ in eligible}),
        "arms": scores,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path, help="held-out capture and prediction manifest JSON")
    args = parser.parse_args()
    try:
        result = score(json.loads(args.manifest.read_bytes()))
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
