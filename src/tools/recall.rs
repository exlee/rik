use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------------------
// Recall tool
// ---------------------------------------------------------------------------

/// Arguments for the recall tool.
#[derive(Deserialize)]
pub struct RecallArgs {
    /// Id of the memory to read, as shown in the History section.
    pub id: usize,
    /// Include the original request.
    #[serde(default)]
    pub request: bool,
    /// Include the model's reasoning from that turn.
    #[serde(default)]
    pub reasoning: bool,
    /// Include what the model said and which tools it ran.
    #[serde(default)]
    pub output: bool,
    /// Include the diff that turn produced.
    #[serde(default)]
    pub diff: bool,
}

/// Error type for the recall tool.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RecallError(String);

/// A tool that reads a remembered turn back in full.
///
/// The History section in the prompt only carries summaries; everything else —
/// the request, the reasoning, the output, the diff — stays in memory until the
/// agent asks for it here.
#[derive(Deserialize, Serialize, Default)]
pub struct RecallTool;

impl Tool for RecallTool {
    const NAME: &'static str = "recall";

    type Error = RecallError;
    type Args = RecallArgs;
    type Output = String;

    fn description(&self) -> String {
        "Read a memory from the History section in full. Give the memory's id and \
         set any of request, reasoning, output, or diff to true. With none set, \
         everything is returned."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Id of the memory, as shown in the History section"
                },
                "request": {
                    "type": "boolean",
                    "description": "Include the original request or question"
                },
                "reasoning": {
                    "type": "boolean",
                    "description": "Include the reasoning from that turn"
                },
                "output": {
                    "type": "boolean",
                    "description": "Include what was said and which tools were run"
                },
                "diff": {
                    "type": "boolean",
                    "description": "Include the diff that turn produced"
                }
            },
            "required": ["id"]
        })
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        crate::memory::recall(
            args.id,
            crate::memory::Recall {
                request: args.request,
                reasoning: args.reasoning,
                output: args.output,
                diff: args.diff,
            },
        )
        .map_err(RecallError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unset_flags_default_to_false_and_unknown_ids_report_back() {
        let args: RecallArgs = serde_json::from_value(json!({"id": 7, "diff": true})).unwrap();

        assert_eq!(args.id, 7);
        assert!(args.diff);
        assert!(!args.reasoning, "unset flags default to false");

        // An id no session could have handed out, so this never races with the
        // memory tests populating the shared store.
        let missing = serde_json::from_value(json!({"id": usize::MAX})).unwrap();
        let error = RecallTool.call(missing).await.unwrap_err().to_string();
        assert!(
            error.contains("Nothing is remembered") || error.contains("No memory"),
            "{error}"
        );
    }

    #[test]
    fn parameters_document_every_flag() {
        let parameters = RecallTool.parameters();
        let properties = parameters["properties"].as_object().unwrap();

        for flag in ["id", "request", "reasoning", "output", "diff"] {
            assert!(properties.contains_key(flag), "missing {flag}");
        }
        assert_eq!(parameters["required"], json!(["id"]));
    }
}
