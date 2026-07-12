//! Line wrapping + sentence indexing for the Reader line model.

/// A wrapped line of chapter text with its byte range into the source text.
/// (char range so a WordMark's offset maps to a line; the Slint reader renders
/// one row per line and shows the left accent on the current sentence's lines.)
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub width: f32,
}

/// Word-wrap `text` into lines of at most `max_chars` (by char count),
/// preserving word boundaries. Each line carries its byte range.
pub fn lines(text: &str, max_chars: usize) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut line_start: usize = 0;
    let mut line_text = String::new();
    let mut line_chars = 0usize;
    let mut i = 0usize;
    let bytes = text.as_bytes();
    let n = bytes.len();
    while i < n {
        while i < n {
            let ch_len = text[i..].chars().next().map_or(0, |c| c.len_utf8());
            if ch_len == 0 {
                break;
            }
            if text[i..].chars().next().is_some_and(|c| c.is_whitespace()) {
                i += ch_len;
            } else {
                break;
            }
        }
        let word_start = i;
        while i < n {
            let Some(ch) = text[i..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                break;
            }
            i += ch.len_utf8();
        }
        if i == word_start {
            break;
        }
        let word = &text[word_start..i];
        let wc = word.chars().count();
        if !line_text.is_empty() && line_chars + 1 + wc > max_chars {
            out.push(Line {
                text: line_text,
                start: line_start,
                end: word_start,
                width: 0.0,
            });
            line_start = word_start;
            line_text = String::new();
            line_chars = 0;
        }
        if line_text.is_empty() {
            line_text.push_str(word);
            line_chars = wc;
        } else {
            line_text.push(' ');
            line_text.push_str(word);
            line_chars += 1 + wc;
        }
    }
    if !line_text.is_empty() {
        out.push(Line {
            text: line_text,
            start: line_start,
            end: n,
            width: 0.0,
        });
    }
    out
}

/// Sentence index at a byte offset = count of sentence-ending punctuation
/// (`. ! ?`) strictly before that offset. Used to map a line's start (and a
/// WordMark's offset) to a sentence for the left-accent highlight.
pub fn sentence_index_at(text: &str, byte_offset: usize) -> usize {
    let upto = byte_offset.min(text.len());
    text[..upto].matches(['.', '!', '?']).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linebreak_wraps_to_max_chars_with_ranges() {
        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let ls = lines(text, 15);
        for w in &ls {
            assert!(w.text.chars().count() <= 15, "line too long: {:?}", w.text);
        }
        assert_eq!(ls.first().unwrap().start, 0);
        assert_eq!(ls.last().unwrap().end, text.len());
        for w in ls.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        assert_eq!(
            ls.iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            text
        );
    }

    #[test]
    fn sentence_index_advances_on_punctuation() {
        let text = "First sentence. Second one! And a third?";
        assert_eq!(sentence_index_at(text, 0), 0);
        assert_eq!(sentence_index_at(text, text.find("Second").unwrap()), 1);
        assert_eq!(sentence_index_at(text, text.find("And").unwrap()), 2);
        assert_eq!(sentence_index_at(text, text.len()), 3);
    }
}
