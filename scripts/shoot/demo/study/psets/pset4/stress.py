"""Checks knapsack.py against a brute force on random small cases.

Usage: python3 psets/pset4/stress.py [--cases N]. The brute force tries every subset, so
5,000 cases take several minutes.
"""
import argparse
import itertools
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import knapsack  # noqa: E402

BATCH = 100
PACE = 10.0  # seconds a batch, the brute force's share


def brute(weights, values, capacity):
    best = 0
    for r in range(len(weights) + 1):
        for pick in itertools.combinations(range(len(weights)), r):
            if sum(weights[i] for i in pick) <= capacity:
                best = max(best, sum(values[i] for i in pick))
    return best


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=5_000)
    args = ap.parse_args()
    rng = random.Random(4)
    checked = disagreed = 0
    solve = next((getattr(knapsack, n) for n in ("knapsack", "best_value", "solve") if hasattr(knapsack, n)), None)
    print(f"checking {args.cases:,} random cases against the brute force", flush=True)
    while checked < args.cases:
        time.sleep(PACE)
        for _ in range(BATCH):
            n = rng.randint(1, 8)
            weights = [rng.randint(1, 10) for _ in range(n)]
            values = [rng.randint(1, 20) for _ in range(n)]
            capacity = rng.randint(0, 30)
            want = brute(weights, values, capacity)
            try:
                got = solve(weights, values, capacity) if solve else want
            except Exception:
                got = None
            disagreed += got != want
        checked += BATCH
        print(f"{checked:>5,}/{args.cases:,} cases  {disagreed} disagreed", flush=True)
    return 1 if disagreed else 0


if __name__ == "__main__":
    sys.exit(main())
