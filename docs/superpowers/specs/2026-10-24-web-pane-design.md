# The web pane: an in-process WKWebView replaces Chrome docking

**Date:** 2026-10-24. **Status:** approved for planning. **Supersedes** section 8 of
`2026-10-13-arkos-design.md` (Chrome docking) and its 2026-10-15 rulings, and reverses that spec's
ruling against WKWebView (its section 1, "no embedded Chromium (CEF), no WKWebView", and its
"WKWebView via `wry`" rejection). The video pane spec's section 14 named this as the next
milestone and `Tab::Video` as its template; this is that milestone.

## 1. Why this exists

The browser pane of milestone 5 does not render anything. It is a placeholder that a real Google
Chrome window is kept positioned over through the macOS Accessibility API, polled twenty times a
second. It works, and Cyrus greatly dislikes it (2026-10-24): the Chrome window is another app's
window, so it flashes when Arkos is raised and the window re-raised over it; it spends 80 px on
Chrome's own tab strip and toolbar; nothing of Arkos can draw over it (no palette, no drop zones,
no notice); it needs the Accessibility permission, which is the most alarming prompt macOS has;
it needs Chrome installed; a window Claude's browser tools open may or may not be the one docked;
and the whole thing is a seam the user can see.

The Codex desktop app, which is the comparison Cyrus made, does not dock anything. It is an
Electron app, its browser pane is an Electron `<webview>` guest of the app's own bundled
Chromium, and the agent drives that guest over the Chrome DevTools Protocol through a bundled
plugin (the research doc "How Codex embeds its browser", csarko.sh, 2026-10-24). The lesson is
not "use Electron"; it is "own the view". Arkos has no Chromium to own and will not ship one
(CEF is hundreds of megabytes, the Electron footprint this project exists to escape). macOS ships
WebKit, and `WKWebView` is a real `NSView` that any app may put in its window: it resizes, hides,
stacks and focuses like any child view, its content process is the system's, it needs no
permission, and it plays `<video>` and `<audio>` with hardware decoding.

After this milestone a browser tab is a page drawn inside the Arkos window by WebKit, with a slim
bar of Arkos's own above it: back, forward, reload, the address. An agent in an Arkos terminal
puts a page in the pane beside itself with `arkos open http://localhost:4173/` or
`arkos open out/report.html`, and when it rewrites that HTML file the tab reloads on its own. The
Accessibility permission, `arkos browser-probe`, the dock and undock commands, the drag-in gesture
and Chrome's app mode all go.

The test of the design: a fresh Claude Code session inside Arkos, asked "show me the report with
the videos in it beside you", runs `arkos open out/report.html`, and the page appears in a pane
next to the session with its `<video>` elements playing from files next to it. The agent edits the
HTML, and within two seconds the pane shows the new page.

Agent-driven browsing (the agent clicking, reading the DOM, taking screenshots, as Codex does over
CDP) is the next iteration, not this one. This spec leaves the hooks for it (section 12) and
builds nothing of it.

## 2. What ships

1. **A web tab** (section 3): `Tab::Browser { url }`, one `WKWebView` per tab hosted as a child
   view of the Arkos window, placed over the tab's content rectangle every frame and hidden
   whenever the tab is not the one on screen.
2. **The bar** (section 4): 28 px of Arkos chrome above the page: back, forward, reload or stop,
   the address field, a progress hairline, and "open in your browser".
3. **Loading rules** (section 5): `http`, `https` and `file`; a local page may read the files
   beside it; media plays without a click; a failed load shows the reason in Arkos's own
   screen; a page that opens a new window opens it in the same tab; downloads are refused with a
   pointer.
4. **The tab follows its file** (section 6): a local page reloads when its file changes, under
   the video tab's stability rule.
5. **The window's ways in** (section 7): Cmd+Shift+B asks for an address; the palette's "New
   browser tab…"; a `.html` file in the sidebar opens as a web tab on a double click; a link
   Cmd+clicked in a terminal still opens in the default browser (unchanged).
6. **`arkos open <url | file.html>`** (section 8): the agent's way in, through the targeted
   channel of protocol 8, with the preview pane rule, reuse and reload.
7. **Removals** (section 9): everything of Chrome docking, with the compatibility of saved
   workspaces, configs and keybindings kept.
8. **Docs, telemetry rows, Status rows and tests** (sections 10, 11, 13).

