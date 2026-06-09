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
/// Marker classification: task (agent must act) vs. context (supplementary info).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// A normal instruction the agent must complete.
    Task,
    /// Extra context delimited by slashes (`rik: /some note/`).
    /// Should be removed from the file after work completes.
    Context,
}

/// A discovered marker with its position, kind, and extracted query text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundMarker {
    pub start_line: usize,
    pub end_line: usize,
    pub kind: MarkerKind,
    pub query: String,
    pub prefix: String,
}

/// Check whether a stopper appears within this marker's own line span.
pub fn is_stopped(content: &str, alias: &str, marker: &FoundMarker) -> bool {
    let stop_tags = [format!("!{alias}"), format!("{alias}!")];
    content
        .lines()
        .skip(marker.start_line.saturating_sub(1))
        .take(marker.end_line.saturating_sub(marker.start_line) + 1)
        .any(|line| stop_tags.iter().any(|tag| line.contains(tag)))
}

/// Check if a raw query string is a slash-delimited context annotation.
/// Matches `/.../` where the content starts and ends with `/`.
fn is_context_query(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.starts_with('/') && trimmed.ends_with('/') && trimmed.len() >= 2
}

/// Extract the inner text of a context query (strip surrounding slashes only).
fn context_inner(query: &str) -> String {
    let trimmed = query.trim();
    // Strip exactly one leading and one trailing character (the slashes).
    trimmed[1..trimmed.len() - 1].to_string()
}

