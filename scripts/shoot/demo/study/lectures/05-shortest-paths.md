# Week 5: Shortest paths

- **BFS** finds shortest paths when every edge costs the same.
- **Dijkstra** handles non-negative weights: always settle the closest unsettled vertex.
- **Bellman-Ford** allows negative edges and detects a negative cycle after `V - 1` rounds.

The trap: Dijkstra with a negative edge can settle a vertex too early and never revisit it.
