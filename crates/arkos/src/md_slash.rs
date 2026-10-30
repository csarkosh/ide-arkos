//! The slash menu's catalogue and edits (spec `2026-10-30-slash-menu-design.md`): what typing `/`
//! offers, how the text after it filters the entries, and the markdown each entry writes in place
//! of the slash and that text. Pure, over the document's lines.

/// What an entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Bullet,
    Numbered,
    Check,
    Heading(u8),
    Table(u8, u8),
    Code,
    Quote,
    Alert(&'static str),
    Divider,
    /// An empty diagram, or the template at this index of `MERMAID`.
    Mermaid(Option<usize>),
    Image,
    Footnote,
    Html(Html),
    Equation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Html {
    Div,
    Centered,
    Image,
}

/// One row of the menu: its name, its keywords for the filter, a hint shown at its right, what it
/// does, and its submenu's rows (shown with no text typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub label: String,
    pub keywords: Vec<String>,
    pub hint: Option<&'static str>,
    pub command: Command,
    pub submenu: Vec<Entry>,
}

fn entry(label: impl Into<String>, keywords: &[&str], command: Command) -> Entry {
    Entry { label: label.into(), keywords: keywords.iter().map(|k| k.to_string()).collect(), hint: None, command, submenu: Vec::new() }
}

/// The Mermaid templates: the submenu's name, the keywords after "mermaid diagram chart", and the
/// diagram.
pub const MERMAID: [(&str, &[&str], &str); 18] = [
    ("Flowchart", &["flowchart"], "flowchart TD\n    A[Start] --> B{Is it working?}\n    B -->|Yes| C[Ship it]\n    B -->|No| D[Fix it]\n    D --> B"),
    ("Sequence", &["sequence"], "sequenceDiagram\n    Alice->>Bob: Hello Bob\n    Bob-->>Alice: Hi Alice"),
    ("Class", &["class"], "classDiagram\n    Animal <|-- Dog\n    Animal : +String name\n    Animal : +eat()\n    Dog : +bark()"),
    ("State", &["state"], "stateDiagram-v2\n    [*] --> Draft\n    Draft --> Review\n    Review --> Published\n    Published --> [*]"),
    ("Entity Relationship", &["entity", "relationship"], "erDiagram\n    CUSTOMER ||--o{ ORDER : places\n    ORDER ||--|{ LINE_ITEM : contains"),
    ("User Journey", &["user", "journey"], "journey\n    title A working day\n    section Morning\n      Coffee: 5: Me\n      Email: 2: Me\n    section Afternoon\n      Deep work: 4: Me"),
    ("Gantt", &["gantt"], "gantt\n    title A plan\n    dateFormat YYYY-MM-DD\n    section Build\n    Design    :a1, 2026-11-02, 5d\n    Implement :after a1, 10d"),
    ("Pie", &["pie"], "pie title Pie Chart Template\n    \"A\" : 386\n    \"B\" : 85\n    \"C\" : 15"),
    ("Quadrant", &["quadrant"], "quadrantChart\n    title Reach and engagement\n    x-axis Low Reach --> High Reach\n    y-axis Low Engagement --> High Engagement\n    quadrant-1 Expand\n    quadrant-2 Promote\n    quadrant-3 Re-evaluate\n    quadrant-4 Improve\n    Campaign A: [0.3, 0.6]\n    Campaign B: [0.45, 0.23]"),
    ("Requirement", &["requirement"], "requirementDiagram\n    requirement fast_load {\n      id: 1\n      text: Pages load in under a second\n      risk: medium\n      verifymethod: test\n    }\n    element web_app {\n      type: service\n    }\n    web_app - satisfies -> fast_load"),
    ("Git Graph", &["git", "graph"], "gitGraph\n    commit\n    branch feature\n    checkout feature\n    commit\n    checkout main\n    merge feature"),
    ("Mindmap", &["mindmap", "map"], "mindmap\n  root((Project))\n    Goals\n      Ship\n    Risks\n      Time"),
    ("Timeline", &["timeline"], "timeline\n    title A year\n    2025 : Started\n    2026 : Shipped"),
    ("XY Chart", &["xy", "bar", "line"], "xychart-beta\n    title \"Sales\"\n    x-axis [jan, feb, mar, apr]\n    y-axis \"Revenue\" 0 --> 100\n    bar [20, 45, 60, 80]\n    line [20, 45, 60, 80]"),
    ("Block Diagram", &["block"], "block-beta\n  columns 3\n  a[\"Input\"] b[\"Process\"] c[\"Output\"]"),
    ("Packet", &["packet", "network"], "packet-beta\n  0-15: \"Source Port\"\n  16-31: \"Destination Port\"\n  32-63: \"Sequence Number\""),
    ("Kanban", &["kanban", "board"], "kanban\n  Todo\n    [Write the spec]\n  Doing\n    [Build it]\n  Done\n    [Ship it]"),
    ("Architecture", &["architecture"], "architecture-beta\n    group api(cloud)[API]\n    service db(database)[Database] in api\n    service server(server)[Server] in api\n    db:L -- R:server"),
];

