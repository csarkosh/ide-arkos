# Sarkos IDE

A terminal layout manager for running many Claude Code sessions side by side: a grid of
terminal panes with tabs, a docked Chrome pane, a file tree and git worktree explorer, in a
window that stays light on memory, scrolls smoothly, and keeps every session alive in a
separate daemon so a frozen UI never loses work.

Written in Rust with GPUI. The design is in
[docs/superpowers/specs/2026-10-13-sarkos-design.md](docs/superpowers/specs/2026-10-13-sarkos-design.md).

## Status

Milestone 1 of 6: the daemon and one terminal pane. See the spec for the rest. Still needing a
human check, because the automated run could not drive the window: typing, scrolling feel,
resize, a TUI, and Cmd+Q.

## Build and run

```bash
cargo build --release
./target/release/sarkos        # starts sarkosd on demand, opens one terminal in the current folder
```

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/sarkos/sarkosd.log`. To stop everything: `pkill -x sarkosd`.

## Tests

```bash
cargo test
```

## If you get headaches

The defaults (16 px Menlo, line height 1.5, a bright grey on navy, no warm tint) follow
[Check your eyes before your theme](https://csarko.sh/research/check-your-eyes-before-your-theme).
The same research puts a comprehensive eye examination that tests convergence and small
astigmatism ahead of any setting. Not medical advice.

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
