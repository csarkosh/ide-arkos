#!/usr/bin/env python3
"""Claude Code's own colours, read from its binary, for the contrast matrix.

Writes crates/arkos/data/claude-code-colours.json: the six stock themes (every key and its
value, as Claude Code spells it: rgb(r,g,b), ansi:<name> or ansi256(n)) and every colour its
diff renderer can draw (as #rrggbb, and the palette indexes it uses). The matrix draws them all
on every Arkos theme, so a stock theme's colours stay readable in Arkos.

    python3 -I scripts/claude-code-colours.py            # the installed claude, write the file
    python3 -I scripts/claude-code-colours.py --check    # compare instead; exit 1 when it differs
    python3 -I scripts/claude-code-colours.py --binary <path> [--check]

The binary is minified, so the script finds the tables by what does not change: the stock
themes through the switch that names them (`case"light":return X;`), the diff renderer's
colours between its colour helper (`return{r:e,g:n,b:t,a:255}`) and the parser after it.
"""
import json
import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURE = os.path.join(ROOT, "crates", "arkos", "data", "claude-code-colours.json")
THEMES = ["dark", "light", "dark-daltonized", "light-daltonized", "dark-ansi", "light-ansi"]


def installed_binary():
    claude = shutil.which("claude")
    if not claude:
        sys.exit("claude-code-colours: no claude on PATH; pass --binary <path>")
    return os.path.realpath(claude)


def version_of(text):
    m = re.search(r"// Version: (\d+\.\d+\.\d+)", text)
    return m.group(1) if m else "unknown"


def stock_themes(text):
    m = re.search(r'switch\((\w+)\)\{case"light":return (\w+);case"light-ansi":return (\w+);case"dark-ansi":return (\w+);'
                  r'case"light-daltonized":return (\w+);case"dark-daltonized":return (\w+);default:return (\w+)\}', text)
    if not m:
        sys.exit("claude-code-colours: the stock theme switch was not found; Claude Code changed its theme code")
    names = dict(zip(["light", "light-ansi", "dark-ansi", "light-daltonized", "dark-daltonized", "dark"], m.groups()[1:]))
    before = text[max(0, m.start() - 80000):m.start()]
    themes = {}
    for theme in THEMES:
        var = names[theme]
        at = max(before.rfind(f"var {var}={{"), before.rfind(f",{var}={{"))
        if at < 0:
            sys.exit(f"claude-code-colours: the {theme} table ({var}) was not found")
        body = before[before.index("{", at) + 1:before.index("}", at)]
        themes[theme] = dict(re.findall(r'(\w+):"([^"]*)"', body))
        if "text" not in themes[theme] or "diffAdded" not in themes[theme]:
            sys.exit(f"claude-code-colours: the {theme} table ({var}) has no text or diffAdded key")
    return themes


def diff_colours(text):
    start = text.find("return{r:e,g:n,b:t,a:255}}")
    if start < 0:
        sys.exit("claude-code-colours: the diff renderer's colour helper was not found")
    end = text.find("function D(e){", start)
    if end < 0 or end - start > 20000:
        sys.exit("claude-code-colours: the end of the diff renderer's colours was not found")
    seg = text[start:end]
    rgb = sorted({"#%02x%02x%02x" % tuple(int(v) for v in t) for t in re.findall(r"\bu\((\d+),(\d+),(\d+)\)", seg)})
    indexed = sorted({int(i) for i in re.findall(r"\bx\((\d+)\)", seg)})
    if len(rgb) < 20:
        sys.exit(f"claude-code-colours: only {len(rgb)} diff colours found; Claude Code changed its diff renderer")
    return {"rgb": rgb, "indexed": indexed}


def main(argv):
    check = "--check" in argv
    binary = argv[argv.index("--binary") + 1] if "--binary" in argv else installed_binary()
    with open(binary, "rb") as f:
        text = f.read().decode("latin1")
    found = {"claude_code": version_of(text), "themes": stock_themes(text), "diff": diff_colours(text)}
    rendered = json.dumps(found, indent=2, sort_keys=True) + "\n"
    if check:
        with open(FIXTURE) as f:
            have = json.load(f)
        if {k: v for k, v in have.items() if k != "claude_code"} == {k: v for k, v in found.items() if k != "claude_code"}:
            print(f"claude-code-colours: Claude Code {found['claude_code']} draws the colours of the fixture (from {have['claude_code']})")
            return 0
        print(f"claude-code-colours: Claude Code {found['claude_code']} changed its colours since {have['claude_code']}; "
              "run scripts/claude-code-colours.py, then cargo test")
        return 1
    os.makedirs(os.path.dirname(FIXTURE), exist_ok=True)
    with open(FIXTURE, "w") as f:
        f.write(rendered)
    print(f"claude-code-colours: wrote {os.path.relpath(FIXTURE, ROOT)} from Claude Code {found['claude_code']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
