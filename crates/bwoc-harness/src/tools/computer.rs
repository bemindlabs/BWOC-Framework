//! Computer-use tool — **P0 spike** (Phase 7 candidate).
//!
//! Three things, and nothing more:
//!
//! 1. A backend-neutral [`ComputerAction`] vocabulary (the intersection of the
//!    Anthropic `computer` tool and the OpenAI CUA action set), so one model maps
//!    to either provider.
//! 2. A [`ComputerExecutor`] seam + a [`MockExecutor`] that returns canned
//!    observations — the loop runs end-to-end with **no GUI, no display, no
//!    container**.
//! 3. The Anthropic provider-native tool serialization ([`anthropic_tool_spec`] +
//!    [`ANTHROPIC_COMPUTER_BETA`]) — the architectural fork from a custom function
//!    tool (`{name, description, input_schema}`) to a provider-defined tool keyed
//!    by `type`.
//!
//! Wired into the live loop behind the `browser` feature: [`default_registry`]
//! registers the tool with a lazily-launched headless-browser executor, the
//! Anthropic provider emits the native [`anthropic_tool_spec`] + beta header, and
//! the policy pipeline gates it — `computer` is `Capability::Gated` (refused on
//! untrusted turns), ask-by-default, and autoprocess-refused when there is no
//! TTY. Default builds pull zero browser deps and never register it. The
//! provider layer now serializes image blocks on both paths (see
//! [`crate::provider::types::ChatMessage::with_images`]); wiring a captured
//! screenshot *through the
//! re-exec turn-executor IPC boundary* into that field, plus richer key-chord
//! parsing, remain later slices.
//!
//! [`default_registry`]: super::registry::default_registry

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{ToolContext, ToolImpl};
use crate::error::HarnessError;

// ---------------------------------------------------------------------------
// Neutral action model
// ---------------------------------------------------------------------------

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

/// Scroll direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// A single backend-neutral computer action. Serializes with an `action`
/// discriminator and sibling fields — the exact shape the model emits as tool
/// input, so `serde_json::from_value` parses a tool call directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ComputerAction {
    /// Capture the current screen.
    Screenshot,
    /// Move the cursor to `(x, y)` without clicking.
    MouseMove { coordinate: (i32, i32) },
    /// Press a mouse button at `(x, y)`.
    Click {
        coordinate: (i32, i32),
        #[serde(default)]
        button: MouseButton,
    },
    /// Double-click the left button at `(x, y)`.
    DoubleClick { coordinate: (i32, i32) },
    /// Type a literal string.
    Type { text: String },
    /// Press a key chord, e.g. `"ctrl+s"` or `"Return"`.
    Key { keys: String },
    /// Scroll at `(x, y)` in `direction` by `amount` clicks.
    Scroll {
        coordinate: (i32, i32),
        direction: ScrollDirection,
        amount: i32,
    },
    /// Idle for `ms` milliseconds (e.g. wait for a page to settle).
    Wait { ms: u64 },
}

/// A captured screen image.
#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    /// IANA media type, e.g. `image/png`.
    pub media_type: String,
    /// Raw image bytes. Base64-encoding these into a
    /// [`crate::provider::types::ImageBlock`] is done where the tool result is
    /// assembled; that end-to-end wiring is a later slice (see module docs).
    pub bytes: Vec<u8>,
}

/// The result of executing a [`ComputerAction`].
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    /// A screen capture (the reply to `Screenshot`).
    Image(Screenshot),
    /// A textual result.
    Text(String),
    /// The action succeeded with nothing to report (clicks, typing, …).
    Ack,
}

// ---------------------------------------------------------------------------
// Executor seam
// ---------------------------------------------------------------------------

/// Drives a (real or mock) computer. The live implementation will talk to a
/// sandboxed display over a socket; the seam keeps that out of the action model
/// and the provider layer.
#[async_trait]
pub trait ComputerExecutor: Send + Sync {
    /// Perform one action and return what was observed.
    async fn execute(&self, action: &ComputerAction) -> Result<Observation, HarnessError>;

    /// The display size in pixels — fed to the provider tool spec so the model
    /// reasons in the right coordinate space.
    fn display_size(&self) -> (u32, u32);
}

/// Smallest possible valid PNG (1×1, transparent) — enough to prove the
/// screenshot path without an image encoder dependency.
const MOCK_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A no-op executor: `Screenshot` yields a 1×1 PNG, everything else acks. Lets
/// the agent loop and the eval harness exercise computer-use with no display.
#[derive(Debug, Clone)]
pub struct MockExecutor {
    pub width: u32,
    pub height: u32,
}

