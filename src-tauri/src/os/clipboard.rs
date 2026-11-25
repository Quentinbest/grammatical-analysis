use std::fmt;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_POLL_TIMEOUT_MS: u64 = 250;
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 10;

const CAPTURE_SENTINEL: &str = "__grammatical_analysis_clipboard_sentinel_v1__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    ReadFailed(String),
    WriteFailed(String),
    SimulateCopyFailed(String),
    CaptureTimeout { timeout_ms: u64 },
    EmptySelection,
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFailed(msg) => write!(f, "failed to read clipboard: {msg}"),
            Self::WriteFailed(msg) => write!(f, "failed to write clipboard: {msg}"),
            Self::SimulateCopyFailed(msg) => write!(f, "failed to simulate copy: {msg}"),
            Self::CaptureTimeout { timeout_ms } => {
                write!(f, "timed out waiting for clipboard change after {timeout_ms}ms")
            }
            Self::EmptySelection => write!(f, "captured selection was empty"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// Abstraction over OS clipboard I/O and simulated copy keystroke.
pub trait ClipboardPort {
    fn read_text(&self) -> Result<Option<String>, ClipboardError>;
    fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
    fn clear(&self) -> Result<(), ClipboardError> {
        self.write_text("")
    }
    fn simulate_copy(&self) -> Result<(), ClipboardError>;
}

/// RAII guard that snapshots the clipboard on creation and restores it on `Drop`,
/// ensuring prior clipboard contents survive both success and error/timeout paths.
pub struct ClipboardGuard<'a, P: ClipboardPort + ?Sized> {
    port: &'a P,
    prior_text: Option<String>,
}

impl<'a, P: ClipboardPort + ?Sized> ClipboardGuard<'a, P> {
    pub fn snapshot(port: &'a P) -> Result<Self, ClipboardError> {
        let prior_text = port.read_text()?;
        Ok(Self { port, prior_text })
    }

    pub fn prior_text(&self) -> Option<&str> {
        self.prior_text.as_deref()
    }
}

impl<P: ClipboardPort + ?Sized> Drop for ClipboardGuard<'_, P> {
    fn drop(&mut self) {
        match &self.prior_text {
            Some(text) => {
                let _ = self.port.write_text(text);
            }
            None => {
                let _ = self.port.clear();
            }
        }
    }
}

/// Snapshots the prior clipboard, triggers a copy keystroke, polls up to
/// `poll_timeout_ms` for the selected text, and always restores the prior
/// clipboard when the internal `ClipboardGuard` drops.
pub fn capture_selected_text<P: ClipboardPort + ?Sized>(
    port: &P,
    poll_timeout_ms: u64,
) -> Result<String, ClipboardError> {
    capture_selected_text_with_interval(port, poll_timeout_ms, DEFAULT_POLL_INTERVAL_MS)
}

pub fn capture_selected_text_with_interval<P: ClipboardPort + ?Sized>(
    port: &P,
    poll_timeout_ms: u64,
    poll_interval_ms: u64,
) -> Result<String, ClipboardError> {
    let _guard = ClipboardGuard::snapshot(port)?;

    // Place a sentinel so we can detect a fresh copy even if the selected text
    // happens to match the user's prior clipboard text.
    port.write_text(CAPTURE_SENTINEL)?;
    port.simulate_copy()?;

    let deadline = Instant::now() + Duration::from_millis(poll_timeout_ms);
    let interval = Duration::from_millis(poll_interval_ms);

    loop {
        if let Some(current) = port.read_text()? {
            if current != CAPTURE_SENTINEL {
                if current.trim().is_empty() {
                    return Err(ClipboardError::EmptySelection);
                }
                return Ok(current);
            }
        }

        if Instant::now() >= deadline {
            return Err(ClipboardError::CaptureTimeout {
                timeout_ms: poll_timeout_ms,
            });
        }

        if !interval.is_zero() {
            thread::sleep(interval);
        }
    }
}

/// Real OS clipboard adapter (macOS via `pbpaste`/`pbcopy`/`osascript`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboard;

