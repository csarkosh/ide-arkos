//! Size marks: how a session's replay remembers the terminal sizes its bytes were written for.
//!
//! A replay is the raw output of a session, and a program writes that output for the size its
//! terminal had at the time: a row is as wide as the terminal was, and a redraw erases as many
//! rows as it had. Parsed at any other size the rows land in the wrong cells. So the daemon
//! writes a mark into the replay at every resize, and a window that parses a replay starts at the
//! size the first bytes were written for and resizes its grid at each mark, as the window that
//! was open at the time did.
//!
//! A mark is an operating system command no terminal acts on, `OSC 7711 ; size ; ... BEL`, so a
//! window older than the marks ignores it. It is written between two reads of the PTY, which may
//! be the middle of one of the program's own sequences: a parser of a replay takes the marks out
//! (`pieces`) and never shows them to the terminal.

/// A terminal size, rows first, as the PTY takes it.
pub type Size = (u16, u16);

const HEAD: &[u8] = b"\x1b]7711;size;";
const END: u8 = 0x07;
/// Four numbers of at most five digits and three semicolons: no mark's body is longer.
const BODY_MAX: usize = 4 * 5 + 3;

/// The most rows and columns a mark may name. A mark is in the replay beside the program's own
/// output, so a program can print one that the daemon never wrote, and a window that believed
/// any size would make a grid of it: 65,535 columns across a whole history is gigabytes, at
/// every relaunch. No pane is this large (a 6K display at the smallest text is under 900
/// columns), so a mark beyond it is not one of ours and stays output, which a terminal ignores.
/// A forged mark within it can only misplace the replayed rows of the session that printed it.
pub const MAX_ROWS: u16 = 1000;
pub const MAX_COLS: u16 = 1000;

/// The mark for a resize from `from` to `to`.
pub fn resize_mark(from: Size, to: Size) -> Vec<u8> {
    let mut m = HEAD.to_vec();
    m.extend_from_slice(format!("{};{};{};{}", from.0, from.1, to.0, to.1).as_bytes());
    m.push(END);
    m
}

/// The mark that closes a replay: the size the session has now, for a replay with no resize in
/// it.
pub fn now_mark(size: Size) -> Vec<u8> {
    let mut m = HEAD.to_vec();
    m.extend_from_slice(format!("{};{}", size.0, size.1).as_bytes());
    m.push(END);
    m
}

/// One stretch of a replay: output to parse, or a resize to make before the output after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    Output(&'a [u8]),
    Resize(Size),
}

/// A replay cut at its marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pieces<'a> {
    /// The size the first output was written for: the first resize's old size, or the closing
    /// mark's size when nothing resized. `None` when the replay has no mark (an older daemon's).
    pub start: Option<Size>,
    pub pieces: Vec<Piece<'a>>,
}

