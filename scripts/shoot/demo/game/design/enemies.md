# Enemies

Every enemy is a row here before it is a line of code; the tests check the two agree.

## The roster

| Enemy | Health | Attack | Armour | Room |
|:---|---:|---:|---:|:---|
| Cave rat | 6 | 2 | 0 | Cellar |
| Bog slime | 10 | 3 | 1 | Hall |
| Lantern wisp | 8 | 5 | 0 | Chapel |
| Stone sentry | 20 | 4 | 3 | Gate |

## Rules of thumb

- A new room's enemy should take the hero three or four turns to beat at the level they arrive.
- Armour is taken off each hit, and a hit always does at least 1.
- A critical hit doubles the damage **after** armour, never before.

## To do

- [x] Cave rat, bog slime
- [x] Lantern wisp
- [x] Stone sentry
- [ ] Something for the frozen stair, the room after the gate