impl ClipboardPort for SystemClipboard {
    fn read_text(&self) -> Result<Option<String>, ClipboardError> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("pbpaste")
                .output()
                .map_err(|e| ClipboardError::ReadFailed(e.to_string()))?;
            if !output.status.success() {
                return Err(ClipboardError::ReadFailed(
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(ClipboardError::ReadFailed(
                "SystemClipboard is only implemented for macOS".to_string(),
            ))
        }
    }

    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};

            let mut child = Command::new("pbcopy")
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| ClipboardError::WriteFailed(e.to_string()))?;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin
                    .write_all(text.as_bytes())
                    .map_err(|e| ClipboardError::WriteFailed(e.to_string()))?;
            }

            let status = child
                .wait()
                .map_err(|e| ClipboardError::WriteFailed(e.to_string()))?;
            if !status.success() {
                return Err(ClipboardError::WriteFailed(format!(
                    "pbcopy exited with {status}"
                )));
            }
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err(ClipboardError::WriteFailed(
                "SystemClipboard is only implemented for macOS".to_string(),
            ))
        }
    }

    fn simulate_copy(&self) -> Result<(), ClipboardError> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("osascript")
                .args([
                    "-e",
                    "tell application \"System Events\" to keystroke \"c\" using command down",
                ])
                .output()
                .map_err(|e| ClipboardError::SimulateCopyFailed(e.to_string()))?;
            if !output.status.success() {
                return Err(ClipboardError::SimulateCopyFailed(
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
            Ok(())
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(ClipboardError::SimulateCopyFailed(
                "SystemClipboard is only implemented for macOS".to_string(),
            ))
        }
    }
}

/// Deterministic in-memory clipboard behavior for unit tests.
#[derive(Debug, Clone)]
pub enum FakeCopyBehavior {
    WriteImmediately(String),
    WriteAfterReads { text: String, delay_reads: usize },
    NoChange,
    Fail(String),
}

#[derive(Debug)]
struct FakeClipboardState {
    text: Option<String>,
    copy_behavior: FakeCopyBehavior,
    pending_text: Option<String>,
    remaining_delay_reads: usize,
    copy_count: usize,
}

/// In-memory `ClipboardPort` implementation for deterministic testing.
#[derive(Debug)]
pub struct FakeClipboard {
    state: Mutex<FakeClipboardState>,
}

impl FakeClipboard {
    pub fn new(initial_text: Option<String>, copy_behavior: FakeCopyBehavior) -> Self {
        Self {
            state: Mutex::new(FakeClipboardState {
                text: initial_text,
                copy_behavior,
                pending_text: None,
                remaining_delay_reads: 0,
                copy_count: 0,
            }),
        }
    }

    pub fn current_text(&self) -> Option<String> {
        self.state.lock().expect("fake clipboard mutex poisoned").text.clone()
    }

    pub fn copy_count(&self) -> usize {
        self.state.lock().expect("fake clipboard mutex poisoned").copy_count
    }
}