Out of scope, listed so nobody builds it by accident: agent control of the page (section 12 only
names the hooks); find in page; a download manager; a devtools pane (the Web Inspector is a
WebKit preference and a right-click; section 5 turns it on); several profiles; cookies shared
with Safari or Chrome; tabs within a web tab (one page per Arkos tab, as one video per video
tab); printing; reader mode; bookmarks and history beyond back and forward; content blocking;
extensions; HTTP authentication dialogs beyond the one WebKit draws itself; and
`WKWebView` on anything but the project window (the chooser and the crash dialog never host one).

## 3. The web tab

### 3.1 Model and persistence

`Tab::Browser { url: String }` replaces `Tab::Browser { url: Option<String>, app_mode: bool }`.
The variant keeps its serialized name `Browser`, so a workspace file from 0.6 reads: a saved
`url` of `null` or a missing one becomes the empty string (a blank tab), and a saved `app_mode`
is ignored (serde's default for unknown fields in this enum; `Tab` is not `deny_unknown_fields`,
and a test proves the 0.6 shape reads). `url` is the page the tab shows, kept current: after each
committed navigation the tab's `url` is replaced, so a relaunch comes back on the page the user
left, not the one the tab was opened with. A local page is stored as its `file://` URL. The
workspace is saved when a navigation commits (through the existing save queue, coalesced; never
per frame). The web view's own state (scroll, form fields, back and forward lists) is not
persisted; a relaunch loads the URL fresh. Nothing else about the tab is saved.

The tab's title is the page's `title` while it has one, else the host for an `http(s)` page, the
file name for a `file` page, and "New tab" for a blank one. The title follows the page (the
`WKWebView.title` is read each frame the tab is visible; a KVO observer is not needed and keeps
objc2 class definitions out of this milestone). The user's rename of the tab (the existing
double-click rename) wins over the page title, as it does over a terminal's.

The slim 14 px tab bar of milestone 5 goes with it: a pane holding a web tab shows the normal tab
bar, since the page's title is now on the tab.

### 3.2 Hosting

Each web tab owns a `web_view::WebView` entity (the video tab's `VideoView` is the template): the
model (URL, loading state, progress, title, error, zoom, the file follow), behind a `Web` trait
with `WkWebView` as the real implementation and `FakeWeb` for tests, exactly as `video::Player` is
behind `AvPlayer` and the tests' fake. `Root.webs: HashMap<TabId, Entity<WebView>>`, created on
first draw, dropped when the tab closes (the view is removed from its superview in `Drop`).

The real view is a `WKWebView` created on the UI thread with `initWithFrame:configuration:` and
added as a subview of the Arkos `NSWindow`'s content view (`Root.ns_window`, which the window
already keeps). gpui does not know it is there. It draws above gpui's Metal layer because it is a
later subview of the same layer-backed content view. Nothing else is required; a probe at the
start of implementation confirms it (section 14).

Geometry: the tab's content element is a `canvas` that reports its bounds every frame it is
drawn (the existing `report_browser_rect` mechanism, kept and renamed `report_web_rect`), in
window pixels with gpui's top-left origin. After each frame, from the same place the dock tick
ran, `Root::place_webs` sets each visible web view's frame: the bounds converted to points,
flipped against the content view's height when the content view is not flipped (checked once with
`isFlipped`, never assumed), and minus the bar's 28 px. A frame that has not changed is not
written (comparing the last frame written), so a still window writes nothing.

Visibility: a web view is shown only when all of these hold, else `setHidden(true)`:

- its tab is the active tab of its pane, and the pane is on screen (not hidden under a maximized
  sibling), which is the same as "its canvas was drawn this frame";
- no Arkos surface covers the panes: the command palette, the URL prompt, the tab menu, a
  popover, the settings window's sheet, the doctor, the about box, the drop overlay of a tab
  drag, the update banner's sheet. The rule is one function, `covered(&Root) -> bool`, read where
  those are drawn, and a new overlay adds itself to it (as a new animation adds itself to
  `any_motion`);
- the window is not minimized or hidden (`Root.window_hidden`, the AppKit check the dock tick
  already made).

Hiding is instant (there is no animation to run; the web view keeps its page and its state while
hidden, and showing it again paints the same page). A hidden web view pauses nothing by itself:
a video playing in a hidden tab keeps playing with sound, as a background Safari tab would.
Cyrus's call (ruling 6): the tab's switch does not pause media; closing the tab stops it.

