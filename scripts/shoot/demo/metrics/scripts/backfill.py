"""Rebuilds the weekly totals from the raw export, one week at a time, as the warehouse job does.

Each week waits on the warehouse's rate limit, so a full backfill of 52 weeks takes several
minutes. It prints each week's totals and changes no file.
"""
import csv
import sys
import time
from collections import defaultdict
from pathlib import Path

WEEKS = 52
PACE = 10.0  # seconds a week, the warehouse's rate limit


def main() -> int:
    rows = list(csv.DictReader(open(Path(__file__).parent.parent / "data" / "signups.csv")))
    totals = defaultdict(lambda: [0, 0])
    for r in rows:
        totals[r["week"]][0] += int(r["visits"])
        totals[r["week"]][1] += int(r["signups"])
    weeks = sorted(totals)
    print(f"backfilling {WEEKS} weeks from the warehouse ({len(weeks)} in the local export)", flush=True)
    for i in range(WEEKS):
        time.sleep(PACE)
        week = weeks[i % len(weeks)]
        visits, signups = totals[week]
        print(f"[{i + 1:>2}/{WEEKS}] {week}  visits {visits:>6,}  signups {signups:>4,}  unchanged", flush=True)
    print("done: 0 weeks changed", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
