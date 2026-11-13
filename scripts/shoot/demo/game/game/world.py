"""The rooms, in the order the hero meets them."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Room:
    name: str
    enemy: str


ROOMS = [
    Room("The cellar", "cave rat"),
    Room("The flooded hall", "bog slime"),
    Room("The old chapel", "lantern wisp"),
    Room("The gate", "stone sentry"),
    Room("The frozen stair", "stone sentry"),
]
