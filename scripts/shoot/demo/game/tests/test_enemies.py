import re
import unittest
from pathlib import Path

from game.enemies import ROSTER
from game.world import ROOMS


class Roster(unittest.TestCase):
    def test_the_code_matches_the_design_table(self):
        text = (Path(__file__).parent.parent / "design" / "enemies.md").read_text()
        rows = re.findall(r"^\| ([A-Z][a-z ]+) \| (\d+) \| (\d+) \| (\d+) \|", text, re.M)
        self.assertEqual(len(rows), len(ROSTER))
        for name, health, attack, armour in rows:
            enemy = ROSTER[name.lower()]
            self.assertEqual((enemy.health, enemy.attack, enemy.armour), (int(health), int(attack), int(armour)), name)

    def test_every_room_s_enemy_is_on_the_roster(self):
        for room in ROOMS:
            self.assertIn(room.enemy, ROSTER, room.name)


if __name__ == "__main__":
    unittest.main()
