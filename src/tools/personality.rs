// ---------------------------------------------------------------------------
// Mood enum
// ---------------------------------------------------------------------------

/// Represents the emotional tone to apply to text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    None,
    Singing,
    Anxious,
    Whispering,
    Screaming,
    Angry,
    Grim,
    Tired,
}

// ---------------------------------------------------------------------------
// moodify
// ---------------------------------------------------------------------------

/// Apply a [`Mood`] to `input` and return the transformed string.
pub fn moodify(mood: Mood, input: &str) -> String {
    match mood {
        Mood::None => input.to_string(),
        Mood::Singing => format!("\u{266A} {input} \u{266A}"),
        Mood::Anxious => input
            .split_whitespace()
            .map(|w| format!("...{w}"))
            .collect::<Vec<_>>()
            .join(" "),
        Mood::Whispering => {
            let inner = if input.is_empty() { " " } else { input };
            format!("(...{inner}...)")
        },
        Mood::Screaming => format!("{}!", input.to_uppercase()),
        Mood::Angry => input
            .split_whitespace()
            .map(|w| format!("{w}!"))
            .collect::<Vec<_>>()
            .join(" "),
        Mood::Grim => input
            .split_whitespace()
            .map(|w| format!("{w}."))
            .collect::<Vec<_>>()
            .join(" "),
        Mood::Tired => input
            .split_whitespace()
            .map(|w| format!("{w}..."))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // None — input passes through unchanged
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_none_returns_input_unchanged() {
        assert_eq!(moodify(Mood::None, "hello world"), "hello world");
    }

    #[test]
    fn moodify_none_single_word() {
        assert_eq!(moodify(Mood::None, "rust"), "rust");
    }

    #[test]
    fn moodify_none_empty_string() {
        assert_eq!(moodify(Mood::None, ""), "");
    }

    #[test]
    fn moodify_none_preserves_punctuation() {
        assert_eq!(moodify(Mood::None, "what's up?"), "what's up?");
    }

    // ---------------------------------------------------------------------------
    // Singing — wrap entire string in ♪ … ♪
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_singing_wraps_in_music_notes() {
        assert_eq!(moodify(Mood::Singing, "hello world"), "♪ hello world ♪");
    }

    #[test]
    fn moodify_singing_single_word() {
        assert_eq!(moodify(Mood::Singing, "la la la"), "♪ la la la ♪");
    }

    #[test]
    fn moodify_singing_empty_string() {
        assert_eq!(moodify(Mood::Singing, ""), "♪  ♪");
    }

    #[test]
    fn moodify_singing_long_sentence() {
        assert_eq!(
            moodify(Mood::Singing, "the rain in spain falls mainly on the plain"),
            "♪ the rain in spain falls mainly on the plain ♪"
        );
    }

    // ---------------------------------------------------------------------------
    // Anxious — add "..." before each word
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_anxious_adds_triple_dots_before_each_word() {
        assert_eq!(moodify(Mood::Anxious, "hello world"), "...hello ...world");
    }

    #[test]
    fn moodify_anxious_single_word() {
        assert_eq!(moodify(Mood::Anxious, "help"), "...help");
    }

    #[test]
    fn moodify_anxious_empty_string() {
        assert_eq!(moodify(Mood::Anxious, ""), "");
    }

    #[test]
    fn moodify_anxious_three_words() {
        assert_eq!(moodify(Mood::Anxious, "i am scared"), "...i ...am ...scared");
    }

    // ---------------------------------------------------------------------------
    // Whispering — wrap in (...input...)
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_whispering_wraps_in_parenthesis_with_triple_dots() {
        assert_eq!(moodify(Mood::Whispering, "hello world"), "(...hello world...)");
    }

    #[test]
    fn moodify_whispering_single_word() {
        assert_eq!(moodify(Mood::Whispering, "shhh"), "(...shhh...)");
    }

    #[test]
    fn moodify_whispering_empty_string() {
        assert_eq!(moodify(Mood::Whispering, ""), "(... ...)");
    }

    #[test]
    fn moodify_whispering_with_numbers() {
        assert_eq!(moodify(Mood::Whispering, "call me at 5"), "(...call me at 5...)");
    }

    // ---------------------------------------------------------------------------
    // Screaming — uppercase + trailing exclamation mark
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_screaming_uppercase_and_exclamation() {
        assert_eq!(moodify(Mood::Screaming, "hello world"), "HELLO WORLD!");
    }

    #[test]
    fn moodify_screaming_already_uppercase() {
        assert_eq!(moodify(Mood::Screaming, "STOP IT"), "STOP IT!");
    }

    #[test]
    fn moodify_screaming_mixed_case() {
        assert_eq!(moodify(Mood::Screaming, "It'S CaSe"), "IT'S CASE!");
    }

    #[test]
    fn moodify_screaming_empty_string() {
        assert_eq!(moodify(Mood::Screaming, ""), "!");
    }

    // ---------------------------------------------------------------------------
    // Angry — exclamation after each word
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_angry_exclamation_after_each_word() {
        assert_eq!(moodify(Mood::Angry, "hello world"), "hello! world!");
    }

    #[test]
    fn moodify_angry_single_word() {
        assert_eq!(moodify(Mood::Angry, "nope"), "nope!");
    }

    #[test]
    fn moodify_angry_empty_string() {
        assert_eq!(moodify(Mood::Angry, ""), "");
    }

    #[test]
    fn moodify_angry_four_words() {
        assert_eq!(moodify(Mood::Angry, "get out of here"), "get! out! of! here!");
    }

    // ---------------------------------------------------------------------------
    // Grim — dot after each word
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_grim_dot_after_each_word() {
        assert_eq!(moodify(Mood::Grim, "hello world"), "hello. world.");
    }

    #[test]
    fn moodify_grim_single_word() {
        assert_eq!(moodify(Mood::Grim, "fine"), "fine.");
    }

    #[test]
    fn moodify_grim_empty_string() {
        assert_eq!(moodify(Mood::Grim, ""), "");
    }

    #[test]
    fn moodify_grim_sentence() {
        assert_eq!(moodify(Mood::Grim, "it is what it is"), "it. is. what. it. is.");
    }

    // ---------------------------------------------------------------------------
    // Tired — triple dots after each word
    // ---------------------------------------------------------------------------
    #[test]
    fn moodify_tired_triple_dots_after_each_word() {
        assert_eq!(moodify(Mood::Tired, "hello world"), "hello... world...");
    }

    #[test]
    fn moodify_tired_single_word() {
        assert_eq!(moodify(Mood::Tired, "ugh"), "ugh...");
    }

    #[test]
    fn moodify_tired_empty_string() {
        assert_eq!(moodify(Mood::Tired, ""), "");
    }

    #[test]
    fn moodify_tired_longer_phrase() {
        assert_eq!(
            moodify(Mood::Tired, "i need some sleep now"),
            "i... need... some... sleep... now..."
        );
    }
}
