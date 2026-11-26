from copy import deepcopy
import unittest

from evaluate_path_forecast import ARMS, score


PROFILE = {"mode": "BW500 level 1", "filter": "recorded test filter", "noise_bandwidth_hz": 3000}


def observation(identity, band="40m", recovered=True, measured=True):
    row = {
        "transmitter": "A", "receiver": "B", "transmission_id": str(identity),
        "utc": "2026-11-26T03:00:00Z", "band": band, "profile": PROFILE,
        "confirmed_tx": True, "eligible_capture": True, "recovered_exact": recovered,
        "snr_status": "measured" if measured else "below_floor",
        "measured_snr_db": 6 if measured else None,
        "snr_upper_db": None if measured else -5,
        "predictions": {
            "band_hour": {"probability": 0.75 if band == "20m" else 0.25, "snr_db": None},
            "p533": {"probability": 0.5, "snr_db": 8},
            "weather": {"probability": 0.9 if band == "40m" else 0.1, "snr_db": 5},
        },
    }
    return row


def manifest(records):
    return {
        "schema": 1,
        "profile": PROFILE,
        "holdout": {"kind": "date", "training_utc_dates": ["2026-11-25"]},
        "model_sha256": dict.fromkeys(ARMS, "a" * 64),
        "records": records,
    }


class ForecastEvaluationTests(unittest.TestCase):
    def test_common_denominator_and_duplicate(self):
        received = observation(1)
        missed = observation(2, band="20m", recovered=False, measured=False)
        ineligible = dict(observation(3), eligible_capture=False)
        unconfirmed = dict(observation(4), confirmed_tx=False)
        result = score(manifest([received, missed, ineligible, received, unconfirmed]))
        self.assertEqual(result["observations"], 4)
        self.assertEqual(result["counts"], {"eligible": 2, "exact": 1, "unconfirmed": 1,
                                            "ineligible_capture": 1, "duplicate": 1, "snr_measured": 1,
                                            "snr_below_floor": 1, "snr_unknown": 0})
        self.assertEqual(result["arms"]["p533"]["brier"], 0.25)
        self.assertEqual(result["arms"]["p533"]["snr_bias_db"], 2)
        self.assertEqual(result["arms"]["weather"]["snr_bias_db"], -1)
        self.assertEqual(result["arms"]["band_hour"]["snr_pairs"], 0)
        self.assertEqual(result["arms"]["p533"]["snr_pairs"], 1)
        self.assertAlmostEqual(result["arms"]["weather"]["brier"], 0.01)
        self.assertEqual(result["arms"]["band_hour"]["best_band_fraction"], 0)
        self.assertEqual(result["arms"]["p533"]["best_band_fraction"], 0.5)
        self.assertEqual(result["arms"]["weather"]["best_band_fraction"], 1)

    def test_tests_never_mix_training_dates_or_conflicting_attempts(self):
        changed_profile = deepcopy(PROFILE)
        changed_profile["noise_bandwidth_hz"] = 500
        with self.assertRaisesRegex(ValueError, "differs"):
            score(manifest([dict(observation(1), profile=changed_profile)]))
        with self.assertRaisesRegex(ValueError, "overlaps"):
            score(manifest([dict(observation(1), utc="2026-11-25T03:00:00Z")]))
        with self.assertRaisesRegex(ValueError, "conflicting duplicate"):
            score(manifest([observation(1), dict(observation(1), recovered_exact=False)]))


if __name__ == "__main__":
    unittest.main()
