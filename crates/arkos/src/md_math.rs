//! Math in the rendered view (diagrams and math spec, section 3): a paragraph that is only
//! `$$ ... $$` is drawn by the bundled MathJax in the window's hidden web view (`render_view`),
//! from a page made here and cached as a diagram's is (`mermaid::prepare`, `mermaid::finish`);
//! `$...$` inside text is drawn as text, its TeX turned into the characters it stands for where
//! Unicode has them (`inline_text`).

use sha2::{Digest, Sha256};
use std::path::Path;

/// The version of the vendored MathJax, in a macro so `SCRIPT_NAME` can be built from it
/// (`crates/arkos/assets/mathjax-tex-svg.js`, from `scripts/vendor-mathjax.sh`).
macro_rules! version {
    () => {
        "3.2.2"
    };
}

/// The name the bundled script is written out under, beside the pages: one file per version.
pub const SCRIPT_NAME: &str = concat!("mathjax-", version!(), "-tex-svg.js");
/// The bundled script.
const SCRIPT: &[u8] = include_bytes!("../assets/mathjax-tex-svg.js");

/// Writes the bundled script into `dir` as `SCRIPT_NAME` unless it is there, through a file of the
/// process's own and a rename, so a page never loads half a script.
pub fn write_script(dir: &Path) -> Result<(), String> {
    let script = dir.join(SCRIPT_NAME);
    if script.is_file() {
        return Ok(());
    }
    let part = dir.join(format!("{SCRIPT_NAME}.{}.part", std::process::id()));
    let written = std::fs::write(&part, SCRIPT).and_then(|()| std::fs::rename(&part, &script)).map_err(|e| format!("cannot write {}: {e}", script.display()));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written
}

/// The cache key of display math `tex` drawn in `color` (`#rrggbb`) at `size` CSS pixels a em.
pub fn cache_key(tex: &str, color: &str, size: u32) -> String {
    let mut hash = Sha256::new();
    for part in ["math", tex, color, &size.to_string()] {
        hash.update(part.as_bytes());
        hash.update(b"\0");
    }
    format!("math-{}", hash.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// A string as a JavaScript string literal that cannot end the script it sits in.
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()).replace("</", "<\\/")
}

/// The page MathJax draws display math `tex` from, in `color` at `size` pixels a em: the same
/// content security policy as a diagram's page (the two inline scripts by their hashes, the bundled
/// script, no network), MathJax set up to draw SVG with its glyphs as paths and nothing typeset on
/// its own, then a script that draws `tex` and puts `ok <w> <h>` in the title, or `error <the
/// message>` for a TeX error.
pub fn page(tex: &str, color: &str, size: u32) -> String {
    let color: String = color.chars().filter(|c| *c == '#' || c.is_ascii_hexdigit()).collect();
    // No context menu and no assistive MathML: the hidden copy MathJax adds for screen readers
    // draws as a stray pixel in a snapshot.
    let config = "window.MathJax = { svg: { fontCache: 'none' }, startup: { typeset: false }, options: { enableMenu: false, menuOptions: { settings: { assistiveMml: false } } } };".to_string();
    let render = format!(
        r#"
(async () => {{
  try {{
    await MathJax.startup.promise;
    const node = await MathJax.tex2svgPromise({src}, {{ display: true }});
    const bad = node.querySelector('[data-mjx-error]');
    if (bad) throw new Error(bad.getAttribute('data-mjx-error'));
    document.body.appendChild(node);
    const r = node.querySelector('svg').getBoundingClientRect();
    const w = Math.max(1, Math.ceil(r.width) + 4), h = Math.max(1, Math.ceil(r.height) + 4);
    document.body.style.width = w + 'px'; document.body.style.height = h + 'px';
    document.title = 'ok ' + w + ' ' + h;
  }} catch (e) {{
    document.title = 'error ' + (String((e && e.message) || e).split('\n')[0] || 'the equation could not be drawn');
  }}
}})();
"#,
        src = js_string(tex)
    );
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src {h1} {h2} file:; style-src 'unsafe-inline'; img-src data:">
<title></title>
<style>html, body {{ margin: 0; padding: 0; background: transparent; color: {color}; font-size: {size}px; }} body {{ padding: 2px; box-sizing: border-box; }} mjx-container {{ margin: 0 !important; display: inline-block !important; }} mjx-assistive-mml {{ display: none !important; }}</style>
<script>{config}</script>
<script src="./{SCRIPT_NAME}"></script>
</head>
<body>
<script>{render}</script>
</body>
</html>
"#,
        h1 = crate::mermaid::script_hash(&config),
        h2 = crate::mermaid::script_hash(&render),
    )
}

