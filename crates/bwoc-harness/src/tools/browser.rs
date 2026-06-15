//! Headless-browser computer-use executor (P1).
//!
//! Two layers:
//!   1. A **pure** action→CDP mapping ([`cdp_plan`] + helpers) — the
//!      backend-neutral description of what a browser does for each
//!      [`ComputerAction`]. Always compiled, always tested, **no browser dep**.
//!   2. A **feature-gated** [`BrowserExecutor`] (the `browser` feature) that
//!      drives a real headless Chromium over CDP via `chromiumoxide`,
//!      implementing the [`ComputerExecutor`] seam from the P0 spike.
//!
//! This is the lighter of the two P1 paths: a headless browser needs no display
//! server, runs on macOS/Linux/Windows, and covers the bulk of computer-use
//! tasks (web automation) — versus a full Xvfb desktop container. The default
//! build pulls **zero** browser deps; the CDP mapping is what makes the
//! executor honest and reviewable without one.
//!
//! [`ComputerExecutor`]: super::computer::ComputerExecutor

use super::computer::{ComputerAction, MouseButton, ScrollDirection};
use serde_json::{Value, json};

/// Default headless viewport (px). The model reasons in this coordinate space.
pub const DEFAULT_VIEWPORT: (u32, u32) = (1280, 800);

/// Pixels scrolled per scroll "click".
const SCROLL_STEP_PX: i64 = 100;

/// A planned CDP call: the DevTools method plus its JSON params. The pure,
/// testable description of one step of an action — independent of any browser
/// client library.
#[derive(Debug, Clone, PartialEq)]
pub struct CdpCall {
    pub method: &'static str,
    pub params: Value,
}

