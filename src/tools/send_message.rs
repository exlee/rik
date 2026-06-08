use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// SendMessage tool
// ---------------------------------------------------------------------------

/// Arguments for the send_message tool.
#[derive(Deserialize)]
pub struct SendMessageArgs {
    /// Message to display.
    pub message: String,
}

/// Error type for the send_message tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SendMessageError(String);

/// A tool that displays a message and triggers the keyboard stop flag.
#[derive(Deserialize, Serialize)]
pub struct SendMessageTool;

impl Tool for SendMessageTool {
    const NAME: &'static str = "send_message";

    type Error = SendMessageError;
    type Args = SendMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Display a message and trigger the keyboard stop.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Message to display"
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let output = format!("[send_message] {}", args.message);
        println!("{}", output);
        crate::keyboard::set_soft_stop();
        Ok(output)
    }
}
