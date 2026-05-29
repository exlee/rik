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
        Mood::Singing => todo!("apply singing mood"),
        Mood::Anxious => todo!("apply anxious mood"),
        Mood::Whispering => todo!("apply whispering mood"),
        Mood::Screaming => todo!("apply screaming mood"),
        Mood::Angry => todo!("apply angry mood"),
        Mood::Grim => todo!("apply grim mood"),
        Mood::Tired => todo!("apply tired mood"),
    }
}