/// Inline TeX as text: Greek letters, operators, relations and arrows as their characters,
/// `\sqrt{x}` as `√x`, `\frac{a}{b}` as `a/b`, a single character or a braced group after `^` or
/// `_` raised or lowered when Unicode has every one of its characters so, `\text{...}` and the
/// font commands as their content, braces dropped; anything else as written.
pub fn inline_text(tex: &str) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut i = 0;
    convert(&chars, &mut i, false).trim().to_string()
}

fn convert(c: &[char], i: &mut usize, group: bool) -> String {
    let mut out = String::new();
    while *i < c.len() {
        let ch = c[*i];
        match ch {
            '}' if group => {
                *i += 1;
                return out;
            }
            '{' => {
                *i += 1;
                out.push_str(&convert(c, i, true));
            }
            '\\' => {
                *i += 1;
                let start = *i;
                while *i < c.len() && c[*i].is_ascii_alphabetic() {
                    *i += 1;
                }
                if *i == start {
                    // `\{`, `\%`, `\,` and the like: one character after the backslash.
                    if let Some(&next) = c.get(*i) {
                        *i += 1;
                        match next {
                            ',' | ':' | ';' | ' ' => out.push(' '),
                            '!' => {}
                            _ => out.push(next),
                        }
                    }
                    continue;
                }
                let name: String = c[start..*i].iter().collect();
                match name.as_str() {
                    "sqrt" => {
                        let inner = argument(c, i);
                        if inner.chars().count() > 1 {
                            out.push_str(&format!("√({inner})"));
                        } else {
                            out.push_str(&format!("√{inner}"));
                        }
                    }
                    "frac" | "dfrac" | "tfrac" => {
                        let (a, b) = (argument(c, i), argument(c, i));
                        let wrap = |s: String| if s.chars().count() > 1 { format!("({s})") } else { s };
                        out.push_str(&format!("{}/{}", wrap(a), wrap(b)));
                    }
                    "text" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathcal" | "operatorname" | "textbf" | "textit" | "mbox" => out.push_str(&argument(c, i)),
                    "mathbb" => out.extend(argument(c, i).chars().map(|a| match a {
                        'R' => 'ℝ',
                        'N' => 'ℕ',
                        'Z' => 'ℤ',
                        'Q' => 'ℚ',
                        'C' => 'ℂ',
                        'P' => 'ℙ',
                        a => a,
                    })),
                    "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "limits" => {}
                    "quad" => out.push_str("  "),
                    "qquad" => out.push_str("    "),
                    _ => match symbol(&name) {
                        Some(s) => out.push_str(s),
                        None => {
                            out.push('\\');
                            out.push_str(&name);
                        }
                    },
                }
            }
            '^' | '_' => {
                *i += 1;
                let arg = argument(c, i);
                let mapped: Option<String> = arg.chars().map(|a| if ch == '^' { superscript(a) } else { subscript(a) }).collect();
                match mapped {
                    Some(m) if !arg.is_empty() => out.push_str(&m),
                    _ if arg.chars().count() == 1 => {
                        out.push(ch);
                        out.push_str(&arg);
                    }
                    _ => out.push_str(&format!("{ch}({arg})")),
                }
            }
            _ => {
                *i += 1;
                out.push(ch);
            }
        }
    }
    out
}

/// The argument at `i`: a braced group, converted, or the next character (a command's whole name).
fn argument(c: &[char], i: &mut usize) -> String {
    while *i < c.len() && c[*i] == ' ' {
        *i += 1;
    }
    match c.get(*i) {
        Some('{') => {
            *i += 1;
            convert(c, i, true)
        }
        Some('\\') => {
            let start = *i;
            *i += 1;
            while *i < c.len() && c[*i].is_ascii_alphabetic() {
                *i += 1;
            }
            let mut j = start;
            convert(&c[..*i], &mut j, false)
        }
        Some(&ch) => {
            *i += 1;
            ch.to_string()
        }
        None => String::new(),
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾', 'n' => 'ⁿ', 'i' => 'ⁱ', 'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ',
        'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ', 'j' => 'ʲ', 'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ', 'o' => 'ᵒ',
        'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ', 'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
        '′' => '′', '∗' | '*' => '*',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎', 'a' => 'ₐ', 'e' => 'ₑ', 'o' => 'ₒ', 'x' => 'ₓ', 'h' => 'ₕ',
        'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'p' => 'ₚ', 's' => 'ₛ', 't' => 'ₜ', 'i' => 'ᵢ', 'j' => 'ⱼ', 'r' => 'ᵣ',
        'u' => 'ᵤ', 'v' => 'ᵥ',
        _ => return None,
    })
}

