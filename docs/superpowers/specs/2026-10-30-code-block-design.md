# Arkos IDE: the code block's chrome (milestone A5)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, section 9); Cyrus asked to start A5 on 2026-10-30.
Builds on A1 to A4, merged at `8386495`.

## 1. What changes

A fenced code block in the rendered view reads and edits as Mark Sharp's does (exploration 3.4):

- A header row over the code: at the left a language pill (the fence's language by name, "Plain
  text" with none), at the right a Copy button. A click on the pill opens a language list under it
  with a search field ("Search for a language..."); typing filters, Up and Down choose, Enter or a
  click sets the fence's language, Escape closes. Copy puts the code (without its fences) on the
  clipboard and reads "Copied" for a second and a half.
- Line numbers in a gutter left of the code, the number on a line's first displayed row when it
  wraps.
- With the caret in it, the block stays a code block: its code lines in the same box, at the same
  size, with the same syntax colours and numbers, the header still there; the fence lines never
  show. The caret steps over them as it steps over a bullet: Up from the first code line and Down
  from the last leave the block, Left from the first code line's start goes to the line above it,
  Right from the last code line's end to the line below; an edit with the caret on a fence line
  (after an undo) lands in the code. Shift+Enter on the last code line still leaves the block (A2).
- A click on the header (not on the pill or the button) selects the whole block, fences included,
  so Delete removes it, as Mark Sharp's click on a code block's border does.
- An indented code block (no fences) gets the header and numbers too; its pill is plain and opens
  nothing, since it has no fence to name a language in.
- A `mermaid` block keeps its diagram while inactive (Tier B); with the caret in it, it is a code
  block like the others, its pill reading "Mermaid". Its Code, Diagram and Split modes are A9's.

Raw mode is unchanged.

## 2. The languages

A list of common languages with the info-string id each writes: Plain text (none), Bash, C, C++,
C#, CSS, Diff, Dockerfile, Go, GraphQL, HTML, Java, JavaScript, JSON, Kotlin, Lua, Makefile,
Markdown, Mermaid, Objective-C, Perl, PHP, Python, R, Ruby, Rust, Scala, SQL, Swift, TOML,
TypeScript, XML, YAML. A fence whose id is not in the list shows the id itself on the pill. Setting
a language rewrites the opening fence line (its marker and indent kept) as one undoable edit.

## 3. Where it lives

`md_code.rs` (new, pure): a block's fences and code lines, the languages and their filter, the
fence edit, splitting a block's colour runs by line. `viewer.rs`: the caret rules, the picker's
state and keys, Copy, the header's select. `md_view.rs`: the header, the numbered rows, the code
block drawn editable; `viewer_element.rs`: shaping a code line with its colours. `lang_menu.rs`
(new): the language list over the window.

## 4. Tests and the screen pass

Headless: fences and code lines of fenced, tilde, unclosed and indented blocks; the language names
and filter; the fence edit; run splitting; the caret rules over fence lines; the copy text. On the
local shoot: a code block's header and numbers; the caret in it keeps the box and colours; Up and
Down out of it; the pill's list, typing "py" and Enter; Copy reads "Copied"; a header click and
Delete removes the block.
