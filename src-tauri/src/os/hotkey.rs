use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

use crate::os::clipboard::{capture_selected_text, ClipboardError, ClipboardPort};
use crate::os::floating_window::Point;

pub const DEFAULT_HOTKEY: &str = "Alt+E";
pub const SELECTION_CAPTURED_EVENT: &str = "selection://captured";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Modifier {
    Control,
    Alt,
    Shift,
    Super,
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control => f.write_str("Ctrl"),
            Self::Alt => f.write_str("Alt"),
            Self::Shift => f.write_str("Shift"),
            Self::Super => f.write_str("Super"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    Empty,
    EmptyToken,
    DuplicateModifier(Modifier),
    MissingModifier,
    MissingKey,
    MultipleKeys { first: String, second: String },
    InvalidKey(String),
    RegistrationFailed(String),
    CaptureFailed(ClipboardError),
    EmitFailed(String),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("hotkey string cannot be empty"),
            Self::EmptyToken => f.write_str("hotkey contains an empty '+' segment"),
            Self::DuplicateModifier(m) => write!(f, "duplicate modifier '{m}' in hotkey"),
            Self::MissingModifier => {
                f.write_str("global hotkey must include at least one modifier")
            }
            Self::MissingKey => f.write_str("hotkey must include a non-modifier trigger key"),
            Self::MultipleKeys { first, second } => {
                write!(f, "hotkey cannot specify multiple keys ('{first}' and '{second}')")
            }
            Self::InvalidKey(k) => write!(f, "unsupported hotkey key '{k}'"),
            Self::RegistrationFailed(msg) => write!(f, "failed to register global hotkey: {msg}"),
            Self::CaptureFailed(err) => write!(f, "hotkey selection capture failed: {err}"),
            Self::EmitFailed(msg) => write!(f, "failed to emit {SELECTION_CAPTURED_EVENT}: {msg}"),
        }
    }
}

impl std::error::Error for HotkeyError {}

/// Parsed and validated global hotkey specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySpec {
    pub modifiers: Vec<Modifier>,
    pub key: String,
}

impl Default for HotkeySpec {
    fn default() -> Self {
        Self::parse(DEFAULT_HOTKEY).expect("DEFAULT_HOTKEY must be valid")
    }
}

impl fmt::Display for HotkeySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(f, "{modifier}+")?;
        }
        f.write_str(&self.key)
    }
}

impl HotkeySpec {
    pub fn parse(raw: &str) -> Result<Self, HotkeyError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(HotkeyError::Empty);
        }

        let mut modifiers = Vec::new();
        let mut key: Option<String> = None;

        for segment in trimmed.split('+') {
            let token = segment.trim();
            if token.is_empty() {
                return Err(HotkeyError::EmptyToken);
            }

            if let Some(modifier) = parse_modifier(token) {
                if modifiers.contains(&modifier) {
                    return Err(HotkeyError::DuplicateModifier(modifier));
                }
                modifiers.push(modifier);
            } else {
                let normalized = normalize_key(token)?;
                if let Some(first) = key {
                    return Err(HotkeyError::MultipleKeys {
                        first,
                        second: normalized,
                    });
                }
                key = Some(normalized);
            }
        }

        if modifiers.is_empty() {
            return Err(HotkeyError::MissingModifier);
        }
        let key = key.ok_or(HotkeyError::MissingKey)?;

        modifiers.sort();
        Ok(Self { modifiers, key })
    }
}

fn parse_modifier(token: &str) -> Option<Modifier> {
    match token.to_ascii_lowercase().as_str() {
        "alt" | "option" | "opt" => Some(Modifier::Alt),
        "ctrl" | "control" => Some(Modifier::Control),
        "shift" => Some(Modifier::Shift),
        "super" | "cmd" | "command" | "meta" | "win" | "cmdorctrl" | "commandorcontrol" => {
            Some(Modifier::Super)
        }
        _ => None,
    }
}

fn normalize_key(token: &str) -> Result<String, HotkeyError> {
    let upper = token.to_ascii_uppercase();
    if upper.len() == 1 && upper.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(upper);
    }

    if let Some(rest) = upper.strip_prefix('F') {
        if !rest.starts_with('0') {
            if let Ok(num) = rest.parse::<u8>() {
                if (1..=24).contains(&num) {
                    return Ok(upper);
                }
            }
        }
    }

    match upper.as_str() {
        "SPACE" => Ok("Space".to_string()),
        "TAB" => Ok("Tab".to_string()),
        "ENTER" | "RETURN" => Ok("Enter".to_string()),
        "ESC" | "ESCAPE" => Ok("Escape".to_string()),
        _ => Err(HotkeyError::InvalidKey(token.to_string())),
    }
}

/// Event payload emitted on `selection://captured` when the global hotkey fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCapturedPayload {
    pub text: String,
    pub cursor: Point,
    pub hotkey: String,
    pub latency_ms: u64,
}

/// Port for platform global shortcut registration backends.
pub trait HotkeyRegistrar {
    fn register(&self, spec: &HotkeySpec) -> Result<(), HotkeyError>;
    fn unregister(&self, spec: &HotkeySpec) -> Result<(), HotkeyError>;
}

/// Active global hotkey registration handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyRegistration {
    spec: HotkeySpec,
}

impl HotkeyRegistration {
    pub fn spec(&self) -> &HotkeySpec {
        &self.spec
    }
}

/// Validates `shortcut` and registers it via `registrar`.
pub fn register_global_hotkey<R: HotkeyRegistrar>(
    registrar: &R,
    shortcut: &str,
) -> Result<HotkeyRegistration, HotkeyError> {
    let spec = HotkeySpec::parse(shortcut)?;
    registrar.register(&spec)?;
    Ok(HotkeyRegistration { spec })
}

