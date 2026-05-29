use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// Mood enum
// ---------------------------------------------------------------------------

/// Represents the emotional tone to apply to text.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
// Moodify tool
// ---------------------------------------------------------------------------

/// Arguments for the moodify tool.
#[derive(Deserialize)]
pub struct MoodifyArgs {
    /// The mood to apply.
    pub mood: Mood,
    /// The input text.
    pub text: String,
}

/// Error type for the moodify tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MoodifyError(String);

/// A tool that applies a mood to text and returns the transformed string.
#[derive(Deserialize, Serialize)]
pub struct MoodifyTool;

impl Tool for MoodifyTool {
    const NAME: &'static str = "moodify";

    type Error = MoodifyError;
    type Args = MoodifyArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Apply a mood to the given text and return the transformed string."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "mood": {
                        "type": "string",
                        "enum": [
                            "none",
                            "singing",
                            "anxious",
                            "whispering",
                            "screaming",
                            "angry",
                            "grim",
                            "tired"
                        ],
                        "description": "The mood to apply to the text"
                    },
                    "text": {
                        "type": "string",
                        "description": "The text to transform"
                    }
                },
                "required": ["mood", "text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let output = match args.mood {
            Mood::None => args.text.clone(),
            Mood::Singing => todo!("apply singing mood"),
            Mood::Anxious => todo!("apply anxious mood"),
            Mood::Whispering => todo!("apply whispering mood"),
            Mood::Screaming => todo!("apply screaming mood"),
            Mood::Angry => todo!("apply angry mood"),
            Mood::Grim => todo!("apply grim mood"),
            Mood::Tired => todo!("apply tired mood"),
        };
        Ok(output)
    }
}