const ALERTS: [(&str, &str); 5] = [("Note", "NOTE"), ("Tip", "TIP"), ("Important", "IMPORTANT"), ("Warning", "WARNING"), ("Caution", "CAUTION")];

fn lists() -> Vec<Entry> {
    vec![
        entry("Bulleted List", &["bulleted", "unordered list", "ul"], Command::Bullet),
        entry("Numbered List", &["numbered list", "ordered list", "ol"], Command::Numbered),
        entry("Check List", &["check", "todo", "checklist"], Command::Check),
    ]
}

fn headings() -> Vec<Entry> {
    (1..=6u8).map(|n| entry(format!("Heading {n}"), &["heading", "header", &format!("h{n}"), &format!("heading{n}"), &format!("header{n}")], Command::Heading(n))).collect()
}

fn alerts(prefix: &str) -> Vec<Entry> {
    ALERTS.iter().map(|(name, tag)| entry(format!("{prefix}{name}"), &["alert", "admonition", &name.to_lowercase()], Command::Alert(tag))).collect()
}

fn mermaids(prefix: &str) -> Vec<Entry> {
    MERMAID
        .iter()
        .enumerate()
        .map(|(i, (name, kw, _))| {
            let mut keywords = vec!["mermaid", "diagram", "chart"];
            keywords.extend_from_slice(kw);
            entry(format!("{prefix}{name}"), &keywords, Command::Mermaid(Some(i)))
        })
        .collect()
}

fn htmls(prefix: &str) -> Vec<Entry> {
    vec![
        entry(format!("{prefix}Basic Div"), &["html", "div"], Command::Html(Html::Div)),
        entry(format!("{prefix}Center Aligned Div"), &["html", "div", "center", "aligned"], Command::Html(Html::Centered)),
        entry(format!("{prefix}Resized Image"), &["html", "image", "resize"], Command::Html(Html::Image)),
    ]
}

fn table() -> Entry {
    Entry { hint: Some("/mxn to set size"), ..entry("Table", &["table", "grid", "spreadsheet"], Command::Table(2, 2)) }
}

fn tail() -> Vec<Entry> {
    vec![
        entry("Divider", &["horizontal rule", "divider", "hr"], Command::Divider),
        entry("Image", &["image", "picture", "img"], Command::Image),
        entry("Footnote", &["footnote", "citation", "reference"], Command::Footnote),
    ]
}

/// The top level, shown with nothing typed after the slash.
pub fn top_level() -> Vec<Entry> {
    let mut out = lists();
    out.push(Entry { hint: Some("/h{1,6} to set level"), submenu: headings(), ..entry("Heading", &["heading", "header"], Command::Heading(1)) });
    out.push(table());
    out.push(entry("Code Block", &["code", "javascript", "python", "js", "codeblock"], Command::Code));
    out.push(Entry { submenu: alerts(""), ..entry("Quote Block", &["block quote", "quote"], Command::Quote) });
    out.push(tail()[0].clone());
    out.push(Entry { submenu: mermaids(""), ..entry("Mermaid Diagram", &["mermaid", "diagram", "chart"], Command::Mermaid(None)) });
    out.extend(tail().into_iter().skip(1));
    out.push(Entry { submenu: htmls(""), ..entry("HTML Block", &["html"], Command::Html(Html::Div)) });
    out.push(entry("Equation Block", &["equation", "math", "latex", "katex", "formula"], Command::Equation));
    out
}

