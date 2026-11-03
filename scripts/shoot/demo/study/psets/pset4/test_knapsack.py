import unittest

from knapsack import knapsack


class KnapsackTest(unittest.TestCase):
    def test_takes_the_best_pair(self):
        self.assertEqual(knapsack([1, 3, 4, 5], [1, 4, 5, 7], 7), 9)

    def test_an_item_that_fills_the_bag_exactly(self):
        self.assertEqual(knapsack([5], [10], 5), 10)

    def test_nothing_fits(self):
        self.assertEqual(knapsack([6, 7], [3, 4], 5), 0)


if __name__ == "__main__":
    unittest.main()
