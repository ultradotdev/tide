use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Extended graphemes shared by viewport and terminal width calculations.
pub fn graphemes(text: &str) -> impl Iterator<Item = &str> {
    text.graphemes(true)
}

/// Terminal display width of a string, using Unicode grapheme clusters.
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_width).sum()
}

/// Terminal display width of one grapheme cluster.
#[must_use]
pub fn grapheme_width(grapheme: &str) -> usize {
    // `UnicodeWidthStr::width` already treats most emoji as width 2. Keep this
    // helper deliberately small so a future width table can replace it without
    // touching the editor.
    grapheme.width()
}

/// Character offsets at grapheme-cluster boundaries, including 0 and `len`.
#[must_use]
pub fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    let mut offset = 0;
    for grapheme in text.graphemes(true) {
        offset += grapheme.chars().count();
        boundaries.push(offset);
    }
    boundaries
}

/// Truncate to at most `max_width` display cells without splitting a grapheme.
#[must_use]
pub fn truncate_to_width(text: &str, max_width: usize) -> &str {
    if max_width == 0 {
        return "";
    }
    let mut width = 0;
    let mut end = 0;
    for grapheme in text.graphemes(true) {
        let next = width + grapheme_width(grapheme);
        if next > max_width {
            break;
        }
        width = next;
        end += grapheme.len();
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_width_is_bytes() {
        assert_eq!(display_width("hello"), 5);
    }

    #[test]
    fn cjk_and_emoji_are_wide() {
        assert_eq!(display_width("漢字"), 4);
        assert_eq!(display_width("👩‍💻"), 2);
    }

    #[test]
    fn combining_marks_do_not_add_cells() {
        assert_eq!(display_width("e\u{0301}"), 1);
    }

    #[test]
    fn boundaries_respect_grapheme_clusters() {
        assert_eq!(grapheme_boundaries("a👩‍💻b"), vec![0, 1, 4, 5]);
    }

    #[test]
    fn truncation_does_not_split_graphemes() {
        assert_eq!(truncate_to_width("a👩‍💻b", 3), "a👩‍💻");
        assert_eq!(truncate_to_width("a👩‍💻b", 1), "a");
    }
}