/// A command's character, when it names one.
fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ϵ", "varepsilon" => "ε", "zeta" => "ζ",
        "eta" => "η", "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ", "nu" => "ν",
        "xi" => "ξ", "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "sigma" => "σ", "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ",
        "phi" => "ϕ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "cdot" => "·", "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓", "ast" => "∗", "star" => "⋆", "circ" => "∘", "bullet" => "•",
        "le" | "leq" => "≤", "ge" | "geq" => "≥", "ne" | "neq" => "≠", "approx" => "≈", "equiv" => "≡", "sim" => "∼", "simeq" => "≃",
        "cong" => "≅", "propto" => "∝", "ll" => "≪", "gg" => "≫", "infty" => "∞", "partial" => "∂", "nabla" => "∇",
        "sum" => "∑", "prod" => "∏", "int" => "∫", "oint" => "∮", "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂",
        "supset" => "⊃", "subseteq" => "⊆", "supseteq" => "⊇", "cup" => "∪", "cap" => "∩", "emptyset" | "varnothing" => "∅",
        "forall" => "∀", "exists" => "∃", "neg" | "lnot" => "¬", "land" | "wedge" => "∧", "lor" | "vee" => "∨", "oplus" => "⊕",
        "otimes" => "⊗", "to" | "rightarrow" => "→", "leftarrow" | "gets" => "←", "Rightarrow" | "implies" => "⇒", "Leftarrow" => "⇐",
        "iff" | "Leftrightarrow" => "⇔", "leftrightarrow" => "↔", "mapsto" => "↦", "uparrow" => "↑", "downarrow" => "↓",
        "ldots" | "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱", "prime" => "′", "degree" => "°", "hbar" => "ℏ",
        "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ", "aleph" => "ℵ", "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋",
        "lceil" => "⌈", "rceil" => "⌉", "angle" => "∠", "perp" => "⊥", "parallel" => "∥", "mid" => "∣", "therefore" => "∴",
        "because" => "∵", "cdotp" => "·", "lvert" | "rvert" | "vert" => "|", "Vert" | "lVert" | "rVert" => "‖",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tex_reads_as_the_characters_it_stands_for() {
        assert_eq!(inline_text("E = mc^2"), "E = mc²");
        assert_eq!(inline_text(r"\alpha + \beta \le \gamma"), "α + β ≤ γ");
        assert_eq!(inline_text("x_{i+1} = x_i^{2n}"), "xᵢ₊₁ = xᵢ²ⁿ");
        assert_eq!(inline_text(r"\sqrt{x} + \sqrt{ab}"), "√x + √(ab)");
        assert_eq!(inline_text(r"\frac{1}{2} \cdot \frac{a+b}{c}"), "1/2 · (a+b)/c");
        assert_eq!(inline_text(r"\text{if } x \in \mathbb{R}"), "if  x ∈ ℝ");
        assert_eq!(inline_text(r"\unknown{x}"), r"\unknownx", "an unknown command as written, its group's braces dropped");
        assert_eq!(inline_text("x^{Q}"), "x^Q", "no raised Q: written out");
        assert_eq!(inline_text("x^{QR}"), "x^(QR)", "a group, bracketed");
        assert_eq!(inline_text("a_Q"), "a_Q");
        assert_eq!(inline_text(r"\{a, b\} \to \infty"), "{a, b} → ∞");
        assert_eq!(inline_text(r"\sum_{k=1}^{n} k"), "∑ₖ₌₁ⁿ k");
    }

    #[test]
    fn the_math_page_carries_its_scripts_by_hash_and_the_tex_cannot_end_its_script() {
        let html = page(r"x</script>y", "#f3edff", 18);
        assert!(!html.contains("x</script>"), "the TeX cannot end its script: {html}");
        assert!(html.contains("color: #f3edff; font-size: 18px"));
        assert!(html.contains(&format!("<script src=\"./{SCRIPT_NAME}\">")));
        let scripts: Vec<&str> = html.split("<script>").skip(1).map(|s| s.split("</script>").next().unwrap()).collect();
        assert_eq!(scripts.len(), 2);
        for s in scripts {
            assert!(html.contains(&crate::mermaid::script_hash(s)));
        }
        assert!(!page("x", "#fff'; alert(1)", 18).contains("alert(1)"), "a colour is only its hex");
        assert_ne!(cache_key("x", "#fff", 18), cache_key("x", "#fff", 19));
        assert!(cache_key("x", "#fff", 18).starts_with("math-"));
    }
}
