# Arkos IDE: images and links (milestone A7)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, sections 3.7, 3.8 and 9, ruling 7); Cyrus asked to start
A7 on 2026-10-30. Builds on A1 to A6 (merged at `084021d`). Already there from A1: a click on a link
reveals `[text](url)` and a click on the revealed address follows it, Cmd+click follows at once, and
an image shows its `![alt](path)` under the picture while the caret is in it.

## 1. Anchors

- A heading's anchor is GitHub's: its text lowercased, every character that is not a letter, a
  digit, a space, `-` or `_` dropped, spaces turned into `-`; a repeated anchor gets `-1`, `-2`.
- `[text](#anchor)` scrolls this document to that heading (unfolding it). `[text](other.md#anchor)`
  opens the other file and scrolls it there once it has loaded; an open tab on it scrolls at once.
  An anchor no heading has, or a link to a heading in a file that is not markdown, opens the file
  at its top, as today.

## 2. Wikilinks

- `[[name]]`, `[[name|alias]]` and `[[name#anchor]]` render as links (the alias, else the name) and
  reveal as written. They follow as links do: `name` beside the document, then `name.md`, then the
  first file named `name` or `name.md` (ignoring case) found under the workspace (the nearest folder
  above the document holding `.git`, else the document's folder), skipping hidden folders, `target`
  and `node_modules`, at most 20,000 entries looked at. A plain relative link that names no file
  falls back the same way.
- `![[picture.png]]` renders the image beside the document.

## 3. Images in

- **Paste.** An image on the clipboard (a screenshot, a browser's Copy Image) is written to the
  image folder as `image.png`, `image-1.png`, `image-2.png` (the first name not taken, the
  extension from its format), and `![image](image.png)` goes in at the caret, in Rendered and Raw
  mode. Undo takes the text out; the file stays.
- **Drop.** Files dropped from Finder on a rendered document go in before the block under the
  pointer, the drop line showing where, each on its own line with a blank line after: an image as
  `![name](path)`, anything else as `[name](path)`. A file inside the document's folder is linked
  where it is; an image from elsewhere is copied in as `image-N` with its own extension; any other
  file from elsewhere is linked by its absolute path. A path with a space is written `<path>`.
- **The image folder.** `[viewer] image_destination` in the config, a folder relative to the
  document's (`{name}` is the document's file name without its extension): empty, the default,
  is the document's own folder; `images` or `{name}.assets` put them in a folder beside it, made
  when first needed. `arkos settings set viewer.image_destination images` sets it.

## 4. Where it lives

`md_links.rs` (new, pure, tested): anchors, the wikilink lookup, the image file name, the folder
pattern, a path made relative and written for markdown. `md_blocks::OPTIONS` gains wikilinks.
`md_view::link_action` gains `Anchor` and the anchor on `OpenMarkdown`. `viewer.rs`: following an
anchor, a pending anchor applied after a load, paste of an image, the drop. `config.rs`,
`agent_cli.rs`: the setting.

## 5. Tests and the screen pass

Headless: anchors with repeats and punctuation, the lookup order, names taken, the pattern, paths
relative and bracketed, wikilinks parsed with their alias and revealed as written. On the local
shoot: a `#anchor` link followed in the document and into another file, a wikilink with an alias
followed, an image pasted (a PNG put on the clipboard by the storyboard's setup), a dropped image
when the shoot can drop (else by the headless test of the insert).
