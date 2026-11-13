"""The roster, as design/enemies.md lists it."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Enemy:
    name: str
    health: int
    attack: int
    armour: int


ROSTER = {
    "cave rat": Enemy("Cave rat", 6, 2, 0),
    "bog slime": Enemy("Bog slime", 10, 3, 1),
    "lantern wisp": Enemy("Lantern wisp", 8, 5, 0),
    "stone sentry": Enemy("Stone sentry", 20, 4, 3),
}