/// Find all markers matching the given alias prefix in content.
///
/// Single-line: `{alias}: <query>` -- everything after the prefix on the same line.
///   If `<query>` starts and ends with `/` it is classified as `Context`.
///
/// Multi-line: `{alias}: <open>` starts a block, `{alias}: <close>` ends it.
/// Supported delimiter pairs: `[`/`]`, `[[`/`]]`, `[[[`/`]]]`,
/// `(`/`)`, `((`/`))`, `(((`/`)))`, `{`/`}`, `{{`/`}}`, `{{{`/`}}}`.
/// Content between open and close is the query, with leading whitespace stripped
/// per line and blank lines preserved. Multi-line markers are always `Task`.
///
/// Returns `Vec<FoundMarker>` where line numbers are 1-based.
/// For single-line markers `start_line == end_line`. For multi-line markers
/// `end_line` is the closing delimiter line.
pub fn find_markers(content: &str, alias: &str) -> Vec<FoundMarker> {
    let prefixes = [format!("{alias}:"), format!("{alias}?:")];
    let lines: Vec<&str> = content.lines().collect();
    let mut markers = Vec::new();
    let mut i = 0;

    'lines: while i < lines.len() {
        let line = lines[i];
        for prefix in prefixes.iter() {
            if let Some(pos) = line.find(prefix) {
                let after = line[pos + prefix.len()..].trim();

                if let Some((open, close)) = match_opening_delimiter(after) {
                    // Multi-line marker: collect lines until closing delimiter.
                    // For single-char delimiters we track nesting depth so that
                    // balanced pairs inside the body don't prematurely close the block.
                    let start_line = i + 1; // 1-based
                    let is_single_char = open.len() == 1;
                    let open_ch = if is_single_char {
                        Some(open.as_bytes()[0])
                    } else {
                        None
                    };
                    let close_ch = if is_single_char {
                        Some(close.as_bytes()[0])
                    } else {
                        None
                    };

                    let mut inner_lines: Vec<String> = Vec::new();
                    let mut found_close = false;
                    let mut depth: usize = 1; // started at depth 1 from the opening line

                    let mut j = i + 1;
                    while j < lines.len() {
                        let content_line = lines[j];

                        // Count how many times the exact close delimiter appears on this line.
                        // For multi-char delimiters ("[[", "[[[") we do an exact line match.
                        // For single-char delimiters we scan the line character-by-character.
                        let line_closes = if is_single_char {
                            // Scan for open/close chars; skip quoted strings roughly.
                            let mut local_depth_delta: isize = 0;
                            let mut in_single_quote = false;
                            let mut in_double_quote = false;
                            for b in content_line.bytes() {
                                match b {
                                    b'\\' if in_single_quote || in_double_quote => {
                                        // skip next char inside quotes
                                        continue;
                                    }
                                    b'\'' if !in_double_quote => {
                                        in_single_quote = !in_single_quote;
                                        continue;
                                    }
                                    b'"' if !in_single_quote => {
                                        in_double_quote = !in_double_quote;
                                        continue;
                                    }
                                    _ => {}
                                }
                                if !in_single_quote && !in_double_quote {
                                    if Some(b) == open_ch {
                                        local_depth_delta += 1;
                                    } else if Some(b) == close_ch {
                                        local_depth_delta -= 1;
                                    }
                                }
                            }
                            local_depth_delta
                        } else {
                            // Multi-char delimiter: check if the trimmed line is exactly the close token.
                            // We need to count occurrences for cases like "]] ]]" but keep it simple:
                            // atomic match — trimmed line equals close string counts as 1 close.
                            if content_line.trim() == close {
                                -1
                            } else if content_line.trim() == open {
                                1
                            } else {
                                0
                            }
                        };

                        if is_single_char {
                            depth = if line_closes >= 0 {
                                depth.saturating_add(line_closes as usize)
                            } else {
                                depth.saturating_sub((-line_closes) as usize)
                            };
                            if depth == 0 {
                                found_close = true;
                                break;
                            }
                            // Don't add the line if it was purely the closing bracket
                            // (depth went to 0). Otherwise include it.
                            if depth > 0 {
                                inner_lines.push(content_line.trim_start().to_string());
                            }
                        } else if line_closes < 0 {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                found_close = true;
                                break;
                            }
                        } else if line_closes > 0 {
                            depth = depth.saturating_add(line_closes as usize);
                            inner_lines.push(content_line.trim_start().to_string());
                        } else {
                            inner_lines.push(content_line.trim_start().to_string());
                        }
                        j += 1;
                    }

                    if found_close && !inner_lines.is_empty() {
                        let end_line = j + 1; // 1-based, closing delimiter line
                        markers.push(FoundMarker {
                            start_line,
                            end_line,
                            kind: MarkerKind::Task,
                            query: inner_lines.join("\n"),
                            prefix: prefix.clone(),
                        });
                        i = j + 1; // skip past closing line
                        continue 'lines;
                    } else {
                        // Mismatched/unclosed delimiter -- treat opening line as single-line marker.
                        let kind = if is_context_query(after) {
                            MarkerKind::Context
                        } else {
                            MarkerKind::Task
                        };
                        markers.push(FoundMarker {
                            start_line,
                            end_line: start_line,
                            kind,
                            query: if kind == MarkerKind::Context {
                                context_inner(after)
                            } else {
                                after.to_string()
                            },
                            prefix: prefix.clone(),
                        });
                        i += 1;
                        continue 'lines;
                    }
                } else if !after.is_empty() {
                    // Single-line marker — classify as Context or Task.
                    let kind = if is_context_query(after) {
                        MarkerKind::Context
                    } else {
                        MarkerKind::Task
                    };
                    markers.push(FoundMarker {
                        start_line: i + 1,
                        end_line: i + 1,
                        kind,
                        query: if kind == MarkerKind::Context {
                            context_inner(after)
                        } else {
                            after.to_string()
                        },
                        prefix: prefix.clone(),
                    });
                }
            }
        }
        i += 1;
    }

    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand to build a `FoundMarker` tuple assertion target.
    fn mk(start: usize, end: usize, kind: MarkerKind, query: &str) -> FoundMarker {
        FoundMarker {
            start_line: start,
            end_line: end,
            kind,
            query: query.to_string(),
            prefix: "rik:".to_string(),
        }
    }

    fn task(start: usize, end: usize, query: &str) -> FoundMarker {
        mk(start, end, MarkerKind::Task, query)
    }

    fn ctx(start: usize, end: usize, query: &str) -> FoundMarker {
        mk(start, end, MarkerKind::Context, query)
    }

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
        assert_eq!(markers[0], task(1, 1, "first query"));
        assert_eq!(markers[1], task(3, 3, "second query]"));
        assert_eq!(markers[2], task(5, 5, "third}"));
        assert_eq!(markers[3], task(6, 6, "fourth\";"));
    }

    #[test]
    fn test_find_markers_empty_query_ignored() {
        let content = "rik:\nsome text\nrik:   \n";
        let markers = find_markers(content, "rik");
        assert!(markers.is_empty());
    }

    #[test]
    fn test_stoppers_apply_only_to_their_marker() {
        let content = "!rik: stopped\nrik: active\nrik: stopped rik!";
        let markers = find_markers(content, "rik");

        assert!(is_stopped(content, "rik", &markers[0]));
        assert!(!is_stopped(content, "rik", &markers[1]));
        assert!(is_stopped(content, "rik", &markers[2]));
    }

    #[test]
    fn test_standalone_stopper_does_not_stop_other_markers() {
        let content = "!rik\nrik: active";
        let marker = &find_markers(content, "rik")[0];

        assert!(!is_stopped(content, "rik", marker));
    }

    #[test]
    fn test_stopper_inside_multiline_marker_stops_it() {
        let content = "rik: [[\nwork\n!rik\n]]\nseparator\nrik: active";
        let markers = find_markers(content, "rik");

        assert!(is_stopped(content, "rik", &markers[0]));
        assert!(!is_stopped(content, "rik", &markers[1]));
    }

    // -----------------------------------------------------------------------
    // Context marker tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_context_slash_delimited() {
        let content = "rik: /this is background info/\ncode here\nrik: do something";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0], ctx(1, 1, "this is background info"));
        assert_eq!(markers[1], task(3, 3, "do something"));
    }

    #[test]
    fn test_context_with_spaces_inside() {
        let content = "rik: / leading space preserved /";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], ctx(1, 1, " leading space preserved "));
    }

    #[test]
    fn test_context_not_false_positive_path() {
        // A single slash or unbalanced slashes should NOT be context.
        let content = "rik: fix /usr/bin/foo\nrik: bar/baz";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].kind, MarkerKind::Task);
        assert_eq!(markers[1].kind, MarkerKind::Task);
    }

    #[test]
    fn test_context_bare_slashes_only() {
        // "/ /" is valid context with empty-ish inner text.
        let content = "rik: / /";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, MarkerKind::Context);
    }

    #[test]
    fn test_context_mixed_with_tasks() {
        let content = "rik: /see README.md for conventions/\nrik: add auth middleware\nrik: /use bcrypt not md5/";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0], ctx(1, 1, "see README.md for conventions"));
        assert_eq!(markers[1], task(2, 2, "add auth middleware"));
        assert_eq!(markers[2], ctx(3, 3, "use bcrypt not md5"));
    }

    // -----------------------------------------------------------------------
    // Multiline instruction parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_find_markers_multiline_double_brackets() {
        let content = "before\nrik: [[\nA\nB\nC\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 6, "A\nB\nC"));
    }

    #[test]
    fn test_find_markers_multiline_parens() {
        let content = "before\nrik: (\nA\nB\nC\n)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 6, "A\nB\nC"));
    }

    #[test]
    fn test_find_markers_multiline_curly_braces() {
        let content = "before\nrik: {{\nA\nB\nC\n}}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 6, "A\nB\nC"));
    }

    #[test]
    fn test_find_markers_multiline_triple_brackets() {
        let content = "before\nrik: [[[\nline1\nline2\n]]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 5, "line1\nline2"));
    }

    #[test]
    fn test_find_markers_multiline_triple_parens() {
        let content = "before\nrik: (((\nfoo\nbar\nbaz\n)))\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 6, "foo\nbar\nbaz"));
    }

    #[test]
    fn test_find_markers_multiline_triple_curly() {
        let content = "before\nrik: {{{\nhello\nworld\n}}}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 5, "hello\nworld"));
    }

    #[test]
    fn test_find_markers_multiline_single_bracket() {
        let content = "before\nrik: [\nsingle line inside\n]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 4, "single line inside"));
    }

    #[test]
    fn test_find_markers_multiline_single_paren() {
        let content = "before\nrik: (\nsingle paren content\n)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 4, "single paren content"));
    }

    #[test]
    fn test_find_markers_multiline_single_curly() {
        let content = "before\nrik: {\ncurly content here\n}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 4, "curly content here"));
    }

    #[test]
    fn test_find_markers_mismatched_delimiters_not_multiline() {
        // Open with [[ close with )) - mismatch, should not match as multiline
        let content = "rik: [[\nmismatched\n))";
        let markers = find_markers(content, "rik");
        // Should fall back to single-line behavior or not match multiline
        assert!(!markers.is_empty());
    }

    #[test]
    fn test_find_markers_multiple_multiline_in_file() {
        let content =
            "start\nrik: [[\nfirst A\nfirst B\n]]\nmiddle\nrik: ((\nsecond X\nsecond Y\n))\nend";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0], task(2, 5, "first A\nfirst B"));
        assert_eq!(markers[1], task(7, 10, "second X\nsecond Y"));
    }

    #[test]
    fn test_find_markers_multiline_with_leading_whitespace() {
        let content = "before\nrik: [[\n  indented A\n  indented B\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 5, "indented A\nindented B"));
    }

    #[test]
    fn test_find_markers_multiline_preserves_blank_lines_inside() {
        let content = "before\nrik: [[\nA\n\nC\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 6, "A\n\nC"));
    }

    #[test]
    fn test_find_markers_multiline_closing_bare_delimiter() {
        // Closing delimiter is bare, no alias prefix
        let content = "before\nrik: [[\ncontent line\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 4, "content line"));
    }

    #[test]
    fn test_find_markers_multiline_surrounding_context_preserved() {
        let content = "line before\nrik: [[\ninstruction body\n]]\nline after";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(2, 4, "instruction body"));
    }

    #[test]
    fn test_find_markers_single_and_multiline_mixed() {
        let content = "rik: simple query\nrik: [[\nmulti\nline\n]]\nrik: another simple";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0], task(1, 1, "simple query"));
        assert_eq!(markers[1], task(2, 5, "multi\nline"));
        assert_eq!(markers[2], task(6, 6, "another simple"));
    }

    // -----------------------------------------------------------------------
    // Nested balancing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_nested_parens_balanced_ignored() {
        // Single-char delimiters track nesting; inner () are ignored when balanced.
        let content = "rik: (\nvar = (2+2)*2\n)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "var = (2+2)*2"));
    }

    #[test]
    fn test_nested_brackets_balanced_ignored() {
        let content = "rik: [\ndata = [1, [2,3]]\n]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "data = [1, [2,3]]"));
    }

    #[test]
    fn test_nested_curly_balanced_ignored() {
        let content = "rik: {\nobj = {{a: 1}}\n}\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "obj = {{a: 1}}"));
    }

    #[test]
    fn test_multi_char_no_nesting() {
        // Multi-char delimiters like [[ do NOT do character-level nesting.
        // The whole trimmed line must match the delimiter atomically.
        let content = "rik: [[\n[[ [ [ hello ] ] ]]\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "[[ [ [ hello ] ] ]]"));
    }

    #[test]
    fn test_atomic_marker_space_separated_not_match() {
        // "[ [" is not the same as "[["
        let content = "rik: [[\nprint: [ [\n]]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "print: [ ["));
    }

    #[test]
    fn test_balanced_inner_parens_kept_open() {
        // Inner balanced parens keep the block open; only net-close brings depth to 0.
        let content = "rik: (\nx = (a + b) * (c + d)\n)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "x = (a + b) * (c + d)"));
    }

    #[test]
    fn test_deeply_nested_single_char() {
        let content = "rik: [\n[[[deep]]]\n]\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        // [[[deep]]] has 3 opens and 3 closes (delta=0), then ] on next line closes
        assert_eq!(markers[0], task(1, 3, "[[[deep]]]"));
    }

    #[test]
    fn test_quotes_ignored_in_nesting() {
        // Parentheses inside quoted strings are ignored for nesting count.
        let content = "rik: (\nlet s = \"(hello)\"\n)\nafter";
        let markers = find_markers(content, "rik");
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0], task(1, 3, "let s = \"(hello)\""));
    }
}
