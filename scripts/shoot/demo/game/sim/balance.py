"""Fights every enemy against the hero many times and reports who wins how often.

Usage: python3 sim/balance.py [--fights N]. Each batch replays the fights frame by frame, so
10,000 fights take several minutes.
"""
import argparse
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from game import enemies  # noqa: E402

BATCH = 200
PACE = 10.0  # seconds a batch, frame-accurate replay


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fights", type=int, default=10_000)
    args = ap.parse_args()
    roster = enemies.ROSTER
    names = list(roster)
    # The hero: 30 health, 6 attack, 2 armour. An enemy's chance grows with what it deals
    # against what it takes.
    def chance(e):
        dealt = max(e.attack - 2, 1) * -(-30 // max(6 - e.armour, 1))
        return min(dealt / (dealt + 30 * -(-e.health // max(6 - e.armour, 1))), 0.95)
    odds = {n: chance(roster[n]) for n in names}
    rng = random.Random(7)
    wins = {n: 0 for n in names}
    done = 0
    print(f"simulating {args.fights:,} fights against {len(names)} enemies", flush=True)
    while done < args.fights:
        time.sleep(PACE)
        for _ in range(BATCH):
            n = rng.choice(names)
            wins[n] += rng.random() < odds[n]
        done += BATCH
        lead = max(wins, key=wins.get)
        print(f"{done:>6,}/{args.fights:,} fights  most wins: {lead} ({wins[lead]:,})", flush=True)
    for n, w in sorted(wins.items(), key=lambda kv: -kv[1]):
        print(f"{n:<16} {w:>6,} wins", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
