# The Inbox stacks, Neighbor stacks on a narrow window, and panes are dragged

Cyrus, 2026-11-14: "I want to let the inbox stack. This is a behavior change to the "Neighbor"
behavior too. On a narrow screen, the default behavior when creating a new pane should be on top.
Allow the panes to be dragged around too."

Why: in the Inbox the session's files always sat to its right with 40% of the width beside the
sidebar, so a table or a rendered document wrapped tightly (the marketing pictures showed it).

## 1. The Inbox's shape

The Inbox was the sessions pane on the left and the session's side (its files, any number of
panes) on the right. Now the side may sit on any of the four sides of the sessions pane: the
tree's root is the sessions pane alone, or one split of either orientation with the sessions pane
as one child and the side as the other. Where the side sits is a `SidePlace`: the side of the
sessions pane it is on, and the sessions pane's share of the split.

- **Where a new side goes.** The place the user last dragged a side to (section 3), kept for the
  window. Until they have, it follows the window: on a narrow window the side goes on top of the
  session, otherwise to its right.
- **The divider.** One position for side by side (the sessions' share, 0.6 at first) and one for
  stacked (0.5 at first), each kept for every session as the single one was, so going from
  session to session keeps the divider where the user put it.
- **Going from session to session.** A session's side comes back at the window's place: the
  user's choice, else what the window's width says now. A side is never moved while it is shown
  because the window was resized.
- Saved with the workspace: `side_ratio` (side by side), `side_stacked` and `side_place`; a file
  from before has neither of the new two and gets the defaults.

## 2. Neighbor on a narrow window

A window is narrow when its panes' area is under 1500 pt wide: a 13 to 14 inch MacBook with the
sidebar shown is narrow, a 16 inch or an external display is not. Neighbor's plain open:

- Not narrow (unchanged): a new tab in the pane to the right, else the one to the left, else a new
  pane split off to the right.
- Narrow: a new tab in the pane above, else the one below, else the one to the right, else the one
  to the left, else a new pane split off on top.

`--split`, `--pane` and Inplace are unchanged. In the Inbox a plain open with no side puts the new
side at the Inbox's place (section 1).

## 3. Dragging a pane

A pane is dragged by its number chip at the left end of its tab bar; the ghost names the pane and
its active tab. It drops where a tab drops, with the same five zones and the same tint:

- On an edge of another pane: the pane moves there, tabs and all, the two sharing that pane's
  space half and half.
- On the centre of another pane, or onto its tab bar: its tabs join that pane, in order, and the
  dragged pane goes.
- On itself: nothing.

In the Inbox the sessions pane has no tab bar and is not dragged. A pane or a tab of the side
dropped on an edge of the sessions pane moves the whole side to that edge, its arrangement kept,
and that place becomes the window's (section 1); the dragged tab is the one shown in its pane. A
drop inside the side arranges the side as in the Grid.

## Tests

`tree.rs`: the Inbox's shape on all four sides; take and put back keep the place and the share;
gather and settle use the place; moving the side; moving a pane to an edge and into another.
`direct.rs`: Neighbor's narrow order and its split on top. `inbox.rs`: a side comes back at the
window's place, a narrow window stacks, the user's place wins, each orientation keeps its divider.
`workspace.rs`: the new fields round-trip and are absent from old files.