/// Every entry a filter looks through, under their full names, in Mark Sharp's order.
fn flat() -> Vec<Entry> {
    let mut out = lists();
    out.extend(headings());
    out.push(table());
    out.push(entry("Code Block", &["code", "javascript", "python", "js", "codeblock"], Command::Code));
    out.push(entry("Quote Block", &["block quote", "quote"], Command::Quote));
    out.extend(alerts("Alert - "));
    out.extend(mermaids("Mermaid Diagram - "));
    out.extend(tail());
    out.extend(htmls("HTML Block - "));
    out.push(entry("Equation Block", &["equation", "math", "latex", "katex", "formula"], Command::Equation));
    out
}

/// The rows for the text typed after the slash: the top level for none; otherwise table sizes for
/// `3x4` (one) or `3` (3x1 to 3x5), then every entry whose name or keywords hold the text.
pub fn filter(query: &str) -> Vec<Entry> {
    if query.is_empty() {
        return top_level();
    }
    let q = query.to_lowercase();
    let size = |s: &str| s.parse::<u8>().ok().filter(|n| (1..=10).contains(n));
    let mut out = Vec::new();
    match q.split_once('x') {
        Some((m, n)) => {
            if let (Some(m), Some(n)) = (size(m), size(n)) {
                out.push(entry(format!("{m}x{n} Table"), &[], Command::Table(m, n)));
            } else if let (Some(m), "") = (size(m), n) {
                out.extend((1..=5).map(|n| entry(format!("{m}x{n} Table"), &[], Command::Table(m, n))));
            }
        }
        None => {
            if let Some(m) = size(&q) {
                out.extend((1..=5).map(|n| entry(format!("{m}x{n} Table"), &[], Command::Table(m, n))));
            }
        }
    }
    out.extend(flat().into_iter().filter(|e| e.label.to_lowercase().contains(&q) || e.keywords.iter().any(|k| k.contains(&q))));
    out
}

/// An edit of the document: lines `from.0..=to.0` from column `from.1` to `to.1` (characters)
/// become `text`; the caret goes to `caret` (line, column) in the document after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashEdit {
    pub from: (usize, usize),
    pub to: (usize, usize),
    pub text: String,
    pub caret: (usize, usize),
}

/// Where the caret goes in a snippet.
const CARET: char = '\u{1}';

fn table_text(rows: u8, cols: u8) -> String {
    let cols = cols.max(1) as usize;
    let header: Vec<String> = (1..=cols).map(|i| if i == 1 { format!("{CARET}Column 1") } else { format!("Column {i}") }).collect();
    let mut out = format!("| {} |\n|{}|", header.join(" | "), vec![" --- "; cols].join("|"));
    for _ in 1..rows.max(1) {
        out.push_str(&format!("\n|{}|", vec!["  "; cols].join("|")));
    }
    out
}

/// The block an entry writes, with `CARET` where the caret goes.
fn block_text(cmd: Command) -> Option<String> {
    Some(match cmd {
        Command::Table(m, n) => table_text(m, n),
        Command::Code => format!("```\n{CARET}\n```"),
        Command::Equation => format!("$$\n{CARET}\n$$"),
        Command::Quote => format!("> {CARET}"),
        Command::Alert(tag) => format!("> [!{tag}]\n> {CARET}"),
        Command::Divider => format!("---\n\n{CARET}"),
        Command::Mermaid(None) => format!("```mermaid\n{CARET}\n```"),
        Command::Mermaid(Some(i)) => format!("```mermaid\n{}\n```\n\n{CARET}", MERMAID.get(i)?.2),
        Command::Html(Html::Div) => format!("<div>\n{CARET}\n</div>"),
        Command::Html(Html::Centered) => format!("<div align=\"center\">\n{CARET}\n</div>"),
        Command::Html(Html::Image) => format!("<img src=\"{CARET}\" width=\"300\">"),
        _ => return None,
    })
}