impl ClipboardPort for FakeClipboard {
    fn read_text(&self) -> Result<Option<String>, ClipboardError> {
        let mut state = self.state.lock().expect("fake clipboard mutex poisoned");
        if state.pending_text.is_some() {
            if state.remaining_delay_reads == 0 {
                state.text = state.pending_text.take();
            } else {
                state.remaining_delay_reads -= 1;
            }
        }
        Ok(state.text.clone())
    }

    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        let mut state = self.state.lock().expect("fake clipboard mutex poisoned");
        state.text = if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        };
        Ok(())
    }

    fn simulate_copy(&self) -> Result<(), ClipboardError> {
        let mut state = self.state.lock().expect("fake clipboard mutex poisoned");
        state.copy_count += 1;
        match state.copy_behavior.clone() {
            FakeCopyBehavior::WriteImmediately(selected) => {
                state.text = Some(selected);
                Ok(())
            }
            FakeCopyBehavior::WriteAfterReads { text, delay_reads } => {
                state.pending_text = Some(text);
                state.remaining_delay_reads = delay_reads;
                Ok(())
            }
            FakeCopyBehavior::NoChange => Ok(()),
            FakeCopyBehavior::Fail(reason) => Err(ClipboardError::SimulateCopyFailed(reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_selected_text_and_restores_prior_clipboard_on_success() {
        let clipboard = FakeClipboard::new(
            Some("PRIOR_CLIPBOARD_TEST".to_string()),
            FakeCopyBehavior::WriteImmediately("The book Maya recommended won a prize.".to_string()),
        );

        let captured = capture_selected_text_with_interval(&clipboard, 50, 1)
            .expect("capture should succeed");

        assert_eq!(captured, "The book Maya recommended won a prize.");
        assert_eq!(clipboard.copy_count(), 1);
        assert_eq!(
            clipboard.current_text().as_deref(),
            Some("PRIOR_CLIPBOARD_TEST")
        );
    }

    #[test]
    fn captures_when_selected_text_matches_prior_clipboard() {
        let clipboard = FakeClipboard::new(
            Some("Same sentence".to_string()),
            FakeCopyBehavior::WriteImmediately("Same sentence".to_string()),
        );

        let captured = capture_selected_text_with_interval(&clipboard, 50, 1)
            .expect("capture should detect fresh copy matching prior text");

        assert_eq!(captured, "Same sentence");
        assert_eq!(clipboard.current_text().as_deref(), Some("Same sentence"));
    }

    #[test]
    fn polls_until_delayed_copy_lands_then_restores() {
        let clipboard = FakeClipboard::new(
            Some("ORIGINAL".to_string()),
            FakeCopyBehavior::WriteAfterReads {
                text: "While walking home, the rain started.".to_string(),
                delay_reads: 3,
            },
        );

        let captured = capture_selected_text_with_interval(&clipboard, 100, 1)
            .expect("capture should succeed after polling");

        assert_eq!(captured, "While walking home, the rain started.");
        assert_eq!(clipboard.current_text().as_deref(), Some("ORIGINAL"));
    }

    #[test]
    fn restores_prior_clipboard_on_timeout() {
        let clipboard = FakeClipboard::new(
            Some("PRIOR_CLIPBOARD_TEST".to_string()),
            FakeCopyBehavior::NoChange,
        );

        let err = capture_selected_text_with_interval(&clipboard, 15, 2)
            .expect_err("capture should time out when clipboard does not change");

        assert_eq!(err, ClipboardError::CaptureTimeout { timeout_ms: 15 });
        assert_eq!(
            clipboard.current_text().as_deref(),
            Some("PRIOR_CLIPBOARD_TEST")
        );
    }

    #[test]
    fn restores_prior_clipboard_on_simulate_copy_error() {
        let clipboard = FakeClipboard::new(
            Some("PRIOR_CLIPBOARD_TEST".to_string()),
            FakeCopyBehavior::Fail("Accessibility permission denied".to_string()),
        );

        let err = capture_selected_text_with_interval(&clipboard, 50, 1)
            .expect_err("capture should surface simulate_copy failure");

        assert_eq!(
            err,
            ClipboardError::SimulateCopyFailed("Accessibility permission denied".to_string())
        );
        assert_eq!(
            clipboard.current_text().as_deref(),
            Some("PRIOR_CLIPBOARD_TEST")
        );
    }

    #[test]
    fn restores_empty_clipboard_when_prior_was_none() {
        let clipboard = FakeClipboard::new(
            None,
            FakeCopyBehavior::WriteImmediately("Selected text".to_string()),
        );

        let captured = capture_selected_text_with_interval(&clipboard, 50, 1)
            .expect("capture should succeed");

        assert_eq!(captured, "Selected text");
        assert_eq!(clipboard.current_text(), None);
    }

    #[test]
    fn rejects_whitespace_only_selection_and_restores_clipboard() {
        let clipboard = FakeClipboard::new(
            Some("KEEP_ME".to_string()),
            FakeCopyBehavior::WriteImmediately("   \n\t ".to_string()),
        );

        let err = capture_selected_text_with_interval(&clipboard, 50, 1)
            .expect_err("whitespace-only selection should error");

        assert_eq!(err, ClipboardError::EmptySelection);
        assert_eq!(clipboard.current_text().as_deref(), Some("KEEP_ME"));
    }
}
