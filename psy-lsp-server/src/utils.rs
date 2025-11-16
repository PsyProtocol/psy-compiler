use psy_ast::Location;
use tower_lsp::lsp_types::{Position, Range};
use unicode_segmentation::UnicodeSegmentation;

/// Returns a string from a range of human characters (graphemes). Respects
/// unicode.
pub fn str_range(s: &str, range: &std::ops::Range<usize>) -> String {
    s.graphemes(true).skip(range.start).take(range.len()).collect()
}

pub fn span_to_range(location: &Location, source: &str) -> Range {
    fn offset_to_position(offset: usize, text: &str) -> Position {
        let mut line = 0;
        let mut col = 0;
        let mut current = 0;

        for l in text.lines() {
            let line_len = l.len() + 1; // +1 for newline
            if current + line_len > offset {
                col = offset - current;
                break;
            }
            current += line_len;
            line += 1;
        }

        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    Range {
        start: offset_to_position(location.start, source),
        end: offset_to_position(location.end, source),
    }
}