/// Emits `selection://captured` to all Tauri webview listeners.
pub fn emit_selection_captured<R: Runtime>(
    app: &AppHandle<R>,
    payload: &SelectionCapturedPayload,
) -> Result<(), HotkeyError> {
    app.emit(SELECTION_CAPTURED_EVENT, payload)
        .map_err(|e| HotkeyError::EmitFailed(e.to_string()))
}

/// Executes the hotkey capture flow: captures selected text with clipboard
/// restore protection and emits `selection://captured` through Tauri.
pub fn trigger_hotkey_capture<R: Runtime, P: ClipboardPort + ?Sized>(
    app: &AppHandle<R>,
    clipboard: &P,
    spec: &HotkeySpec,
    cursor: Point,
    poll_timeout_ms: u64,
) -> Result<SelectionCapturedPayload, HotkeyError> {
    let started = Instant::now();
    let text = capture_selected_text(clipboard, poll_timeout_ms)
        .map_err(HotkeyError::CaptureFailed)?;

    let payload = SelectionCapturedPayload {
        text,
        cursor,
        hotkey: spec.to_string(),
        latency_ms: started.elapsed().as_millis() as u64,
    };

    emit_selection_captured(app, &payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeRegistrar {
        registered: Mutex<Vec<HotkeySpec>>,
    }

    impl FakeRegistrar {
        fn new() -> Self {
            Self {
                registered: Mutex::new(Vec::new()),
            }
        }
    }

    impl HotkeyRegistrar for FakeRegistrar {
        fn register(&self, spec: &HotkeySpec) -> Result<(), HotkeyError> {
            self.registered.lock().unwrap().push(spec.clone());
            Ok(())
        }

        fn unregister(&self, spec: &HotkeySpec) -> Result<(), HotkeyError> {
            self.registered.lock().unwrap().retain(|s| s != spec);
            Ok(())
        }
    }

    #[test]
    fn parses_default_hotkey_alt_e() {
        let spec = HotkeySpec::default();
        assert_eq!(spec.modifiers, vec![Modifier::Alt]);
        assert_eq!(spec.key, "E");
        assert_eq!(spec.to_string(), "Alt+E");
    }

    #[test]
    fn normalizes_aliases_and_modifier_order() {
        let spec = HotkeySpec::parse(" shift + option + ctrl + g ").expect("valid hotkey");
        assert_eq!(
            spec.modifiers,
            vec![Modifier::Control, Modifier::Alt, Modifier::Shift]
        );
        assert_eq!(spec.key, "G");
        assert_eq!(spec.to_string(), "Ctrl+Alt+Shift+G");

        let fn_key = HotkeySpec::parse("Cmd+F12").expect("valid function key hotkey");
        assert_eq!(fn_key.modifiers, vec![Modifier::Super]);
        assert_eq!(fn_key.key, "F12");
        assert_eq!(fn_key.to_string(), "Super+F12");
    }

    #[test]
    fn rejects_invalid_hotkey_strings() {
        assert_eq!(HotkeySpec::parse(""), Err(HotkeyError::Empty));
        assert_eq!(HotkeySpec::parse("   "), Err(HotkeyError::Empty));
        assert_eq!(HotkeySpec::parse("Alt+"), Err(HotkeyError::EmptyToken));
        assert_eq!(HotkeySpec::parse("+E"), Err(HotkeyError::EmptyToken));
        assert_eq!(
            HotkeySpec::parse("Alt+Option+E"),
            Err(HotkeyError::DuplicateModifier(Modifier::Alt))
        );
        assert_eq!(HotkeySpec::parse("E"), Err(HotkeyError::MissingModifier));
        assert_eq!(
            HotkeySpec::parse("Ctrl+Shift"),
            Err(HotkeyError::MissingKey)
        );
        assert_eq!(
            HotkeySpec::parse("Alt+E+F"),
            Err(HotkeyError::MultipleKeys {
                first: "E".to_string(),
                second: "F".to_string(),
            })
        );
        assert_eq!(
            HotkeySpec::parse("Alt+Banana"),
            Err(HotkeyError::InvalidKey("Banana".to_string()))
        );
        assert_eq!(
            HotkeySpec::parse("Alt+F25"),
            Err(HotkeyError::InvalidKey("F25".to_string()))
        );
    }

    #[test]
    fn registers_validated_hotkey_through_registrar() {
        let registrar = FakeRegistrar::new();
        let reg = register_global_hotkey(&registrar, "Option+E").expect("registration succeeds");
        assert_eq!(reg.spec().to_string(), "Alt+E");
        assert_eq!(registrar.registered.lock().unwrap().len(), 1);
    }

    #[test]
    fn rejects_leading_zero_function_keys_and_accepts_f1_to_f24() {
        assert_eq!(
            HotkeySpec::parse("Alt+F01"),
            Err(HotkeyError::InvalidKey("F01".to_string()))
        );
        assert_eq!(
            HotkeySpec::parse("Alt+F00"),
            Err(HotkeyError::InvalidKey("F00".to_string()))
        );
        assert_eq!(
            HotkeySpec::parse("Alt+f09"),
            Err(HotkeyError::InvalidKey("f09".to_string()))
        );

        for n in 1..=24 {
            let raw = format!("Alt+F{n}");
            let spec = HotkeySpec::parse(&raw)
                .unwrap_or_else(|e| panic!("expected {raw} to parse, got {e}"));
            assert_eq!(spec.modifiers, vec![Modifier::Alt]);
            assert_eq!(spec.key, format!("F{n}"));
            assert_eq!(spec.to_string(), raw);
        }
    }
}