/// CDP button name for a [`MouseButton`].
pub fn mouse_button_name(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

/// `(deltaX, deltaY)` for a scroll of `amount` clicks in `direction`.
pub fn scroll_deltas(direction: ScrollDirection, amount: i32) -> (i64, i64) {
    let step = SCROLL_STEP_PX * amount as i64;
    match direction {
        ScrollDirection::Up => (0, -step),
        ScrollDirection::Down => (0, step),
        ScrollDirection::Left => (-step, 0),
        ScrollDirection::Right => (step, 0),
    }
}

fn mouse_event(kind: &str, x: i32, y: i32, button: &str, click_count: i64) -> CdpCall {
    CdpCall {
        method: "Input.dispatchMouseEvent",
        params: json!({
            "type": kind, "x": x, "y": y,
            "button": button, "clickCount": click_count,
        }),
    }
}

/// Translate one [`ComputerAction`] into the ordered CDP calls that realize it.
/// `Wait` and `Screenshot` are handled specially by the executor (sleep / image
/// capture) but still appear here so the mapping is complete and inspectable.
pub fn cdp_plan(action: &ComputerAction) -> Vec<CdpCall> {
    match action {
        ComputerAction::Screenshot => vec![CdpCall {
            method: "Page.captureScreenshot",
            params: json!({ "format": "png" }),
        }],
        ComputerAction::MouseMove { coordinate: (x, y) } => {
            vec![mouse_event("mouseMoved", *x, *y, "none", 0)]
        }
        ComputerAction::Click {
            coordinate: (x, y),
            button,
        } => {
            let b = mouse_button_name(*button);
            vec![
                mouse_event("mousePressed", *x, *y, b, 1),
                mouse_event("mouseReleased", *x, *y, b, 1),
            ]
        }
        // A double-click is two full click cycles with an incrementing
        // clickCount (1 then 2) — Chromium keys `dblclick` off that, so a single
        // press/release with clickCount=2 is registered as one click on many pages.
        ComputerAction::DoubleClick { coordinate: (x, y) } => vec![
            mouse_event("mousePressed", *x, *y, "left", 1),
            mouse_event("mouseReleased", *x, *y, "left", 1),
            mouse_event("mousePressed", *x, *y, "left", 2),
            mouse_event("mouseReleased", *x, *y, "left", 2),
        ],
        ComputerAction::Type { text } => vec![CdpCall {
            method: "Input.insertText",
            params: json!({ "text": text }),
        }],
        ComputerAction::Key { keys } => vec![
            CdpCall {
                method: "Input.dispatchKeyEvent",
                params: json!({ "type": "keyDown", "key": keys }),
            },
            CdpCall {
                method: "Input.dispatchKeyEvent",
                params: json!({ "type": "keyUp", "key": keys }),
            },
        ],
        ComputerAction::Scroll {
            coordinate: (x, y),
            direction,
            amount,
        } => {
            let (dx, dy) = scroll_deltas(*direction, *amount);
            vec![CdpCall {
                method: "Input.dispatchMouseEvent",
                params: json!({
                    "type": "mouseWheel", "x": x, "y": y,
                    "deltaX": dx, "deltaY": dy,
                }),
            }]
        }
        ComputerAction::Wait { ms } => vec![CdpCall {
            method: "__sleep__",
            params: json!({ "ms": ms }),
        }],
    }
}

// ---------------------------------------------------------------------------
// Live executor — feature-gated (`browser`)
// ---------------------------------------------------------------------------

#[cfg(feature = "browser")]
pub use live::BrowserExecutor;

#[cfg(feature = "browser")]
mod live {
    use super::{DEFAULT_VIEWPORT, mouse_button_name, scroll_deltas};
    use crate::error::HarnessError;
    use crate::tools::computer::{ComputerAction, ComputerExecutor, Observation, Screenshot};
    use async_trait::async_trait;
    use chromiumoxide::cdp::browser_protocol::input::{
        DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
        DispatchMouseEventType, InsertTextParams, MouseButton as CdpMouseButton,
    };
    use chromiumoxide::page::ScreenshotParams;
    use chromiumoxide::{Browser, BrowserConfig, Page};
    use futures_util::StreamExt;
    use std::time::Duration;
    use tokio::task::JoinHandle;

    fn err(reason: impl std::fmt::Display) -> HarnessError {
        HarnessError::ToolExecution {
            tool: "browser".to_string(),
            reason: reason.to_string(),
        }
    }

    fn cdp_button(b: super::MouseButton) -> CdpMouseButton {
        match mouse_button_name(b) {
            "right" => CdpMouseButton::Right,
            "middle" => CdpMouseButton::Middle,
            _ => CdpMouseButton::Left,
        }
    }

    /// A headless-Chromium-backed [`ComputerExecutor`]. Drives a single page over
    /// CDP. The browser is launched headless; no display server is required.
    pub struct BrowserExecutor {
        // Kept alive for the session; dropping closes the browser.
        _browser: Browser,
        page: Page,
        _handler: JoinHandle<()>,
        viewport: (u32, u32),
    }

    impl BrowserExecutor {
        /// Launch a headless browser and open `start_url` (e.g. `about:blank`).
        pub async fn launch(start_url: &str) -> Result<Self, HarnessError> {
            let (w, h) = DEFAULT_VIEWPORT;
            let config = BrowserConfig::builder()
                .window_size(w, h)
                .build()
                .map_err(err)?;
            let (browser, mut handler) = Browser::launch(config).await.map_err(err)?;
            // The handler drives the CDP connection; it must be polled for the
            // browser to make progress.
            let handle = tokio::spawn(async move { while handler.next().await.is_some() {} });
            let page = browser.new_page(start_url).await.map_err(err)?;
            Ok(Self {
                _browser: browser,
                page,
                _handler: handle,
                viewport: (w, h),
            })
        }

        async fn mouse(
            &self,
            kind: DispatchMouseEventType,
            x: i32,
            y: i32,
            button: CdpMouseButton,
            clicks: i64,
        ) -> Result<(), HarnessError> {
            let params = DispatchMouseEventParams::builder()
                .r#type(kind)
                .x(x as f64)
                .y(y as f64)
                .button(button)
                .click_count(clicks)
                .build()
                .map_err(err)?;
            self.page.execute(params).await.map_err(err)?;
            Ok(())
        }
    }

    #[async_trait]
    impl ComputerExecutor for BrowserExecutor {
        async fn execute(&self, action: &ComputerAction) -> Result<Observation, HarnessError> {
            match action {
                ComputerAction::Screenshot => {
                    let bytes = self
                        .page
                        .screenshot(ScreenshotParams::builder().build())
                        .await
                        .map_err(err)?;
                    Ok(Observation::Image(Screenshot {
                        media_type: "image/png".to_string(),
                        bytes,
                    }))
                }
                ComputerAction::MouseMove { coordinate: (x, y) } => {
                    self.mouse(
                        DispatchMouseEventType::MouseMoved,
                        *x,
                        *y,
                        CdpMouseButton::None,
                        0,
                    )
                    .await?;
                    Ok(Observation::Ack)
                }
                ComputerAction::Click {
                    coordinate: (x, y),
                    button,
                } => {
                    let b = cdp_button(*button);
                    self.mouse(DispatchMouseEventType::MousePressed, *x, *y, b.clone(), 1)
                        .await?;
                    self.mouse(DispatchMouseEventType::MouseReleased, *x, *y, b, 1)
                        .await?;
                    Ok(Observation::Ack)
                }
                ComputerAction::DoubleClick { coordinate: (x, y) } => {
                    // Two click cycles with clickCount 1 then 2 — see `cdp_plan`.
                    for clicks in [1, 2] {
                        self.mouse(
                            DispatchMouseEventType::MousePressed,
                            *x,
                            *y,
                            CdpMouseButton::Left,
                            clicks,
                        )
                        .await?;
                        self.mouse(
                            DispatchMouseEventType::MouseReleased,
                            *x,
                            *y,
                            CdpMouseButton::Left,
                            clicks,
                        )
                        .await?;
                    }
                    Ok(Observation::Ack)
                }
                ComputerAction::Type { text } => {
                    let params = InsertTextParams::builder()
                        .text(text.clone())
                        .build()
                        .map_err(err)?;
                    self.page.execute(params).await.map_err(err)?;
                    Ok(Observation::Ack)
                }
                ComputerAction::Key { keys } => {
                    for kind in [DispatchKeyEventType::KeyDown, DispatchKeyEventType::KeyUp] {
                        let params = DispatchKeyEventParams::builder()
                            .r#type(kind)
                            .key(keys.clone())
                            .build()
                            .map_err(err)?;
                        self.page.execute(params).await.map_err(err)?;
                    }
                    Ok(Observation::Ack)
                }
                ComputerAction::Scroll {
                    coordinate: (x, y),
                    direction,
                    amount,
                } => {
                    let (dx, dy) = scroll_deltas(*direction, *amount);
                    let params = DispatchMouseEventParams::builder()
                        .r#type(DispatchMouseEventType::MouseWheel)
                        .x(*x as f64)
                        .y(*y as f64)
                        .delta_x(dx as f64)
                        .delta_y(dy as f64)
                        .build()
                        .map_err(err)?;
                    self.page.execute(params).await.map_err(err)?;
                    Ok(Observation::Ack)
                }
                ComputerAction::Wait { ms } => {
                    tokio::time::sleep(Duration::from_millis(*ms)).await;
                    Ok(Observation::Ack)
                }
            }
        }

        fn display_size(&self) -> (u32, u32) {
            self.viewport
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_plan_is_press_then_release() {
        let plan = cdp_plan(&ComputerAction::Click {
            coordinate: (10, 20),
            button: MouseButton::Left,
        });
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].params["type"], "mousePressed");
        assert_eq!(plan[1].params["type"], "mouseReleased");
        assert_eq!(plan[0].params["x"], 10);
        assert_eq!(plan[0].params["button"], "left");
        assert_eq!(plan[0].params["clickCount"], 1);
    }

    #[test]
    fn double_click_is_two_cycles_with_incrementing_count() {
        let plan = cdp_plan(&ComputerAction::DoubleClick { coordinate: (1, 2) });
        assert_eq!(plan.len(), 4);
        let kinds: Vec<&str> = plan
            .iter()
            .map(|c| c.params["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "mousePressed",
                "mouseReleased",
                "mousePressed",
                "mouseReleased"
            ]
        );
        let counts: Vec<i64> = plan
            .iter()
            .map(|c| c.params["clickCount"].as_i64().unwrap())
            .collect();
        assert_eq!(counts, [1, 1, 2, 2]);
    }

    #[test]
    fn right_button_name() {
        let plan = cdp_plan(&ComputerAction::Click {
            coordinate: (0, 0),
            button: MouseButton::Right,
        });
        assert_eq!(plan[0].params["button"], "right");
    }

    #[test]
    fn scroll_deltas_directions() {
        assert_eq!(scroll_deltas(ScrollDirection::Down, 3), (0, 300));
        assert_eq!(scroll_deltas(ScrollDirection::Up, 1), (0, -100));
        assert_eq!(scroll_deltas(ScrollDirection::Right, 2), (200, 0));
        assert_eq!(scroll_deltas(ScrollDirection::Left, 1), (-100, 0));
    }

    #[test]
    fn scroll_plan_carries_wheel_deltas() {
        let plan = cdp_plan(&ComputerAction::Scroll {
            coordinate: (5, 5),
            direction: ScrollDirection::Down,
            amount: 2,
        });
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].params["type"], "mouseWheel");
        assert_eq!(plan[0].params["deltaY"], 200);
    }

    #[test]
    fn type_and_key_plans() {
        let t = cdp_plan(&ComputerAction::Type { text: "hi".into() });
        assert_eq!(t[0].method, "Input.insertText");
        assert_eq!(t[0].params["text"], "hi");

        let k = cdp_plan(&ComputerAction::Key {
            keys: "Enter".into(),
        });
        assert_eq!(k.len(), 2);
        assert_eq!(k[0].params["type"], "keyDown");
        assert_eq!(k[1].params["type"], "keyUp");
    }

    #[test]
    fn screenshot_plan_is_png() {
        let plan = cdp_plan(&ComputerAction::Screenshot);
        assert_eq!(plan[0].method, "Page.captureScreenshot");
        assert_eq!(plan[0].params["format"], "png");
    }

    // Live end-to-end check: launches a real headless Chromium. Gated behind the
    // `browser` feature AND `#[ignore]` so CI never runs it (no browser in the
    // matrix). Run locally with:
    //   cargo test -p bwoc-harness --features browser -- --ignored live_
    #[cfg(feature = "browser")]
    #[tokio::test]
    #[ignore = "launches a real headless Chrome"]
    async fn live_screenshot_and_input_smoke() {
        use crate::tools::computer::{ComputerExecutor, Observation};
        let exec = super::BrowserExecutor::launch("data:text/html,<h1>BWOC</h1>")
            .await
            .expect("launch headless chrome");
        let obs = exec
            .execute(&ComputerAction::Screenshot)
            .await
            .expect("screenshot");
        match obs {
            Observation::Image(s) => {
                assert_eq!(s.media_type, "image/png");
                assert_eq!(&s.bytes[1..4], b"PNG");
                assert!(s.bytes.len() > 100);
            }
            other => panic!("expected image, got {other:?}"),
        }
        // Input actions should dispatch without error.
        exec.execute(&ComputerAction::MouseMove { coordinate: (5, 5) })
            .await
            .expect("mouse move");
        exec.execute(&ComputerAction::Type { text: "hi".into() })
            .await
            .expect("type");
        assert_eq!(exec.display_size(), DEFAULT_VIEWPORT);
    }
}