/// A line with its list marker or heading hashes made the entry's: a list item's marker (and task
/// box) replaced, heading hashes replaced, or the marker put after the line's indent and quote marks.
/// Returns the new line and how many characters the marker part grew by.
fn reformat(line: &str, cmd: Command) -> (String, isize) {
    let lead = line.len() - line.trim_start_matches([' ', '>']).len();
    let body = &line[lead..];
    let old = crate::md_blocks::list_item_columns(line).map(|(_, content)| {
        let mut end = line.char_indices().nth(content).map_or(line.len(), |(b, _)| b);
        if line[end..].starts_with("[ ] ") || line[end..].starts_with("[x] ") || line[end..].starts_with("[X] ") {
            end += 4;
        }
        end - lead
    });
    let old = old.unwrap_or_else(|| {
        let hashes = body.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && body[hashes..].starts_with(' ') {
            hashes + 1
        } else {
            0
        }
    });
    let marker = match cmd {
        Command::Bullet => "- ".to_string(),
        Command::Numbered => "1. ".to_string(),
        Command::Check => "- [ ] ".to_string(),
        Command::Heading(n) => format!("{} ", "#".repeat(n.clamp(1, 6) as usize)),
        _ => String::new(),
    };
    let out = format!("{}{marker}{}", &line[..lead], &body[old..]);
    (out, marker.chars().count() as isize - body[..old].chars().count() as isize)
}

