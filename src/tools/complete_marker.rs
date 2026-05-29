// ---------------------------------------------------------------------------
// Marker finding utilities
// ---------------------------------------------------------------------------

/// Opening delimiters that start a multi-line marker, sorted longest-first.
/// Each entry is (open, close) pair.
const MULTILINE_DELIMITERS: &[(&str, &str)] = &[
    ("[[[", "]]]"),
    ("(((", ")))"),
    ("{{{", "}}}"),
    ("[[", "]]"),
    ("((", "))"),
    ("{{", "}}"),
    ("[", "]"),
    ("(", ")"),
    ("{", "}"),
];

/// Check if text after `alias:` is a multi-line opening delimiter.
/// Returns (open, close) pair if it is.
fn match_opening_delimiter(text: &str) -> Option<(&'static str, &'static str)> {
    let trimmed = text.trim();
    // Match longest first to avoid e.g. [[[ matching as a single [
    for &(open, close) in MULTILINE_DELIMITERS {
        if trimmed == open {
            return Some((open, close));
        }
    }
    None
}

/// Check if a line is the closing delimiter for a multi-line block.
/// The line should be exactly `alias: close` (with optional whitespace).
fn is_closing_line(line: &str, alias: &str, close: &str) -> bool {
    let prefix = format!("{alias}:");
    if let Some(pos) = line.find(&prefix) {
        let after = line[pos + prefix.len()..].trim();
        // Allow close to have leading whitespace stripped by the trim
        after == close || after == close.trim()
    } else {
        false
    }
}

/// Find all markers matching the given alias prefix in content.
///
/// Single-line: `{alias}: <query>` -- everything after the prefix on the same line.
///
/// Multi-line: `{alias}: <open>` starts a block, `{alias}: <close>` ends it.
/// Supported delimiter pairs: `[`/`]`, `[[`/`]]`, `[[[`/`]]]`,
/// `(`/`)`, `((`/`))`, `(((`/`)))`, `{`/`}`, `{{`/`}}`, `{{{`/`}}}`.
/// Content between open and close is the query, with leading whitespace stripped
/// per line and blank lines preserved.
///
/// Returns `Vec<(start_line, end_line, query)>` where line numbers are 1-based.
/// For single-line markers `start_line == end_line`. For multi-line markers
/// `end_line` is the closing delimiter line.
pub fn find_markers(content: &str, alias: &str) -> Vec<(usize, usize, String)> {
    let prefix = format!("{alias}:");
    let lines: Vec<&str> = content.lines().collect();
    let mut markers = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if let Some(pos) = line.find(&prefix) {
            let after = line[pos + prefix.len()..].trim();

            if let Some((_open, close)) = match_opening_delimiter(after) {
                // Multi-line marker: collect lines until closing delimiter
                let start_line = i + 1; // 1-based
                let mut inner_lines: Vec<String> = Vec::new();
                let mut found_close = false;

                let mut j = i + 1;
                while j < lines.len() {
                    if is_closing_line(lines[j], alias, close) {
                        found_close = true;
                        break;
                    }
                    // Strip leading whitespace from inner lines
                    inner_lines.push(lines[j].trim_start().to_string());
                    j += 1;
                }

                if found_close && !inner_lines.is_empty() {
                    let end_line = j + 1; // 1-based, closing delimiter line
                    markers.push((start_line, end_line, inner_lines.join("\n")));
                    i = j + 1; // skip past closing line
                    continue;
                } else {
                    // Mismatched/unclosed delimiter -- treat opening line as single-line marker
                    markers.push((start_line, start_line, after.to_string()));
                    i += 1;
                    continue;
                }
            } else if !after.is_empty() {
                // Single-line marker
                markers.push((i + 1, i + 1, after.to_string()));
            }
        }
        i += 1;
    }

    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_markers_various_positions() {
        let content = "rik: first query\n\
                       world\n\
                       [rik: second query]\n\
                       rik:\n\
                       {rik: third}\n\
                       const x = \"rik: fourth\";\n\
                       end";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 4);
        assert_eq!(markers[0], (1, 1, "first query".to_string()));
        assert_eq!(markers[1], (3, 3, "second query]".to_string()));
        assert_eq!(markers[2], (5, 5, "third}".to_string()));
        assert_eq!(markers[3], (6, 6, "fourth\";".to_string()));
    }

    #[test]
    fn test_find_markers_empty_query_ignored() {
        let content = "rik:\nsome text\nrik:   \n";
        let markers = find_markers(content, "rik");
        assert!(markers.is_empty());
    }

    // -----------------------------------------------------------------------
    // Multiline instruction parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_markers_multiline_double_brackets() {
        let content = "before\nrik: [[\nA\nB\nC\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_parens() {
        let content = "before\nrik: (\nA\nB\nC\nrik: )\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_curly_braces() {
        let content = "before\nrik: {{\nA\nB\nC\nrik: }}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\nB\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_brackets() {
        let content = "before\nrik: [[[\nline1\nline2\nrik: ]]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "line1\nline2".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_parens() {
        let content = "before\nrik: (((\nfoo\nbar\nbaz\nrik: )))\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "foo\nbar\nbaz".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_triple_curly() {
        let content = "before\nrik: {{{\nhello\nworld\nrik: }}}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "hello\nworld".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_bracket() {
        let content = "before\nrik: [\nsingle line inside\nrik: ]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "single line inside".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_paren() {
        let content = "before\nrik: (\nsingle paren content\nrik:)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "single paren content".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_single_curly() {
        let content = "before\nrik: {\ncurly content here\nrik: }\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "curly content here".to_string()));
    }

    #[test]
    fn test_find_markers_mismatched_delimiters_not_multiline() {
        // Open with [[ close with )) - mismatch, should not match as multiline
        let content = "rik: [[\nmismatched\nrik: ))";
        let markers = find_markers(content, "rik");
        // Should fall back to single-line behavior or not match multiline
        assert!(!markers.is_empty());
    }

    #[test]
    fn test_find_markers_multiple_multiline_in_file() {
        let content = "start\nrik: [[\nfirst A\nfirst B\nrik: ]]\nmiddle\nrik: ((\nsecond X\nsecond Y\nrik: ))\nend";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0], (2, 5, "first A\nfirst B".to_string()));
        assert_eq!(markers[1], (7, 10, "second X\nsecond Y".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_with_leading_whitespace() {
        let content = "before\nrik: [[\n  indented A\n  indented B\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 5, "indented A\nindented B".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_preserves_blank_lines_inside() {
        let content = "before\nrik: [[\nA\n\nC\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 6, "A\n\nC".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_closing_on_separate_line_only_alias_prefix() {
        // The closing delimiter line contains only alias and closing bracket
        let content = "before\nrik: [[\ncontent line\nrik: ]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "content line".to_string()));
    }

    #[test]
    fn test_find_markers_multiline_surrounding_context_preserved() {
        let content = "line before\nrik: [[\ninstruction body\nrik: ]]\nline after";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], (2, 4, "instruction body".to_string()));
    }

    #[test]
    fn test_find_markers_single_and_multiline_mixed() {
        let content = "rik: simple query\nrik: [[\nmulti\nline\nrik: ]]\nrik: another simple";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0], (1, 1, "simple query".to_string()));
        assert_eq!(markers[1], (2, 5, "multi\nline".to_string()));
        assert_eq!(markers[2], (6, 6, "another simple".to_string()));
    }
}