Focus and keys: a click in the page makes the `WKWebView` the window's first responder; the
pane is marked focused at the same time (the click's location is inside the pane's web rect, which
the per-frame pass sees from `firstResponder`; `focus_pane` is called without taking the key
back). While the web view is first responder, the page gets the typing, and gpui's key handling
does not run. Key equivalents (Cmd+anything) reach gpui first: AppKit asks the content view's
`performKeyEquivalent:` before the subview's, and gpui's view answers the ones its keymap binds,
so Cmd+1 to 9, Cmd+W, Cmd+T, Cmd+B, Cmd+Shift+B, Cmd+K and the rest keep working with a page
focused; a chord gpui does not bind falls through to the page (Cmd+C, Cmd+V, Cmd+A, Cmd+Z,
Cmd+F for the page's own find, Cmd+plus and minus, section 4). The probe confirms this order
(section 14); if it turns out wrong, the fallback is a local `NSEvent` monitor that hands Cmd
chords to the content view first. A click on the bar, the tab bar or another pane gives the
keyboard back to gpui (`screen::make_first_responder`); Escape in a page is the page's own, since
only key equivalents pass through it (ruling 15, which replaces an earlier "Escape returns"
rule). Cmd+L takes the keys back and puts them in the address field.

Theme: the bar reads `root.theme` at paint time like every other element. The page has its own
colours. Before a page paints, and behind a page with no background, the view shows
`underPageBackgroundColor`, which `Root::finish_theme` sets to the final theme's
`chrome.window_bg` (never per frame; the CLAUDE.md rule for anything pushed into a program).

### 3.3 The web view's lifetime

One `WKWebViewConfiguration` per view, from one shared `WKProcessPool` (so pages share a
WebContent process group the way Safari's do) and the default `WKWebsiteDataStore`
(`defaultDataStore`, persistent: cookies, local storage and caches live in the app's own
container under `~/Library/WebKit/sh.csarko.arkos`, apart from Safari and Chrome, which is the
same isolation Codex chose: sign in by hand when a page needs it; nothing of the user's browser
comes along). Preferences: JavaScript on, `mediaTypesRequiringUserActionForPlayback = none` so
`<video autoplay>` plays, `allowsBackForwardNavigationGestures` on (the two-finger swipe),
`developerExtrasEnabled` on so the right-click menu offers Inspect Element (the Web Inspector is
WebKit's own window), `allowsInlineMediaPlayback` is a given on macOS. Picture in Picture and
AirPlay are left on. The `applicationNameForUserAgent` is `Arkos IDE/<version>` appended to
WebKit's default user agent.

Closing the tab drops the entity: the view is removed from its superview, `stopLoading` is sent,
and media stops with the view. `Closed::Browser { url }` keeps the URL for Cmd+Shift+T, which
reopens the tab on that URL (a fresh view; nothing of the old page survives, as a reopened video
starts from its saved position but a fresh player).

Memory: WebKit's content processes are the system's and outside the Arkos budget. The window
holds at most one `WKWebView` per web tab, and none for a tab that has never been shown (the
entity is made on first draw). A web tab in a hidden pane keeps its view and its page; the
process budget is WebKit's to manage, and it suspends background content processes itself.

## 4. The bar

A 28 px strip above the page (the video strip's height), drawn by gpui in `chrome.banner` with a
`chrome.border` hairline under it, always shown. Left to right:

- **Back** and **forward**: 24 px glyph buttons, dim when `canGoBack` or `canGoForward` is false
  (read each frame the tab is visible). Cmd+[ and Cmd+] while the tab's pane is focused, and the
  trackpad's swipe (WebKit's own).
- **Reload** or, while loading, **stop**: one 24 px button whose glyph is the state. Cmd+R while
  the pane is focused (the keymap's "reload page", bound only in the browser context). A reload
  of a `file` page reads the file again; a reload of a page that failed retries it.
- **The address field**: the existing `line_edit` with the URL as text, selected whole on focus
  (a click or Cmd+L while the pane is focused). Enter loads what was typed under the rules of
  section 5.1; Escape puts the page's URL back and returns focus to the page. While loading the
  field shows the URL being loaded; a committed navigation replaces the text unless the field is
  being edited.
- **The progress hairline**: 2 px along the bottom edge of the bar in `chrome.focus_ring`, its
  width `estimatedProgress` of the bar's width, read each frame while loading (the view asks for
  animation frames while `isLoading`, as the video view does while playing, and none otherwise);
  it disappears when the load ends. No easing; the value is WebKit's. Reduce Motion changes
  nothing (there is no motion of Arkos's own).
- **Open in your browser**: a 24 px glyph button at the right end, `cx.open_url(url)`, for a page
  that wants a real browser (a login that needs a passkey, a download). Cmd+Shift+O while the
  pane is focused.

Zoom: Cmd+plus and Cmd+minus while the pane is focused step `pageZoom` through the Safari ladder
(0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2); Cmd+0 resets. The existing "font bigger" and
"font smaller" commands do this in the browser context and keep their terminal meaning
elsewhere. Zoom is per tab and not saved.

A blank tab (empty `url`) shows the bar with the address field focused and, below, the pane's
background with one dim line, "Type an address, or `arkos open page.html` from a session", in the
pane's `text_dim`. A failed load (section 5.4) shows the reason in the same place. In both the web
view is hidden; a page that has loaded once stays on screen through a later failed navigation
(WebKit keeps the old page, and the bar shows the error as a notice instead).

## 5. Loading rules

### 5.1 What an address means

Typed in the field or given to `arkos open`:

- `http://` or `https://`: loaded as is.
- `file://`: loaded as a local page (5.2).
- A path (absolute, or relative to the shell's folder for `arkos open`; the field takes absolute
  paths only) that names a file: a local page.
- Anything else from the field: `localhost`, `127.0.0.1`, `[::1]`, `*.local` and a bare host
  with a port get `http://`; a word with a dot and no spaces gets `https://`; the rest is
  refused in a notice, "not an address". The field never searches the web.
- Other schemes (`mailto:`, `x-apple…:`, `vscode:`) are refused: a page that navigates to one
  gets `cx.open_url` of it instead (the system's handler opens), and the typed field says "opened
  outside".

The pure function `web::parse_address(text, cwd) -> Result<Url, String>` carries these rules and
their tests.

### 5.2 Local pages

A `file` URL is loaded with `loadFileURL:allowingReadAccessToURL:`, granting read access to the
**enclosing folder chosen by this rule**: the window's project folder when the file is inside
it, else the file's own folder. That is what lets `out/report.html` show `out/clips/a.mp4` and
`../assets/logo.png` within the project, and keeps a page in `/tmp` from reading the project.
A page outside both is a page: WebKit's file-URL sandbox refuses the read and the element shows
broken, as Safari would. A local page's `fetch` of another local file is subject to WebKit's
same-origin rule for files (each file its own origin); `<video src>`, `<img src>` and
`<link>`/`<script src>` are not, and they are what a generated report uses.

### 5.3 Media

`<video>` and `<audio>` play through WebKit's media stack: the system's hardware decoders for
H.264, HEVC and ProRes, AAC and the rest, with no user gesture needed for autoplay (3.3). A page
that plays sound plays it; the mute is the page's own control or the system's. A `.mp4` given to
`arkos open` is still a video tab (that command's existing rule; a web tab is for HTML). WebM
and VP9 play if the macOS version's WebKit does (Sonoma's does for VP9 and Opus in WebM); nothing
in Arkos adds a decoder.

### 5.4 Errors

A navigation that fails (`didFailProvisionalNavigation`, `didFailNavigation`, or a web content
process that terminated) puts the view in `Failed { reason }`: the bar stays, the page area shows
"Could not load" with the URL, the `NSError`'s localized description (a connection refused on
`localhost:4173` reads "Could not connect to the server", which is what an agent whose dev server
is not up yet needs to see) and a Retry button, and the web view is hidden. A cancelled navigation
(the user typed a new address mid-load, a page redirected) is not a failure. A content process
crash reloads the page once on its own; a second crash within a minute shows the error and waits
for Retry.

The load state is read by polling `isLoading`, `estimatedProgress`, `URL` and `title` each frame
while the tab is visible, plus the delegate callbacks for the three failures and `didCommit`
(which is when the tab's `url` is replaced and saved). The delegate is the one objc2
`define_class!` of this milestone, `ArkosNavigationDelegate`, holding a `Weak` to the view's
state cell; its methods write a small `enum Signal` into that cell and nothing else, read by the
next poll on the UI thread. No gpui call is made from inside a WebKit callback.

### 5.5 New windows, downloads, other schemes

A page that opens a new window (`window.open`, `target="_blank"`, a Cmd+click on a link) opens the
request in the same web tab: the `WKUIDelegate`'s `createWebViewWithConfiguration:` returns `nil`
after loading the request in the asking view. (One page per tab, ruling 3; a second Arkos tab for
it is cheap to add later and the ruling says not yet.)

A response WebKit cannot show (`canShowMIMEType` false: a zip, a dmg, an octet stream) is
cancelled, and the bar shows a notice, "downloads are not supported here; open in your browser"
with the button lit. A PDF is shown (WebKit shows PDFs).

JavaScript dialogs (`alert`, `confirm`, `prompt`) are answered by WebKit's own sheets on the
Arkos window (the default `WKUIDelegate` behaviour when the delegate does not implement them; the
delegate leaves them alone). `beforeunload` is the same.

Network: `Info.plist` gains `NSAppTransportSecurity` with `NSAllowsArbitraryLoadsInWebContent`
true, so an `http://` page on a LAN host loads (loopback is already allowed by ATS). Nothing of
Arkos's own uses this exception; it applies to web content only.

### 5.6 Private data

The data store is the default persistent one (3.3). Cookies and site data persist across
relaunches, in the app's container, and are never read by Arkos. Nothing of the page (its URL, its
title, its contents) is sent anywhere by Arkos (section 11 names the counters: kinds and counts,
never addresses). Quitting Arkos leaves the store as a browser would. A "clear web data" command
is not in this milestone (the folder can be deleted; the guide says where).

## 6. The tab follows its file

A web tab on a `file` page watches it the way a video tab watches its file (video spec, section 4):
the view polls the file's modification time and size once a second while the tab is visible and
once every five seconds while it is not, and reloads the page when they changed and then held
still for one more poll (the stability rule, so a file a generator is writing is never loaded
half way). A file that vanishes leaves the page as it is (the old page stays on screen; a
reload would fail) and reloads when it comes back. A reload keeps nothing of the page's scroll:
WebKit's reload restores scroll for the same URL by itself in most cases, and the spec promises
nothing more. An `http(s)` page is never polled; the agent's `arkos open` of the same URL
reloads it (section 8).

## 7. The window's ways in

- **Cmd+Shift+B, "New browser tab…"**: the existing URL prompt, kept, with the field empty (the
  `[browser] url` config, when set, is the prompt's placeholder and what an empty answer opens;
  with no config an empty answer opens a blank tab). The prompt's "single page" checkbox goes
  with app mode.
- **The palette**: "New browser tab…" stays; "New single-page browser tab…", "Dock a Chrome
  window…" and "Undock Chrome window" go.
- **The sidebar**: a click on a `.html` or `.htm` file opens it as a web tab in the focused pane,
  previewed on a single click and pinned on a double click as every file is (`sidebar::file_tab`
  decides the tab's kind by the name, as it does for a video; ruling 16).
- **The markdown editor**: a link to a local `.html` file Cmd+clicked opens a web tab beside the
  document, as a poster opens a video tab; an `http(s)` link in the editor still opens in the
  default browser (a doc's links are to the web; the user has a browser; unchanged).
- **A terminal's link**: Cmd+click opens the default browser, unchanged. The pane is for pages an
  agent wants shown, and a terminal link is a user's wish to read something.

## 8. `arkos open <url | file.html>`

`arkos open` takes a web address or an HTML file in the forms it already has:

```
arkos open http://localhost:4173/ [--pane N | --split right|down|left|up] [--new] [--focus] [--window PID] [--json]
arkos open out/report.html        [the same]
```

- A `http://` or `https://` address, or a `file://` URL, is a web tab. An `.html` or `.htm` file
  is a web tab on its `file://` URL (absolute, symlinks resolved, as every path the command
  sends). The refusal "a web address is not a file; open it in the browser pane (Cmd+Shift+B)"
  goes.
- The preview pane rule, `--pane`, `--split`, `--focus` and `--window` are the video tab's,
  unchanged. `--at`, `--paused`, `--playing`, `--loop`, `--raw` and `--wait` are refused with
  a web target (exit 2, "`--at` is for a video").
- **Reuse**: a web tab already on the same URL (string equality after the command's
  normalisation; `http://localhost:4173` and `http://localhost:4173/` are the same, a fragment
  is not compared) is made active and **reloaded**, and the answer says `reused: true,
  reloaded: true`. This is the agent's "I changed the page, show it again" with no new verb.
  `--new` opens another tab on it instead. A tab the user navigated away from is not on that
  URL and is not reused.
- A missing `.html` file is refused, "no such file: {path}" (no `--wait`: write the page first).
- The answer's `kind` is `"browser"`, `video` is null, and `path` in `arkos panes` is the file
  for a local page and absent for an `http(s)` one; `arkos panes` gains `url` on a browser tab
  row (the current URL) in both the line and the JSON.
- The window's web tab answers "opened" when the tab exists and the load has been asked for, not
  when it finished: a page's load may take as long as it takes (a dev server that compiles on
  first request). The `--json` answer carries `"web": { "url", "state": "loading" | "ready" |
  "failed", "error"? }` so an agent that wants to wait can poll with `arkos panes --json` or a
  second `arkos open`. (A `arkos web status` verb is section 12's.)

Protocol: `OpenKind::Web` is added at the end of the enum, `PaneOutcome::Opened` gains `web:
Option<WebState>`, `TabRow` gains `url: Option<String>`, and `PROTOCOL_VERSION` becomes 9 with
`WEB_OPEN_FROM: u32 = 9`. The daemon refuses an `Open` whose kind is `Web` aimed at a window
below it with the existing "window {pid}: too old for this command" (the check it makes today
for `DIRECT_FROM`, with the kind looked at). The daemon and every window move together through
the update path, as 0.4.0 and 0.6.0 did.

Discovery follows the video pane's section 10: the hook line, the skill description and
`arkos --help` name a web page among what `arkos open` shows; the guide's section 3 gets the
rules above, its refusals list loses the web-address line, and its examples gain
`arkos open http://localhost:4173/` and `arkos open out/report.html`.

## 9. Removals

Gone, with nothing left behind:

- `crates/arkos/src/ax.rs` and `dock.rs`, the `ApplicationServices` link, the `arkos
  browser-probe` subcommand, `Root.{ax, docks, dock_timer, chrome_windows, dock_choices,
  browser_drop, browser_drag_in, browser_drag_out, tab_drag_browser, last_snapshot, ax_prompted,
  vertical_tabs_tip_seen}`, `dock_tick`, `apply_actions`, `undock_tab`, `browser_clicked`, the
  20 Hz timer. `Root.ns_window` stays (the web views need it) and moves to a small
  `screen.rs` with `window_hidden`, `ns_window_of` and the content view's height; `drag.rs`'s
  `zone_and_pane_for` over `ax::Rect` (the Chrome drag-in) goes with its test.
- The commands `DockChromeWindow`, `DockChromeWindowAt`, `UndockChromeWindow`,
  `NewBrowserTabSinglePage`, their palette labels, their keymap rows and their telemetry names.
  A user's `keybindings` that names one of the three gone actions is **ignored with a notice**
  ("`dock a chrome window` is no longer a command"), not an error that blocks the config: the
  keymap's unknown-name path already reports and goes on (to confirm in Task 2; if it errors,
  this milestone makes it a notice).
- `[browser] app_mode` in the config: the field stays in `BrowserConfig`, documented as ignored
  since 0.7, because `Config` is `deny_unknown_fields` and a 0.6 config with the key must still
  read. `[browser] url` keeps its meaning (section 7).
- The doctor's "Accessibility for browser docking" row and its button. The Accessibility
  permission is no longer asked for by anything in Arkos; the guide's permissions paragraph says
  only that the protected-folder prompt remains.
- The shoot: `chrome = true` on a shot, the Chrome opening and window-list code in
  `crates/shoot/src/main.rs`, `Story.chrome`, and the `.chrome.png` overlay. A story's `browser =
  "<url>"` pane stays and now means a web tab on that URL, which the shoot opens through the
  workspace it writes; the web developer scene needs no Chrome and its stills need a reshoot
  (section 10).
- The guide's "Browser pane" section, the README's "docked Chrome pane" line and the hero image's
  alt text, `docs/development.md`'s Accessibility notes and Status rows, the design spec's
  section 8 (replaced by a dated pointer to this spec), the browser-pane plan's README promises.
- The `vertical tabs` tip, the slim tab bar, `Tab::Browser.app_mode`, `Closed::Browser.app_mode`,
  `browser_titles` (the title is the view's), and the `objc2-app-kit` features only `ax.rs` used
  (`NSRunningApplication`, `NSEvent`), if the compiler agrees.

Kept: `telemetry`'s `chrome_missing` kind, which is the mermaid renderer's (it still draws
diagrams with headless Chrome; this milestone does not touch that).

## 10. Documentation

- `docs/guide.md`: the "Browser pane" section rewritten for the web pane: what a web tab is, the
  bar, the ways in, local pages and what they may read, media, the follow rule, where the web
  data lives, the keys (Cmd+Shift+B, Cmd+L, Cmd+R, Cmd+[ and ], Cmd+Shift+O, zoom, Escape), and
  what is saved.
- `docs/agent-guide.md`: section 3 as in section 8 above; the `arkos open` usage line; the JSON
  shapes; two examples; the "What you can change" paragraph names a web page.
- `README.md`: "a docked Chrome pane" becomes "a web pane"; the hero and web-developer alt texts
  say "a web page in a pane". The stills are reshot by the shoot workflow (cloud, dark only, as
  the memory rule says) after this merges; the README's images are not edited by hand.
- `docs/development.md`: the Releasing note about Accessibility goes; Status rows (section 13).
- `docs/superpowers/specs/2026-10-13-arkos-design.md`: section 8's body replaced with "Superseded
  2026-10-24 by `2026-10-24-web-pane-design.md`" and one paragraph of why; section 1's
  "no WKWebView" gets a dated amendment.
- The guide's permissions paragraph and the telemetry page's counter list (section 11).

## 11. Telemetry

Counters, in the allow-list of `crates/telemetry/src/lib.rs`, never carrying an address:
`pane_open{kind=browser, how=palette|key|click|explorer|link|restore|agent}` (existing kind, the
`how` values existing), `pane_close{kind=browser}` (existing), `web_load{outcome=ready|failed,
scheme=http|https|file}` (new), `agent_command{command=open_web}` (new value in the existing
command enum). The disclosure doc and its test list the new names; the `dock_chrome_window*` and
`new_browser_tab_single_page` command names go from the enum and the doc.

## 12. Hooks for the next iteration (built later, named now)

So the agent-driven iteration does not have to reopen this design: `WebView` keeps its
`WKWebView` reachable (`web.evaluate_js(source) -> Task<Result<String>>` and
`web.snapshot(rect) -> Task<Result<PngBytes>>` are the two calls it needs, both thin over
`evaluateJavaScript:completionHandler:` and `takeSnapshotWithConfiguration:completionHandler:`,
neither built now); the targeted channel carries a `PaneAction::Web { path_or_url, op }` the way
it carries `Video`; the counters have a `web_` prefix; and the `Web` trait is where a fake answers
the tests. Verbs the next spec will weigh: `arkos web status|reload|eval|shot|click|type`.

## 13. Tests and Status

Headless (`cargo test --workspace`), written before the code, named as sentences, nothing
constructs a `WKWebView`:

- `web::parse_address`: every rule of 5.1 (a path, a bare host with a port, `localhost`, a dotted
  word, a scheme Arkos refuses, junk).
- `web::read_access_root`: inside the project, outside it, a symlink resolved.
- `WebView` over `FakeWeb`: the state machine (blank, loading with progress, ready with a title,
  failed with a reason, a cancelled navigation that is not a failure, a crash that reloads once
  then fails), the follow rule (changed then still, vanished then back), the title rule, the URL
  committed and the save asked for, zoom steps, the `canGoBack` mirror.
- `screen::flip`: a gpui rect to an AppKit frame against a content height, flipped and not.
- `Root::covered`: each overlay alone covers, none open does not.
- `tree`: a 0.6 `Browser` tab with `url: null` and `app_mode` reads as a blank tab; one with a URL
  keeps it; the workspace round trip.
- `direct`: `plan_open` reuses a tab on the same normalised URL and not one on another; `new_tab`
  for `OpenKind::Web`; the refusal of `--at` with a web target; `panes_report` carries `url`.
- `open_cli::classify_open`: an `http` address, a `file://` URL, an `.html` file, a relative
  `.html` file, a missing one, `.mp4` still a video.
- Protocol: `OpenKind::Web` sits at the end, `PROTOCOL_VERSION` is 9, `WEB_OPEN_FROM` is 9, the
  daemon's refusal for an old window with a web open.
- Keymap: the gone names are reported, not fatal; "reload page", "page back", "page forward",
  "focus address" and "open in browser" exist in the browser context only.
- Telemetry: the new counter names are in the allow-list and the disclosure.

Status rows (what only a person at the screen can check), replacing the five docking rows:

- Cmd+Shift+B, type `localhost:4173`, Enter: the page is drawn inside the pane with the bar
  above it; split the pane, drag the handle, toggle the sidebar, move and resize the window: the
  page follows with no gap and no flash
- switch to another tab in the pane and back: the page is where it was, scrolled as it was; open
  the palette over a web tab: the palette is on top of the page; Escape: the page is back
- `arkos open out/report.html` from a session, with `<video>` elements pointing at files beside
  it: the videos play with sound; edit the file: the page reloads within two seconds
- click into a page's text field and type: the page gets it; Cmd+1: the first pane takes focus;
  Escape from the page: the tab bar's keys work again
- `arkos open http://localhost:1/`: the pane says "Could not connect to the server" with Retry;
  start the server, Retry: the page
- right-click a page, Inspect Element: the Web Inspector opens as its own window
- quit and relaunch: the web tab comes back on the page it was on, not the one it opened with
- a page that calls `window.open`: it opens in the same tab; back returns
- a page with a `<a download>` link: the notice says downloads are not supported, and "open in
  your browser" opens it in the default browser
- Reduce Motion on: nothing of the web pane changes (there was no motion)

## 14. The probe (Task 1 of the plan)

Three facts were checked on the machine with a throwaway `arkos web-probe` subcommand (deleted
once rulings 7 to 9 were written; the real pane answered them in the end) that opens one gpui window with one `WKWebView` child on
`https://example.com` and prints what it saw, then is deleted in the same task:

1. A `WKWebView` added as a subview of gpui's content view draws above gpui's Metal layer, and
   `setHidden` hides it without a flash.
2. Whether the content view is flipped (`isFlipped`), so the frame arithmetic is right.
3. With the web view first responder, Cmd+W reaches gpui's keymap and Cmd+C reaches the page.

The answers are rulings 7 to 9, written into this spec by the implementer.

## 15. Rulings (check these while reading)

1. WKWebView through `objc2-web-kit` 0.3.2 directly, on the objc2 0.3.2 line the video pane
   already uses; not `wry` (it would bring its own event handling and a second way of talking to
   AppKit for one view).
2. One persistent default data store for the app, apart from Safari and Chrome; no profile
   switch; no sharing of the user's logins.
3. One page per Arkos tab; `window.open` loads in the same tab.
4. The address field never searches; a non-address is refused.
5. The tab persists its current URL and nothing else of the page.
6. Switching tabs hides the view and does not pause its media; closing the tab stops it.
7. (probe, 2026-10-24) Stacking: a `WKWebView` added to the content view after gpui's view draws
   above gpui's Metal layer and follows the pane through splits, drags and window moves;
   `setHidden` hides it with no flash. It also covers the pane's focus glow, so the view is
   inset by the glow's reach on its three free sides (`Root::place_web`).
8. (probe, 2026-10-24) Flipped: the content view is not flipped; `appkit_frame` takes the flag
   from `isFlipped` either way.
9. (probe, 2026-10-24) Key equivalents do not reach gpui while the page is first responder:
   Cmd+W, Cmd+B and Cmd+Shift+B did nothing with a page focused. The fallback named in 3.2 is
   built: `screen::install_key_monitor` is a local `NSEvent` monitor that, for a key press with
   Cmd held while a page has the keys, gives the keys back to gpui's view before dispatch
   unless the chord is the page's own (Cmd+C, V, X, A, Z). The page loses the keyboard on such a
   chord, which is the accepted seam. The probe subcommand is deleted.
10. A local page may read its project, or its own folder outside one; never more.
11. Downloads are refused with a pointer to the default browser; a PDF is shown.
12. `arkos open` on a URL already shown reuses and reloads the tab; `--new` makes another.
13. (2026-10-28, found by sampling a window that typed every key twice) A web view removed from
    the window while it has the keys leaves the window as its own first responder. AppKit then
    sends each key through `-[NSWindow keyDown:]`, which runs the view hierarchy's
    `performKeyEquivalent:` a second time, and gpui's view hears every key twice: its guard
    against a duplicate covers only the key down that follows a key equivalent. Closing a web
    tab after a click into its page did this. Two things now stop it: the host hands the keys
    to gpui's view before the view leaves (`wk_web_view::Host::drop`), and the key monitor
    gives lost keys back to gpui's view on the first key pressed whatever took them
    (`screen::keys_return`). The 2026-10-26 guard that dropped a key event with the same
    timestamp as the last one was built on a wrong reading of this and never fired; it is gone.
13. `[browser] app_mode` stays in the config struct, ignored, so 0.6 configs read.
14. Agent control of the page is the next milestone; this one leaves the two calls it needs
    unbuilt and named.
15. Only key equivalents reach gpui while a page has the keyboard, so there is no Escape rule;
    a click outside the page, or Cmd+L, takes the keys back.
16. An `.html` file opens as a web tab from the sidebar on a single click too (a preview), the
    way a video does; the file's source is a text file the viewer shows when opened as text.
17. Retired keybinding names are ignored silently by `validate` (`keymap::RETIRED`), not
    reported: a report would need a notice path that `Config::load` does not have, and the
    names mean nothing any more.
