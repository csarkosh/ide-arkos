# Claude Config Folders Implementation Plan

**Goal:** A Claude Code started under another config folder (`CLAUDE_CONFIG_DIR`) shows its state on its tab, is resumed under that folder after a restart, and the doctor's Claude Code rows read and fix every folder in use.

**Architecture:** Three new pieces in `crates/arkos`, no change to the daemon or the protocol. `claude_folders` keeps the list (the default folder, then the lines of `claude-folders` in the application support folder). `agent_probe` asks the daemon for `ListSessionsDetailed` on a connection of its own, reads the process table, and says which sessions run a `claude`, under which folder, and whether it left an agent record. `doctor_panel::Probe` takes the list of settings files and the tabs that report nothing in place of one settings path. The SessionStart hook writes the folder into the resume command.

**Tech Stack:** Rust, `arkos` (`hook_cli.rs`, `claude_folders.rs`, `agent_probe.rs`, `doctor_panel.rs`, `doctor.rs`, `sessions.rs`, `main.rs`), `libc` (`proc_listallpids`, `proc_pidinfo`, `sysctl` `KERN_PROCARGS2`).

**Spec:** `docs/superpowers/specs/2026-11-12-claude-config-folders-design.md`

## Global Constraints

- Tests headless (`cargo test --workspace`), named as sentences, written before the code they check. Doc comments plain prose; no em-dashes anywhere.
- `nice -n 15 cargo ...`, never `--release`, one cargo at a time; finish with `nice -n 15 cargo check --workspace --all-targets` showing no warnings.
- The protocol, the manifest and the handover do not change: `AgentRecord.resume` is already a list of words.
- With the default folder alone, every doctor text is what it is today: the existing `doctor_panel` tests pass with only their `Probe` literal changed.
- A sandbox never probes and never writes `claude-folders`.

## Review Focus

1. A recovery under another folder: the typed line is `'env' 'CLAUDE_CONFIG_DIR=<folder>' 'claude' '--resume' '<id>'`. Pinned in Task 1 (`an_agent_started_under_another_config_folder_is_resumed_under_it`).
2. The doctor must not say "installed" while a folder in use has no hooks. Pinned in Task 4 (`a_folder_without_hooks_is_named_and_the_row_needs_a_fix`).
3. A plain shell, or a `claude` that left its record, is never called silent. Pinned in Task 3 (`a_claude_with_an_agent_record_reports_and_a_plain_shell_is_not_one`).
4. The probe costs nothing on a timer: it runs at startup, when the doctor opens or acts, and on a title change at most once a minute for a session that reports nothing. Pinned in Task 5 (`a_title_change_asks_for_a_probe_once_a_minute_and_only_for_a_session_that_reports_nothing`).

## Task 1: An agent is resumed under the folder it ran in

**Files:** `crates/arkos/src/hook_cli.rs`

- [x] Tests first: `an_agent_started_under_another_config_folder_is_resumed_under_it` (the words, a folder with a space, an empty or absent folder is today's command), and the run path passes the hook's own `CLAUDE_CONFIG_DIR`.
- [x] `resume_command(kind, id, config_dir: Option<&Path>)`; `request_for(.., config_dir)`; `run_with(.., config_dir)`; `run` reads `CLAUDE_CONFIG_DIR` (empty counts as unset).

## Task 2: The list of folders

**Files:** `crates/arkos/src/claude_folders.rs`, `crates/arkos/src/paths.rs`, `crates/arkos/src/main.rs` (mod)

- [x] Tests first: the default is first and never written; a missing folder is skipped and dropped at the next write; a folder is listed once; a write happens only when the list changes; `tilde` writes the home folder as `~`.
- [x] `paths::claude_folders_file()`, `claude_folders::{default_folder, read, add, settings_of, tilde}`.

## Task 3: The probe

**Files:** `crates/arkos/src/agent_probe.rs`, `crates/arkos/src/main.rs` (mod)

- [x] Tests first, pure: `the_descendants_of_a_shell_are_found_through_the_parent_ids`, `procargs_give_the_words_and_the_environment` (a truncated buffer included), `the_folder_is_the_variable_when_set_and_not_empty`, `a_claude_with_an_agent_record_reports_and_a_plain_shell_is_not_one`.
- [x] Test against a real process: `a_child_named_claude_is_found_with_its_folder` (a `sleep` started with `arg0` `claude` and `CLAUDE_CONFIG_DIR` set, under the test's own pid).
- [x] `agent_probe::{Seen, descendants, parse_procargs, is_claude, folder_of, survey, process_table, procargs, run}`.

## Task 4: The doctor's rows over every folder

**Files:** `crates/arkos/src/doctor_panel.rs`

- [x] Tests first: `a_folder_without_hooks_is_named_and_the_row_needs_a_fix`, `the_scroll_and_theme_rows_name_the_folders_that_need_them`, `one_unparseable_folder_breaks_its_rows_and_names_the_file`, `hooks_everywhere_and_a_silent_tab_asks_for_a_restart_with_no_button`.
- [x] `Probe { settings: &[PathBuf], home, silent: &[String], .. }`; the three rows aggregate; `doctor_panel::needing(..)` gives the folders an action writes.

## Task 5: The window

**Files:** `crates/arkos/src/sessions.rs`, `crates/arkos/src/main.rs`

- [x] Test first: `a_title_change_asks_for_a_probe_once_a_minute_and_only_for_a_session_that_reports_nothing`.
- [x] `SessionManager`: `reporting`, `probed`, `probe_wanted`; set from the list, `Event::State` and `sync_label`.
- [x] `Root`: `claude_folders`, `silent`; `probe_agents(cx)` off the UI thread at startup, when the doctor opens, when the hook action finishes and when `probe_wanted`; a folder first seen gets the startup theme step.
- [x] The scroll, theme and hook actions write every folder that needs it; `follow_theme_in_claude` runs over the list.

## Task 6: `arkos doctor`

**Files:** `crates/arkos/src/doctor.rs`

- [x] Test first: `the_claude_section_lists_each_folder_and_each_silent_session`.
- [x] A "Claude Code config folders" section: each folder's hooks, scroll and theme, then each session that reports nothing with its folder. Read-only: the list file is not written.

## Task 7: Docs

**Files:** `docs/guide.md`, `docs/development.md`, the spec

- [x] The guide's hooks section and doctor rows, "After a restart"; a Status row for the by-hand check; the spec's wording on a broken folder brought in line with the rows.
- [x] `nice -n 15 cargo test --workspace`, `nice -n 15 cargo check --workspace --all-targets` with no warnings.