/// Cuts `replay` at its marks. The closing mark names the size the session has now, so it is a
/// resize too (to the size the last output was written for, which changes nothing).
pub fn pieces(replay: &[u8]) -> Pieces<'_> {
    let mut out = Pieces { start: None, pieces: Vec::new() };
    let mut from = 0;
    let mut at = 0;
    while let Some(found) = find(&replay[at..], HEAD).map(|p| p + at) {
        let body_start = found + HEAD.len();
        let window = &replay[body_start..replay.len().min(body_start + BODY_MAX + 1)];
        let parsed = window.iter().position(|&b| b == END).and_then(|len| parse(&window[..len]).map(|sizes| (len, sizes)));
        let Some((len, (old, new))) = parsed else {
            // Not one of ours (or cut short by the end of the replay): it stays output.
            at = body_start;
            continue;
        };
        if found > from {
            out.pieces.push(Piece::Output(&replay[from..found]));
        }
        if out.start.is_none() {
            out.start = Some(old.unwrap_or(new));
        }
        out.pieces.push(Piece::Resize(new));
        from = body_start + len + 1;
        at = from;
    }
    if from < replay.len() {
        out.pieces.push(Piece::Output(&replay[from..]));
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A mark's body: `rows;cols` (the size now) or `rows;cols;rows;cols` (from, to).
fn parse(body: &[u8]) -> Option<(Option<Size>, Size)> {
    let text = std::str::from_utf8(body).ok()?;
    let numbers: Vec<u16> = text.split(';').map(|n| n.parse::<u16>().ok().filter(|&n| n > 0)).collect::<Option<_>>()?;
    // Rows and columns alternate; each is within what a pane can be.
    if numbers.iter().enumerate().any(|(i, &n)| n > if i % 2 == 0 { MAX_ROWS } else { MAX_COLS }) {
        return None;
    }
    match numbers[..] {
        [rows, cols] => Some((None, (rows, cols))),
        [from_rows, from_cols, rows, cols] => Some((Some((from_rows, from_cols)), (rows, cols))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_without_marks_is_one_piece_of_output_at_no_known_size() {
        let p = pieces(b"plain \x1b[1mbold\x1b[0m");
        assert_eq!(p.start, None);
        assert_eq!(p.pieces, vec![Piece::Output(b"plain \x1b[1mbold\x1b[0m")]);
        assert_eq!(pieces(b""), Pieces { start: None, pieces: vec![] });
    }

    #[test]
    fn a_resize_mark_cuts_the_output_and_names_the_size_before_it() {
        let mut replay = b"wide".to_vec();
        replay.extend(resize_mark((24, 80), (24, 60)));
        replay.extend(b"narrow");
        replay.extend(resize_mark((24, 60), (14, 60)));
        replay.extend(now_mark((14, 60)));
        let p = pieces(&replay);
        assert_eq!(p.start, Some((24, 80)));
        assert_eq!(p.pieces, vec![Piece::Output(b"wide"), Piece::Resize((24, 60)), Piece::Output(b"narrow"), Piece::Resize((14, 60)), Piece::Resize((14, 60))]);
    }

    #[test]
    fn the_closing_mark_alone_gives_the_size_of_a_replay_that_never_resized() {
        let mut replay = b"output".to_vec();
        replay.extend(now_mark((30, 100)));
        let p = pieces(&replay);
        assert_eq!(p.start, Some((30, 100)));
        assert_eq!(p.pieces, vec![Piece::Output(b"output"), Piece::Resize((30, 100))]);
    }

    #[test]
    fn a_mark_written_inside_one_of_the_programs_sequences_comes_out_and_the_sequence_is_whole_again() {
        let mut replay = b"\x1b[3".to_vec();
        replay.extend(resize_mark((24, 80), (24, 60)));
        replay.extend(b"1mred");
        let p = pieces(&replay);
        let output: Vec<u8> = p.pieces.iter().filter_map(|p| if let Piece::Output(b) = p { Some(b.to_vec()) } else { None }).flatten().collect();
        assert_eq!(output, b"\x1b[31mred");
    }

    #[test]
    fn a_mark_naming_a_size_no_pane_has_is_not_believed() {
        for forged in [&b"\x1b]7711;size;24;80;60000;80\x07"[..], b"\x1b]7711;size;24;80;24;60000\x07", b"\x1b]7711;size;1001;80\x07", b"\x1b]7711;size;24;1001\x07", b"\x1b]7711;size;65535;65535;24;80\x07"] {
            let p = pieces(forged);
            assert_eq!(p.start, None, "{:?}", String::from_utf8_lossy(forged));
            assert_eq!(p.pieces, vec![Piece::Output(forged)]);
        }
        let largest = resize_mark((24, 80), (MAX_ROWS, MAX_COLS));
        assert_eq!(pieces(&largest).pieces, vec![Piece::Resize((MAX_ROWS, MAX_COLS))]);
    }

    #[test]
    fn what_only_looks_like_a_mark_stays_output() {
        for odd in [&b"\x1b]7711;size;24;80"[..], b"\x1b]7711;size;x;80\x07", b"\x1b]7711;size;0;80\x07", b"\x1b]7711;size;1;2;3\x07", b"\x1b]7711;size;1;2;3;4;5\x07", b"\x1b]7711;size;99999999;80\x07"] {
            let p = pieces(odd);
            assert_eq!(p.start, None, "{:?}", String::from_utf8_lossy(odd));
            assert_eq!(p.pieces, vec![Piece::Output(odd)]);
        }
    }
}