/// The highest footnote number in the document, plus one.
fn next_footnote(lines: &[String]) -> usize {
    let mut max = 0;
    for l in lines {
        let mut rest = l.as_str();
        while let Some(i) = rest.find("[^") {
            rest = &rest[i + 2..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if rest[digits.len()..].starts_with(']') {
                max = max.max(digits.parse().unwrap_or(0));
            }
        }
    }
    max + 1
}

/// The (line, column) of `CARET` in `text` written from (`line`, `col`), and the text without it.
fn place(text: &str, line: usize, col: usize) -> (String, (usize, usize)) {
    let at = text.find(CARET).unwrap_or(text.len());
    let before = &text[..at];
    let caret = match before.rfind('\n') {
        Some(i) => (line + before.matches('\n').count(), before[i + 1..].chars().count()),
        None => (line, col + before.chars().count()),
    };
    (text.replace(CARET, ""), caret)
}

/// The edit an entry makes for a slash at column `slash` of line `line` with the caret at column
/// `end` (the text between is the query). `None` for Image, whose path the caller asks for
/// (`image_edit`).
pub fn apply(cmd: Command, lines: &[String], line: usize, slash: usize, end: usize) -> Option<SlashEdit> {
    let cur = lines.get(line)?;
    let chars: Vec<char> = cur.chars().collect();
    let (slash, end) = (slash.min(chars.len()), end.clamp(slash.min(chars.len()), chars.len()));
    let before: String = chars[..slash].iter().collect();
    let after: String = chars[end..].iter().collect();
    let stripped = format!("{before}{after}");
    let whole = ((line, 0), (line, chars.len()));
    match cmd {
        Command::Bullet | Command::Numbered | Command::Check | Command::Heading(_) => {
            let (text, grew) = reformat(&stripped, cmd);
            let col = (slash as isize + grew).max(0) as usize;
            Some(SlashEdit { from: whole.0, to: whole.1, text, caret: (line, col) })
        }
        Command::Footnote => {
            let n = next_footnote(lines);
            let last = lines.len() - 1;
            let tail: Vec<&str> = lines[line + 1..].iter().map(String::as_str).collect();
            let mut text = format!("{before}[^{n}]{after}");
            if !tail.is_empty() {
                text.push('\n');
                text.push_str(&tail.join("\n"));
            }
            let ends_blank = text.ends_with('\n') || text.lines().last().is_some_and(|l| l.trim().is_empty());
            text.push_str(if ends_blank { "\n" } else { "\n\n" });
            text.push_str(&format!("[^{n}]: {CARET}"));
            let (text, caret) = place(&text, line, 0);
            Some(SlashEdit { from: (line, 0), to: (last, lines[last].chars().count()), text, caret })
        }
        Command::Image => None,
        _ => {
            let block = block_text(cmd)?;
            let blank = |n: Option<&String>| n.is_none_or(|l| l.trim().is_empty());
            let next_gap = if blank(lines.get(line + 1)) { "" } else { "\n" };
            let text = if stripped.trim().is_empty() {
                let prev_gap = if line == 0 || blank(lines.get(line - 1)) { "" } else { "\n" };
                format!("{prev_gap}{block}{next_gap}")
            } else {
                format!("{stripped}\n\n{block}{next_gap}")
            };
            let (text, caret) = place(&text, line, 0);
            Some(SlashEdit { from: whole.0, to: whole.1, text, caret })
        }
    }
}

/// The edit for an image chosen in the picker: `![name](path)` in place of the slash and its query.
pub fn image_edit(lines: &[String], line: usize, slash: usize, end: usize, name: &str, path: &str) -> Option<SlashEdit> {
    let cur = lines.get(line)?;
    let n = cur.chars().count();
    let path = if path.contains([' ', '(', ')']) { format!("<{path}>") } else { path.to_string() };
    let text = format!("![{name}]({path})");
    let col = slash + text.chars().count();
    Some(SlashEdit { from: (line, slash.min(n)), to: (line, end.min(n)), text, caret: (line, col) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.label.as_str()).collect()
    }

    fn lines(s: &str) -> Vec<String> {
        s.split('\n').map(String::from).collect()
    }

    /// The document after `cmd` for the slash at `slash` and the caret at `end` on `line`.
    fn run(doc: &str, line: usize, slash: usize, end: usize, cmd: Command) -> (Vec<String>, (usize, usize)) {
        let ls = lines(doc);
        let e = apply(cmd, &ls, line, slash, end).expect("an edit");
        let mut out: Vec<String> = ls[..e.from.0].to_vec();
        let head: String = ls[e.from.0].chars().take(e.from.1).collect();
        let tail: String = ls[e.to.0].chars().skip(e.to.1).collect();
        let mid = format!("{head}{}{tail}", e.text);
        out.extend(mid.split('\n').map(String::from));
        out.extend(ls[e.to.0 + 1..].iter().cloned());
        (out, e.caret)
    }

    #[test]
    fn the_top_level_is_mark_sharp_s_order_with_its_submenus() {
        let top = top_level();
        assert_eq!(labels(&top), vec!["Bulleted List", "Numbered List", "Check List", "Heading", "Table", "Code Block", "Quote Block", "Divider", "Mermaid Diagram", "Image", "Footnote", "HTML Block", "Equation Block"]);
        let sub = |name: &str| top.iter().find(|e| e.label == name).unwrap().submenu.len();
        assert_eq!((sub("Heading"), sub("Quote Block"), sub("Mermaid Diagram"), sub("HTML Block"), sub("Table")), (6, 5, 18, 3, 0));
        assert_eq!(top[3].hint, Some("/h{1,6} to set level"));
        assert_eq!(top[4].hint, Some("/mxn to set size"));
    }

    #[test]
    fn the_text_after_the_slash_filters_names_and_keywords_and_sizes_tables() {
        assert_eq!(labels(&filter("h2")), vec!["Heading 2"]);
        assert_eq!(labels(&filter("ul"))[0], "Bulleted List", "a plain substring match, as Mark Sharp's; the catalogue's order puts it first");
        assert_eq!(labels(&filter("todo")), vec!["Check List"]);
        assert_eq!(labels(&filter("pie")), vec!["Mermaid Diagram - Pie"]);
        assert_eq!(labels(&filter("HR")), vec!["Divider"], "case aside");
        assert_eq!(labels(&filter("3x4")), vec!["3x4 Table"]);
        assert_eq!(labels(&filter("3")), vec!["3x1 Table", "3x2 Table", "3x3 Table", "3x4 Table", "3x5 Table", "Heading 3"], "the sizes first, then what the text matches");
        assert_eq!(labels(&filter("11x2")), Vec::<&str>::new(), "sizes up to 10");
        assert!(filter("zzz").is_empty());
        assert!(labels(&filter("math")).contains(&"Equation Block"));
        assert!(labels(&filter("citation")).contains(&"Footnote"));
    }

    #[test]
    fn line_formats_apply_to_the_slash_s_line_and_convert_a_list_item() {
        assert_eq!(run("/", 0, 0, 1, Command::Bullet), (lines("- "), (0, 2)));
        assert_eq!(run("text /h2", 0, 5, 8, Command::Heading(2)), (lines("## text "), (0, 8)));
        assert_eq!(run("## old /h3", 0, 7, 10, Command::Heading(3)).0, lines("### old "), "hashes replaced");
        assert_eq!(run("- item /todo", 0, 7, 12, Command::Check).0, lines("- [ ] item "), "a list item converts");
        assert_eq!(run("- [ ] task /ol", 0, 11, 14, Command::Numbered).0, lines("1. task "));
        assert_eq!(run("  - nested /ul", 0, 11, 14, Command::Numbered).0, lines("  1. nested "), "the indent stays");
    }

    #[test]
    fn blocks_take_an_empty_line_or_follow_the_text_with_blank_lines_around() {
        assert_eq!(run("/code", 0, 0, 5, Command::Code), (lines("```\n\n```"), (1, 0)));
        assert_eq!(run("para\n/", 1, 0, 1, Command::Divider), (lines("para\n\n---\n\n"), (4, 0)), "a blank line before, so the paragraph is not a heading");
        assert_eq!(run("text /", 0, 5, 6, Command::Quote), (lines("text \n\n> "), (2, 2)), "after text: a new paragraph");
        assert_eq!(run("/\nnext", 0, 0, 1, Command::Equation), (lines("$$\n\n$$\n\nnext"), (1, 0)), "a blank line after");
        assert_eq!(run("/", 0, 0, 1, Command::Table(3, 2)).0, lines("| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n|  |  |"));
        assert_eq!(run("/", 0, 0, 1, Command::Table(3, 2)).1, (0, 2), "the caret in the first header cell");
        assert_eq!(run("/", 0, 0, 1, Command::Alert("TIP")), (lines("> [!TIP]\n> "), (1, 2)));
        let (doc, caret) = run("/", 0, 0, 1, Command::Mermaid(Some(7)));
        assert_eq!((doc[0].as_str(), doc[1].as_str(), caret), ("```mermaid", "pie title Pie Chart Template", (doc.len() - 1, 0)), "a template: the caret after the block");
        assert_eq!(run("/", 0, 0, 1, Command::Html(Html::Image)), (lines("<img src=\"\" width=\"300\">"), (0, 10)));
    }

    #[test]
    fn a_footnote_takes_the_next_number_and_its_definition_goes_at_the_end() {
        let (doc, caret) = run("one /\n\nsee [^2]\n\n[^2]: two", 0, 4, 5, Command::Footnote);
        assert_eq!(doc, lines("one [^3]\n\nsee [^2]\n\n[^2]: two\n\n[^3]: "));
        assert_eq!(caret, (6, 6));
        assert_eq!(run("a /", 0, 2, 3, Command::Footnote).0, lines("a [^1]\n\n[^1]: "));
    }

    #[test]
    fn an_image_goes_in_place_of_the_slash() {
        let ls = lines("see /img here");
        let e = image_edit(&ls, 0, 4, 8, "cat", "images/my cat.png").unwrap();
        assert_eq!((e.from, e.to, e.text.as_str(), e.caret), ((0, 4), (0, 8), "![cat](<images/my cat.png>)", (0, 31)));
    }
}
