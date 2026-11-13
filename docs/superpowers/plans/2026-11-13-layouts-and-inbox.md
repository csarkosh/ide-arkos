# Layouts and the Inbox: plan

Spec: `docs/superpowers/specs/2026-11-13-layouts-and-inbox-design.md`. Each task is tests first
and ends with `cargo check --workspace --all-targets` clean and a commit.

1. **Placement.** `[layout] placement` in the config (`inplace`, `neighbor`); `direct::plan_open`
   takes it for `Placement::Here` (right neighbour, else left, else a split to the right, using
   `Tree::neighbor`); `settings set layout.placement`; the Settings row comes with task 6.
2. **Blocked and the line.** `ClaudeState::Blocked`, `Request::SetAttention`, `Event::Note`,
   `Request::ListNotes`, protocol 11; the daemon keeps and sends the line; `arkos notify --hook`
   maps every event (`hook_cli.rs`), the installed entries change, the doctor's row follows.
3. **Attention.** `attention.rs`: marks (seen, done, later), `attention::of`; `SessionManager`
   keeps notes, marks and each state's time; seen replaces the click's `Idle` for hooked
   sessions; a key typed into a blocked session makes it working; marks saved in the workspace;
   the tab bar's marks take the five shapes.
4. **The list in the Grid.** `sessions_list.rs` (grouping, rows, ages); the sidebar's Sessions
   section; the row menu (End, Mark unread, Done, Later, Rename); "Next session that needs
   you"; the Dock badge.
5. **Ended sessions.** Kept under Done with their resume command; Resume and Remove; saved.
6. **The layout switch.** `[layout] name`; `Tree` regrouping and `inbox::arrange`; the Grid's
   tree kept aside and restored; the Settings Layout section; palette commands.
7. **The Inbox's window.** The sessions pane's header in place of its tab bar; Sessions and
   Files in the sidebar, the filter field, New session; a session's last opened tab brought
   forward; splits and pane drags inert.
8. **The notification.** `layout.notify`; a macOS notification for Needs you from the bundled
   app when the window is not in front; a click focuses the session.
9. **Docs and Status.** The guide's sections, the agent guide's keys and hooks, Status rows; a
   sandbox pass.
