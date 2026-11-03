"""Problem set 4, question 2: the 0/1 knapsack, bottom up."""


def knapsack(weights, values, capacity):
    """The most value that fits in a bag holding at most `capacity`."""
    n = len(weights)
    best = [[0] * capacity for _ in range(n + 1)]
    for i in range(1, n + 1):
        for w in range(capacity):
            best[i][w] = best[i - 1][w]
            if weights[i - 1] <= w:
                best[i][w] = max(best[i][w], best[i - 1][w - weights[i - 1]] + values[i - 1])
    return best[n][capacity - 1]
