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
        let mut line = 0u32;
        let mut character = 0u32;

        // Parser locations are UTF-8 byte offsets, while LSP positions use
        // UTF-16 code units. Stop at a character boundary and clamp offsets
        // beyond EOF so diagnostics never point back to column zero.
        for (byte_index, ch) in text.char_indices() {
            if byte_index >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                character = 0;
            } else {
                character += ch.len_utf16() as u32;
            }
        }

        Position { line, character }
    }

    Range {
        start: offset_to_position(location.start, source),
        end: offset_to_position(location.end, source),
    }
}

#[cfg(test)]
mod tests {
    use psy_common::FileId;

    use super::*;

    #[test]
    fn str_range_uses_grapheme_indices() {
        assert_eq!(str_range("a👨‍👩‍👧‍👦éz", &(1..3)), "👨‍👩‍👧‍👦é");
    }

    #[test]
    fn span_to_range_converts_byte_offsets_to_utf16_positions() {
        let source = "a😀b\n中z";
        let start = source.find('b').unwrap();
        let end = source.find('z').unwrap();
        let range = span_to_range(&Location::new(FileId(0), start, end), source);

        assert_eq!(range.start, Position::new(0, 3));
        assert_eq!(range.end, Position::new(1, 1));
    }

    #[test]
    fn span_to_range_handles_eof_and_clamps_past_eof() {
        let source = "first\nlast";
        let eof = source.len();
        let range = span_to_range(&Location::new(FileId(0), eof, eof + 20), source);

        assert_eq!(range.start, Position::new(1, 4));
        assert_eq!(range.end, Position::new(1, 4));
    }
}