impl Default for MockExecutor {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
        }
    }
}

#[async_trait]
impl ComputerExecutor for MockExecutor {
    async fn execute(&self, action: &ComputerAction) -> Result<Observation, HarnessError> {
        Ok(match action {
            ComputerAction::Screenshot => Observation::Image(Screenshot {
                media_type: "image/png".to_string(),
                bytes: MOCK_PNG.to_vec(),
            }),
            _ => Observation::Ack,
        })
    }

    fn display_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

// ---------------------------------------------------------------------------
// Anthropic provider-native tool serialization
// ---------------------------------------------------------------------------

/// Beta header value (`anthropic-beta`) that enables the computer-use tool type.
pub const ANTHROPIC_COMPUTER_BETA: &str = "computer-use-2025-01-24";

/// The Anthropic-native tool spec for the `computer` tool. Unlike a custom
/// function tool, this is keyed by `type` and carries the display geometry; the
/// provider emits it verbatim in the request `tools` array (and must also send
/// the [`ANTHROPIC_COMPUTER_BETA`] beta header).
pub fn anthropic_tool_spec(width: u32, height: u32, display_number: Option<u32>) -> Value {
    let mut spec = json!({
        "type": "computer_20250124",
        "name": "computer",
        "display_width_px": width,
        "display_height_px": height,
    });
    if let Some(n) = display_number {
        spec["display_number"] = json!(n);
    }
    spec
}

// ---------------------------------------------------------------------------
// ToolImpl bridge (custom-function fallback for non-native backends)
// ---------------------------------------------------------------------------

/// Renders an [`Observation`] into the string a tool result carries today. The
/// provider layer can now transport a screenshot as an `image` block (see
/// [`crate::provider::types::ChatMessage::with_images`]); routing this
/// observation's bytes into that field across the re-exec IPC boundary is the
/// remaining slice — for now a screenshot is reported by shape so the loop is
/// observable.
fn render_observation(obs: &Observation) -> String {
    match obs {
        Observation::Image(s) => {
            format!(
                "screenshot captured ({}, {} bytes)",
                s.media_type,
                s.bytes.len()
            )
        }
        Observation::Text(t) => t.clone(),
        Observation::Ack => "ok".to_string(),
    }
}

/// Wraps a [`ComputerExecutor`] as a [`ToolImpl`] so non-Anthropic backends can
/// drive it as an ordinary function tool. Anthropic uses [`anthropic_tool_spec`]
/// instead; both dispatch into the same executor.
pub struct ComputerTool<E: ComputerExecutor> {
    executor: E,
}

impl<E: ComputerExecutor> ComputerTool<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E: ComputerExecutor + 'static> ToolImpl for ComputerTool<E> {
    fn name(&self) -> &'static str {
        "computer"
    }

    fn description(&self) -> &'static str {
        "Control a computer GUI: take a screenshot, move/click the mouse, type \
         text, press key chords, or scroll."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["screenshot", "mouse_move", "click", "double_click",
                             "type", "key", "scroll", "wait"],
                    "description": "The action to perform."
                },
                "coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 2,
                    "maxItems": 2,
                    "description": "[x, y] target, for mouse actions."
                },
                "button": {
                    "type": "string",
                    "enum": ["left", "right", "middle"],
                    "description": "Mouse button for `click` (default left)."
                },
                "text": { "type": "string", "description": "Text for `type`." },
                "keys": { "type": "string", "description": "Key chord for `key`, e.g. `ctrl+s`." },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Scroll direction."
                },
                "amount": { "type": "integer", "description": "Scroll amount in clicks." },
                "ms": { "type": "integer", "description": "Milliseconds for `wait`." }
            },
            "required": ["action"],
            // Per-action required fields — keeps the schema honest with the
            // runtime enum, so the model can't emit e.g. `{action:"click"}`
            // (schema-valid but a guaranteed deserialize failure).
            "oneOf": [
                { "properties": { "action": { "const": "screenshot" } }, "required": ["action"] },
                { "properties": { "action": { "const": "mouse_move" } }, "required": ["action", "coordinate"] },
                { "properties": { "action": { "const": "click" } }, "required": ["action", "coordinate"] },
                { "properties": { "action": { "const": "double_click" } }, "required": ["action", "coordinate"] },
                { "properties": { "action": { "const": "type" } }, "required": ["action", "text"] },
                { "properties": { "action": { "const": "key" } }, "required": ["action", "keys"] },
                { "properties": { "action": { "const": "scroll" } }, "required": ["action", "coordinate", "direction", "amount"] },
                { "properties": { "action": { "const": "wait" } }, "required": ["action", "ms"] }
            ]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String, HarnessError> {
        let action: ComputerAction =
            serde_json::from_value(args).map_err(|e| HarnessError::ToolExecution {
                tool: "computer".to_string(),
                reason: format!("invalid action: {e}"),
            })?;
        let obs = self.executor.execute(&action).await?;
        Ok(render_observation(&obs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_roundtrips_through_tool_input_shape() {
        // The serialized form is exactly what the model emits as tool input.
        let click = json!({"action": "click", "coordinate": [12, 34], "button": "right"});
        let parsed: ComputerAction = serde_json::from_value(click.clone()).unwrap();
        assert_eq!(
            parsed,
            ComputerAction::Click {
                coordinate: (12, 34),
                button: MouseButton::Right
            }
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), click);
    }

    #[test]
    fn click_button_defaults_to_left() {
        let parsed: ComputerAction =
            serde_json::from_value(json!({"action": "click", "coordinate": [1, 2]})).unwrap();
        assert_eq!(
            parsed,
            ComputerAction::Click {
                coordinate: (1, 2),
                button: MouseButton::Left
            }
        );
    }

    #[test]
    fn screenshot_and_type_parse() {
        assert_eq!(
            serde_json::from_value::<ComputerAction>(json!({"action": "screenshot"})).unwrap(),
            ComputerAction::Screenshot
        );
        assert_eq!(
            serde_json::from_value::<ComputerAction>(json!({"action": "type", "text": "hi"}))
                .unwrap(),
            ComputerAction::Type { text: "hi".into() }
        );
    }

    #[test]
    fn anthropic_spec_is_provider_native_not_custom_function() {
        let spec = anthropic_tool_spec(1280, 800, Some(1));
        assert_eq!(spec["type"], "computer_20250124");
        assert_eq!(spec["name"], "computer");
        assert_eq!(spec["display_width_px"], 1280);
        assert_eq!(spec["display_height_px"], 800);
        assert_eq!(spec["display_number"], 1);
        // A custom function tool would carry `input_schema`; a native tool must not.
        assert!(spec.get("input_schema").is_none());
        assert_eq!(ANTHROPIC_COMPUTER_BETA, "computer-use-2025-01-24");
    }

    #[test]
    fn anthropic_spec_omits_display_number_when_absent() {
        let spec = anthropic_tool_spec(1024, 768, None);
        assert!(spec.get("display_number").is_none());
    }

    #[tokio::test]
    async fn mock_screenshot_returns_png_image() {
        let exec = MockExecutor::default();
        let obs = exec.execute(&ComputerAction::Screenshot).await.unwrap();
        match obs {
            Observation::Image(s) => {
                assert_eq!(s.media_type, "image/png");
                assert_eq!(&s.bytes[1..4], b"PNG");
            }
            other => panic!("expected image, got {other:?}"),
        }
        assert_eq!(exec.display_size(), (1024, 768));
    }

    #[tokio::test]
    async fn mock_click_acks() {
        let exec = MockExecutor::default();
        let obs = exec
            .execute(&ComputerAction::Click {
                coordinate: (5, 5),
                button: MouseButton::Left,
            })
            .await
            .unwrap();
        assert_eq!(obs, Observation::Ack);
    }

    #[test]
    fn schema_requires_per_action_fields() {
        let tool = ComputerTool::new(MockExecutor::default());
        let schema = tool.parameters_schema();
        let branches = schema["oneOf"].as_array().expect("oneOf present");
        // The click branch must require `coordinate`, not just `action`.
        let click = branches
            .iter()
            .find(|b| b["properties"]["action"]["const"] == "click")
            .expect("click branch");
        let required: Vec<&str> = click["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"coordinate"));
    }

    #[tokio::test]
    async fn tool_dispatches_screenshot_through_executor() {
        let tool = ComputerTool::new(MockExecutor::default());
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(json!({"action": "screenshot"}), &ctx)
            .await
            .unwrap();
        assert!(out.contains("screenshot captured"));
        assert!(out.contains("image/png"));
    }

    #[tokio::test]
    async fn tool_rejects_unknown_action() {
        let tool = ComputerTool::new(MockExecutor::default());
        let ctx = ToolContext::new(std::env::temp_dir());
        let err = tool
            .execute(json!({"action": "teleport"}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::ToolExecution { .. }));
    }
}
