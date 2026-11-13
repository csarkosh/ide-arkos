import unittest

from game.combat import damage, turns_to_beat


class Damage(unittest.TestCase):
    def test_armour_comes_off_each_hit(self):
        self.assertEqual(damage(5, 2), 3)

    def test_a_hit_always_does_at_least_one(self):
        self.assertEqual(damage(2, 9), 1)

    def test_a_critical_doubles_the_damage_after_armour(self):
        # design/enemies.md: 4 attack into 3 armour is 1, and a critical makes it 2, not 5.
        self.assertEqual(damage(4, 3, critical=True), 2)

    def test_the_sentry_takes_the_hero_a_while(self):
        self.assertEqual(turns_to_beat(20, 6, 3), 7)


if __name__ == "__main__":
    unittest.main()
