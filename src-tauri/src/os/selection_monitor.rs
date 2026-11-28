use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

use crate::os::floating_window::{
    show_pill_window_near_cursor, Point, Rect, DEFAULT_WINDOW_MARGIN, SELECTION_PILL_WINDOW_LABEL,
};

pub const SELECTION_CANDIDATE_EVENT: &str = "selection://candidate";
pub const ASSISTANT_STATUS_EVENT: &str = "assistant://status";
pub const PILL_AUTO_DISMISS_DURATION: Duration = Duration::from_secs(6);
pub const SELECTION_SETTLE_DELAY: Duration = Duration::from_millis(45);

const MIN_DRAG_DISTANCE: f64 = 12.0;
const MIN_DRAG_DURATION_MS: u64 = 50;
const MAX_DRAG_DURATION_MS: u64 = 15_000;
const MAX_MULTI_CLICK_DISTANCE: f64 = 10.0;
const MAX_ANALYZABLE_CHARS: usize = 2_400;
const MIN_ASCII_LETTERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseGestureEvent {
    LeftMouseDown {
        point: Point,
        at_ms: u64,
        click_count: i64,
    },
    LeftMouseDragged {
        point: Point,
    },
    LeftMouseUp {
        point: Point,
        at_ms: u64,
        click_count: i64,
    },
    OtherInteraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureOutcome {
    Candidate { cursor: Point },
    DismissPill,
    Ignore,
}

#[derive(Debug, Clone, Copy)]
struct ActivePress {
    start_point: Point,
    start_at_ms: u64,
    click_count: i64,
    dragged: bool,
}

#[derive(Debug, Default, Clone)]
pub struct SelectionGestureDetector {
    active_press: Option<ActivePress>,
}

impl SelectionGestureDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_event(&mut self, event: MouseGestureEvent) -> GestureOutcome {
        match event {
            MouseGestureEvent::LeftMouseDown {
                point,
                at_ms,
                click_count,
            } => self.on_mouse_down(point, at_ms, click_count),
            MouseGestureEvent::LeftMouseDragged { point } => self.on_mouse_drag(point),
            MouseGestureEvent::LeftMouseUp {
                point,
                at_ms,
                click_count,
            } => self.on_mouse_up(point, at_ms, click_count),
            MouseGestureEvent::OtherInteraction => self.on_other_interaction(),
        }
    }

    pub fn on_mouse_down(&mut self, point: Point, at_ms: u64, click_count: i64) -> GestureOutcome {
        self.active_press = Some(ActivePress {
            start_point: point,
            start_at_ms: at_ms,
            click_count,
            dragged: false,
        });
        GestureOutcome::Ignore
    }

    pub fn on_mouse_drag(&mut self, _point: Point) -> GestureOutcome {
        if let Some(press) = &mut self.active_press {
            press.dragged = true;
        }
        GestureOutcome::Ignore
    }

    pub fn on_mouse_up(&mut self, point: Point, at_ms: u64, click_count: i64) -> GestureOutcome {
        let Some(press) = self.active_press.take() else {
            return GestureOutcome::Ignore;
        };

        let dx = f64::from(point.x.saturating_sub(press.start_point.x));
        let dy = f64::from(point.y.saturating_sub(press.start_point.y));
        let distance = (dx * dx + dy * dy).sqrt();
        let duration_ms = at_ms.saturating_sub(press.start_at_ms);
        let effective_clicks = click_count.max(press.click_count);

        if press.dragged && distance >= MIN_DRAG_DISTANCE {
            if (MIN_DRAG_DURATION_MS..=MAX_DRAG_DURATION_MS).contains(&duration_ms) {
                return GestureOutcome::Candidate { cursor: point };
            }
            return GestureOutcome::Ignore;
        }

        if effective_clicks >= 2 && distance <= MAX_MULTI_CLICK_DISTANCE {
            return GestureOutcome::Candidate { cursor: point };
        }

        if effective_clicks <= 1 && distance < MIN_DRAG_DISTANCE {
            return GestureOutcome::DismissPill;
        }

        GestureOutcome::Ignore
    }

    pub fn on_other_interaction(&mut self) -> GestureOutcome {
        self.active_press = None;
        GestureOutcome::DismissPill
    }
}

pub fn is_analyzable_selection(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let mut char_count = 0usize;
    let mut ascii_letters = 0usize;
    for ch in trimmed.chars() {
        char_count += 1;
        if char_count > MAX_ANALYZABLE_CHARS {
            return false;
        }
        if ch.is_ascii_alphabetic() {
            ascii_letters += 1;
        }
    }

    ascii_letters >= MIN_ASCII_LETTERS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxSelectionRead {
    Selected(String),
    ExplicitlyEmpty,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingSelection {
    Resolved {
        text: String,
        cursor: Point,
    },
    DeferredCopy {
        cursor: Point,
        source_pid: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCandidatePayload {
    pub preview_text: Option<String>,
    pub cursor: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionAssistantStatus {
    pub popup_enabled: bool,
    pub accessibility_trusted: bool,
    pub resident_in_tray: bool,
}

pub struct SelectionAssistantState {
    popup_enabled: AtomicBool,
    exiting: AtomicBool,
    resident_in_tray: AtomicBool,
    has_pending_flag: AtomicBool,
    generation: AtomicU64,
    pending: Mutex<Option<PendingSelection>>,
    tray_toggle_item: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl Default for SelectionAssistantState {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionAssistantState {
    pub fn new() -> Self {
        Self {
            popup_enabled: AtomicBool::new(true),
            exiting: AtomicBool::new(false),
            resident_in_tray: AtomicBool::new(false),
            has_pending_flag: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            pending: Mutex::new(None),
            tray_toggle_item: Mutex::new(None),
        }
    }

    pub fn is_popup_enabled(&self) -> bool {
        self.popup_enabled.load(Ordering::SeqCst)
    }

    pub fn set_popup_enabled(&self, enabled: bool) -> bool {
        self.popup_enabled.store(enabled, Ordering::SeqCst);
        if let Ok(guard) = self.tray_toggle_item.lock() {
            if let Some(item) = guard.as_ref() {
                let _ = item.set_checked(enabled);
            }
        }
        if !enabled {
            return self.clear_pending();
        }
        false
    }

    pub fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    pub fn set_exiting(&self, exiting: bool) {
        self.exiting.store(exiting, Ordering::SeqCst);
    }

    pub fn is_resident_in_tray(&self) -> bool {
        self.resident_in_tray.load(Ordering::SeqCst)
    }

    pub fn set_resident_in_tray(&self, resident: bool) {
        self.resident_in_tray.store(resident, Ordering::SeqCst);
    }

    pub fn set_tray_toggle_item(&self, item: CheckMenuItem<Wry>) {
        if let Ok(mut guard) = self.tray_toggle_item.lock() {
            *guard = Some(item);
        }
    }

    pub fn has_pending_fast(&self) -> bool {
        self.has_pending_flag.load(Ordering::Acquire)
    }

    pub fn has_pending(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn record_pending(&self, selection: PendingSelection) -> u64 {
        let mut guard = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *guard = Some(selection);
        self.has_pending_flag.store(true, Ordering::Release);
        next_gen
    }

    pub fn take_pending(&self) -> Option<PendingSelection> {
        let mut guard = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_some() {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        let taken = guard.take();
        self.has_pending_flag.store(false, Ordering::Release);
        taken
    }

    pub fn clear_pending(&self) -> bool {
        self.take_pending().is_some()
    }

    pub fn expire_generation(&self, expected_generation: u64) -> bool {
        let mut guard = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.generation.load(Ordering::SeqCst) == expected_generation && guard.is_some() {
            *guard = None;
            self.has_pending_flag.store(false, Ordering::Release);
            return true;
        }
        false
    }

    pub fn status(&self) -> SelectionAssistantStatus {
        SelectionAssistantStatus {
            popup_enabled: self.is_popup_enabled(),
            accessibility_trusted: is_accessibility_trusted(),
            resident_in_tray: self.is_resident_in_tray(),
        }
    }
}

pub fn hide_pill_window<R: Runtime>(app: &AppHandle<R>) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(pill) = app_for_main.get_webview_window(SELECTION_PILL_WINDOW_LABEL) {
            let _ = pill.hide();
        }
    });
}

pub fn apply_popup_enabled(
    app: &AppHandle,
    assistant: &SelectionAssistantState,
    enabled: bool,
) -> SelectionAssistantStatus {
    assistant.set_popup_enabled(enabled);
    if !enabled {
        hide_pill_window(app);
    }
    let status = assistant.status();
    let _ = app.emit(ASSISTANT_STATUS_EVENT, &status);
    status
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

    use super::{
        show_pill_window_near_cursor, AxSelectionRead, GestureOutcome, MouseGestureEvent,
        PendingSelection, Point, Rect, SelectionAssistantState, SelectionCandidatePayload,
        SelectionGestureDetector, DEFAULT_WINDOW_MARGIN, PILL_AUTO_DISMISS_DURATION,
        SELECTION_CANDIDATE_EVENT, SELECTION_PILL_WINDOW_LABEL, SELECTION_SETTLE_DELAY,
        is_analyzable_selection,
    };

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFNumberRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopMode = CFStringRef;
    type CFIndex = isize;
    type CFTypeID = usize;
    type CFStringEncoding = u32;
    type CFNumberType = CFIndex;

    type AXUIElementRef = CFTypeRef;
    type AXError = i32;

    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CGEventMask = u64;
    type CGEventType = u32;
    type CGEventField = u32;
    type CGEventTapLocation = u32;
    type CGEventTapPlacement = u32;
    type CGEventTapOptions = u32;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    const K_CF_STRING_ENCODING_UTF8: CFStringEncoding = 0x0800_0100;
    const K_CF_NUMBER_SINT32_TYPE: CFNumberType = 3;
    const K_AX_ERROR_SUCCESS: AXError = 0;

    const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
    const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;

    const K_CG_SESSION_EVENT_TAP: CGEventTapLocation = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP: CGEventTapPlacement = 0;
    const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: CGEventTapOptions = 0x0000_0001;

    const K_CG_EVENT_LEFT_MOUSE_DOWN: CGEventType = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: CGEventType = 2;
    const K_CG_EVENT_RIGHT_MOUSE_DOWN: CGEventType = 3;
    const K_CG_EVENT_LEFT_MOUSE_DRAGGED: CGEventType = 6;
    const K_CG_EVENT_SCROLL_WHEEL: CGEventType = 22;
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: CGEventType = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: CGEventType = 0xFFFF_FFFF;

    const K_CG_MOUSE_EVENT_CLICK_STATE: CGEventField = 1;
    const TAP_RETRY_INTERVAL: Duration = Duration::from_secs(5);

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        #[link_name = "kCFRunLoopCommonModes"]
        static K_CF_RUN_LOOP_COMMON_MODES: CFRunLoopMode;

        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
        fn CFStringGetTypeID() -> CFTypeID;
        fn CFNumberGetTypeID() -> CFTypeID;
        fn CFNumberGetValue(
            number: CFNumberRef,
            the_type: CFNumberType,
            value_ptr: *mut c_void,
        ) -> u8;
        fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        fn CFStringCreateWithBytes(
            alloc: CFAllocatorRef,
            bytes: *const u8,
            num_bytes: CFIndex,
            encoding: CFStringEncoding,
            is_external_representation: u8,
        ) -> CFStringRef;
        fn CFStringGetLength(the_string: CFStringRef) -> CFIndex;
        fn CFStringGetMaximumSizeForEncoding(
            length: CFIndex,
            encoding: CFStringEncoding,
        ) -> CFIndex;
        fn CFStringGetCString(
            the_string: CFStringRef,
            buffer: *mut c_char,
            buffer_size: CFIndex,
            encoding: CFStringEncoding,
        ) -> u8;
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: CFIndex,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(
            rl: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFRunLoopMode,
        );
        fn CFRunLoopRun();
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
        fn CGEventTapCreate(
            tap: CGEventTapLocation,
            place: CGEventTapPlacement,
            options: CGEventTapOptions,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: CGEventField) -> i64;
    }

    struct CfGuard(CFTypeRef);

    impl Drop for CfGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CFRelease(self.0);
                }
            }
        }
    }

    fn create_cfstring(text: &str) -> CfGuard {
        let ptr = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as CFIndex,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        };
        CfGuard(ptr)
    }

    fn cfstring_to_string(cf_str: CFStringRef) -> Option<String> {
        if cf_str.is_null() {
            return None;
        }
        let utf16_len = unsafe { CFStringGetLength(cf_str) };
        if utf16_len <= 0 {
            return Some(String::new());
        }
        let max_size =
            unsafe { CFStringGetMaximumSizeForEncoding(utf16_len, K_CF_STRING_ENCODING_UTF8) }
                .saturating_add(1);
        if max_size <= 0 {
            return None;
        }
        let mut buf = vec![0u8; max_size as usize];
        let ok = unsafe {
            CFStringGetCString(
                cf_str,
                buf.as_mut_ptr().cast::<c_char>(),
                max_size,
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        if ok == 0 {
            return None;
        }
        CStr::from_bytes_until_nul(&buf)
            .ok()
            .and_then(|cstr| cstr.to_str().ok())
            .map(str::to_owned)
    }

    pub fn is_accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    fn is_non_selectable_ax_role(role: &str) -> bool {
        matches!(
            role,
            "AXScrollBar"
                | "AXSlider"
                | "AXSplitter"
                | "AXGrowArea"
                | "AXMenuItem"
                | "AXToolbar"
                | "AXValueIndicator"
                | "AXImage"
                | "AXButton"
                | "AXCheckBox"
                | "AXRadioButton"
                | "AXPopUpButton"
                | "AXMenuButton"
                | "AXTabGroup"
                | "AXOutline"
                | "AXList"
                | "AXIcon"
                | "AXColorWell"
                | "AXBusyIndicator"
                | "AXProgressIndicator"
                | "AXIncrementor"
        )
    }

    fn is_non_text_window_owner(owner_name: &str) -> bool {
        matches!(
            owner_name,
            "Finder"
                | "Dock"
                | "Window Server"
                | "SystemUIServer"
                | "Control Center"
                | "Notification Center"
                | "ScreenSaverEngine"
                | "loginwindow"
        )
    }

    struct FrontmostWindowOwner {
        pid: Option<i32>,
        owner_name: Option<String>,
    }

    fn inspect_topmost_window_owner() -> FrontmostWindowOwner {
        let options = K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY
            | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS;
        let list_raw = unsafe { CGWindowListCopyWindowInfo(options, 0) };
        if list_raw.is_null() {
            return FrontmostWindowOwner {
                pid: None,
                owner_name: None,
            };
        }
        let list_guard = CfGuard(list_raw);
        let count = unsafe { CFArrayGetCount(list_guard.0) };

        let key_layer = create_cfstring("kCGWindowLayer");
        let key_pid = create_cfstring("kCGWindowOwnerPID");
        let key_owner = create_cfstring("kCGWindowOwnerName");
        if key_layer.0.is_null() || key_pid.0.is_null() || key_owner.0.is_null() {
            return FrontmostWindowOwner {
                pid: None,
                owner_name: None,
            };
        }

        for idx in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(list_guard.0, idx) };
            if dict.is_null() {
                continue;
            }
            let layer_ref = unsafe { CFDictionaryGetValue(dict, key_layer.0) };
            if layer_ref.is_null() || unsafe { CFGetTypeID(layer_ref) != CFNumberGetTypeID() } {
                continue;
            }
            let mut layer: i32 = -1;
            let layer_ok = unsafe {
                CFNumberGetValue(
                    layer_ref,
                    K_CF_NUMBER_SINT32_TYPE,
                    (&mut layer as *mut i32).cast::<c_void>(),
                )
            };
            if layer_ok == 0 || layer != 0 {
                continue;
            }

            let mut pid: Option<i32> = None;
            let pid_ref = unsafe { CFDictionaryGetValue(dict, key_pid.0) };
            if !pid_ref.is_null() && unsafe { CFGetTypeID(pid_ref) == CFNumberGetTypeID() } {
                let mut raw_pid: i32 = 0;
                let pid_ok = unsafe {
                    CFNumberGetValue(
                        pid_ref,
                        K_CF_NUMBER_SINT32_TYPE,
                        (&mut raw_pid as *mut i32).cast::<c_void>(),
                    )
                };
                if pid_ok != 0 && raw_pid > 0 {
                    pid = Some(raw_pid);
                }
            }

            let mut owner_name: Option<String> = None;
            let owner_ref = unsafe { CFDictionaryGetValue(dict, key_owner.0) };
            if !owner_ref.is_null() && unsafe { CFGetTypeID(owner_ref) == CFStringGetTypeID() } {
                owner_name = cfstring_to_string(owner_ref);
            }

            return FrontmostWindowOwner { pid, owner_name };
        }

        FrontmostWindowOwner {
            pid: None,
            owner_name: None,
        }
    }

    pub fn read_ax_selected_text() -> AxSelectionRead {
        if !is_accessibility_trusted() {
            return AxSelectionRead::Unavailable;
        }

        let system_wide = CfGuard(unsafe { AXUIElementCreateSystemWide() });
        if system_wide.0.is_null() {
            return AxSelectionRead::Unavailable;
        }

        let attr_focused = create_cfstring("AXFocusedUIElement");
        if attr_focused.0.is_null() {
            return AxSelectionRead::Unavailable;
        }

        let mut focused_raw: CFTypeRef = std::ptr::null();
        let focused_err = unsafe {
            AXUIElementCopyAttributeValue(system_wide.0, attr_focused.0, &mut focused_raw)
        };
        if focused_err != K_AX_ERROR_SUCCESS || focused_raw.is_null() {
            return AxSelectionRead::Unavailable;
        }
        let focused_el = CfGuard(focused_raw);

        let attr_selected = create_cfstring("AXSelectedText");
        if attr_selected.0.is_null() {
            return AxSelectionRead::Unavailable;
        }

        let mut selected_raw: CFTypeRef = std::ptr::null();
        let selected_err = unsafe {
            AXUIElementCopyAttributeValue(focused_el.0, attr_selected.0, &mut selected_raw)
        };
        if selected_err != K_AX_ERROR_SUCCESS || selected_raw.is_null() {
            let attr_role = create_cfstring("AXRole");
            if !attr_role.0.is_null() {
                let mut role_raw: CFTypeRef = std::ptr::null();
                let role_err = unsafe {
                    AXUIElementCopyAttributeValue(focused_el.0, attr_role.0, &mut role_raw)
                };
                if role_err == K_AX_ERROR_SUCCESS && !role_raw.is_null() {
                    let role_val = CfGuard(role_raw);
                    if unsafe { CFGetTypeID(role_val.0) == CFStringGetTypeID() } {
                        if let Some(role_name) = cfstring_to_string(role_val.0) {
                            if is_non_selectable_ax_role(role_name.trim()) {
                                return AxSelectionRead::ExplicitlyEmpty;
                            }
                        }
                    }
                }
            }
            return AxSelectionRead::Unavailable;
        }
        let selected_val = CfGuard(selected_raw);

        let is_string = unsafe { CFGetTypeID(selected_val.0) == CFStringGetTypeID() };
        if !is_string {
            return AxSelectionRead::Unavailable;
        }

        let Some(raw_text) = cfstring_to_string(selected_val.0) else {
            return AxSelectionRead::Unavailable;
        };
        let trimmed = raw_text.trim();
        if trimmed.is_empty() {
            AxSelectionRead::ExplicitlyEmpty
        } else {
            AxSelectionRead::Selected(trimmed.to_string())
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct TapDispatch {
        outcome: GestureOutcome,
        event_logical: (f64, f64),
        down_logical: Option<(f64, f64)>,
    }

    struct TapCallbackContext {
        epoch: Instant,
        assistant: Arc<SelectionAssistantState>,
        has_active_press: AtomicBool,
        detector: Mutex<SelectionGestureDetector>,
        down_logical: Mutex<Option<(f64, f64)>>,
        tap_port: AtomicPtr<c_void>,
        tx: Sender<TapDispatch>,
    }

    unsafe extern "C" fn event_tap_callback(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if user_info.is_null() {
            return event;
        }
        let ctx = unsafe { &*(user_info.cast::<TapCallbackContext>()) };

        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            let tap = ctx.tap_port.load(Ordering::SeqCst);
            if !tap.is_null() {
                unsafe {
                    CGEventTapEnable(tap, true);
                }
            }
            return event;
        }

        if event.is_null() {
            return event;
        }

        if (event_type == K_CG_EVENT_SCROLL_WHEEL || event_type == K_CG_EVENT_RIGHT_MOUSE_DOWN)
            && !ctx.assistant.has_pending_fast()
            && !ctx.has_active_press.load(Ordering::Acquire)
        {
            return event;
        }

        let loc = unsafe { CGEventGetLocation(event) };
        let point = Point {
            x: loc.x.round() as i32,
            y: loc.y.round() as i32,
        };
        let at_ms = ctx.epoch.elapsed().as_millis() as u64;

        let mut detector = ctx
            .detector
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut down_slot = ctx
            .down_logical
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match event_type {
            K_CG_EVENT_LEFT_MOUSE_DOWN => {
                let clicks =
                    unsafe { CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_CLICK_STATE) };
                *down_slot = Some((loc.x, loc.y));
                ctx.has_active_press.store(true, Ordering::Release);
                let _ = detector.handle_event(MouseGestureEvent::LeftMouseDown {
                    point,
                    at_ms,
                    click_count: clicks,
                });
            }
            K_CG_EVENT_LEFT_MOUSE_DRAGGED => {
                let _ = detector.handle_event(MouseGestureEvent::LeftMouseDragged { point });
            }
            K_CG_EVENT_LEFT_MOUSE_UP => {
                ctx.has_active_press.store(false, Ordering::Release);
                let clicks =
                    unsafe { CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_CLICK_STATE) };
                let down_logical = down_slot.take();
                let outcome = detector.handle_event(MouseGestureEvent::LeftMouseUp {
                    point,
                    at_ms,
                    click_count: clicks,
                });
                if outcome != GestureOutcome::Ignore {
                    let _ = ctx.tx.send(TapDispatch {
                        outcome,
                        event_logical: (loc.x, loc.y),
                        down_logical,
                    });
                }
            }
            K_CG_EVENT_RIGHT_MOUSE_DOWN | K_CG_EVENT_SCROLL_WHEEL => {
                ctx.has_active_press.store(false, Ordering::Release);
                *down_slot = None;
                let outcome = detector.handle_event(MouseGestureEvent::OtherInteraction);
                if ctx.assistant.has_pending_fast() {
                    let _ = ctx.tx.send(TapDispatch {
                        outcome,
                        event_logical: (loc.x, loc.y),
                        down_logical: None,
                    });
                }
            }
            _ => {}
        }

        event
    }

    fn point_in_visible_window(win: &WebviewWindow, logical: (f64, f64)) -> bool {
        if !win.is_visible().unwrap_or(false) {
            return false;
        }
        let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
            return false;
        };
        let scale = win.scale_factor().unwrap_or(1.0);
        let phys_point = Point {
            x: (logical.0 * scale).round() as i32,
            y: (logical.1 * scale).round() as i32,
        };
        let rect = Rect {
            x: pos.x,
            y: pos.y,
            width: size.width as i32,
            height: size.height as i32,
        };
        rect.contains(phys_point)
    }

    fn is_in_own_visible_windows(
        app: &AppHandle,
        event_logical: (f64, f64),
        down_logical: Option<(f64, f64)>,
    ) -> bool {
        for label in ["main", SELECTION_PILL_WINDOW_LABEL] {
            if let Some(win) = app.get_webview_window(label) {
                if point_in_visible_window(&win, event_logical) {
                    return true;
                }
                if let Some(down) = down_logical {
                    if point_in_visible_window(&win, down) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn resolve_physical_cursor(win: &WebviewWindow, logical: (f64, f64)) -> Point {
        if let Ok(monitors) = win.available_monitors() {
            for monitor in &monitors {
                let scale = monitor.scale_factor();
                let candidate = Point {
                    x: (logical.0 * scale).round() as i32,
                    y: (logical.1 * scale).round() as i32,
                };
                let rect = Rect {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width as i32,
                    height: monitor.size().height as i32,
                };
                if rect.contains(candidate) {
                    return candidate;
                }
            }
        }
        let scale = win.scale_factor().unwrap_or(1.0);
        Point {
            x: (logical.0 * scale).round() as i32,
            y: (logical.1 * scale).round() as i32,
        }
    }

    fn arm_auto_dismiss_timer(
        app: AppHandle,
        assistant: Arc<SelectionAssistantState>,
        generation: u64,
    ) {
        std::thread::spawn(move || {
            std::thread::sleep(PILL_AUTO_DISMISS_DURATION);
            if assistant.expire_generation(generation) {
                let app_hide = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some(pill) = app_hide.get_webview_window(SELECTION_PILL_WINDOW_LABEL) {
                        let _ = pill.hide();
                    }
                });
            }
        });
    }

    enum ResolvedCandidateKind {
        Text(String),
        Deferred { source_pid: Option<i32> },
    }

    fn handle_worker_dispatch(
        app: &AppHandle,
        assistant: &Arc<SelectionAssistantState>,
        dispatch: TapDispatch,
    ) {
        if assistant.is_exiting() {
            return;
        }

        match dispatch.outcome {
            GestureOutcome::Ignore => {}
            GestureOutcome::DismissPill => {
                if !assistant.has_pending() {
                    return;
                }
                let app_main = app.clone();
                let assistant_main = Arc::clone(assistant);
                let _ = app.run_on_main_thread(move || {
                    if is_in_own_visible_windows(
                        &app_main,
                        dispatch.event_logical,
                        dispatch.down_logical,
                    ) {
                        return;
                    }
                    if assistant_main.clear_pending() {
                        if let Some(pill) = app_main.get_webview_window(SELECTION_PILL_WINDOW_LABEL)
                        {
                            let _ = pill.hide();
                        }
                    }
                });
            }
            GestureOutcome::Candidate { .. } => {
                if !assistant.is_popup_enabled() {
                    return;
                }

                std::thread::sleep(SELECTION_SETTLE_DELAY);
                if !assistant.is_popup_enabled() || assistant.is_exiting() {
                    return;
                }

                let owner = inspect_topmost_window_owner();
                let self_pid = std::process::id() as i32;
                if owner.pid == Some(self_pid) {
                    return;
                }

                let candidate_kind = match read_ax_selected_text() {
                    AxSelectionRead::ExplicitlyEmpty => return,
                    AxSelectionRead::Selected(text) => {
                        if !is_analyzable_selection(&text) {
                            return;
                        }
                        ResolvedCandidateKind::Text(text)
                    }
                    AxSelectionRead::Unavailable => {
                        if let Some(name) = owner.owner_name.as_deref() {
                            if is_non_text_window_owner(name.trim()) {
                                return;
                            }
                        }
                        ResolvedCandidateKind::Deferred {
                            source_pid: owner.pid,
                        }
                    }
                };

                let app_main = app.clone();
                let assistant_main = Arc::clone(assistant);
                let _ = app.run_on_main_thread(move || {
                    if !assistant_main.is_popup_enabled() || assistant_main.is_exiting() {
                        return;
                    }
                    if is_in_own_visible_windows(
                        &app_main,
                        dispatch.event_logical,
                        dispatch.down_logical,
                    ) {
                        return;
                    }
                    let Some(pill) = app_main.get_webview_window(SELECTION_PILL_WINDOW_LABEL)
                    else {
                        return;
                    };

                    let cursor = resolve_physical_cursor(&pill, dispatch.event_logical);
                    let (pending, preview_text) = match candidate_kind {
                        ResolvedCandidateKind::Text(text) => (
                            PendingSelection::Resolved {
                                text: text.clone(),
                                cursor,
                            },
                            Some(text),
                        ),
                        ResolvedCandidateKind::Deferred { source_pid } => (
                            PendingSelection::DeferredCopy { cursor, source_pid },
                            None,
                        ),
                    };

                    let generation = assistant_main.record_pending(pending);
                    let payload = SelectionCandidatePayload {
                        preview_text,
                        cursor,
                    };
                    let _ = app_main.emit(SELECTION_CANDIDATE_EVENT, &payload);

                    if let Err(err) =
                        show_pill_window_near_cursor(&pill, cursor, DEFAULT_WINDOW_MARGIN)
                    {
                        eprintln!("selection pill placement failed: {err}");
                        return;
                    }

                    arm_auto_dismiss_timer(app_main, assistant_main, generation);
                });
            }
        }
    }

    pub fn start_selection_monitor(app: AppHandle, assistant: Arc<SelectionAssistantState>) {
        let (tx, rx) = mpsc::channel::<TapDispatch>();

        let worker_app = app;
        let worker_assistant = Arc::clone(&assistant);
        std::thread::Builder::new()
            .name("ga-selection-worker".into())
            .spawn(move || {
                while let Ok(dispatch) = rx.recv() {
                    if worker_assistant.is_exiting() {
                        break;
                    }
                    handle_worker_dispatch(&worker_app, &worker_assistant, dispatch);
                }
            })
            .expect("spawn selection worker thread");

        std::thread::Builder::new()
            .name("ga-selection-event-tap".into())
            .spawn(move || {
                let ctx = Box::new(TapCallbackContext {
                    epoch: Instant::now(),
                    assistant: Arc::clone(&assistant),
                    has_active_press: AtomicBool::new(false),
                    detector: Mutex::new(SelectionGestureDetector::new()),
                    down_logical: Mutex::new(None),
                    tap_port: AtomicPtr::new(std::ptr::null_mut()),
                    tx,
                });
                let ctx_ptr = Box::into_raw(ctx);

                let event_mask: CGEventMask = (1u64 << K_CG_EVENT_LEFT_MOUSE_DOWN)
                    | (1u64 << K_CG_EVENT_LEFT_MOUSE_UP)
                    | (1u64 << K_CG_EVENT_RIGHT_MOUSE_DOWN)
                    | (1u64 << K_CG_EVENT_LEFT_MOUSE_DRAGGED)
                    | (1u64 << K_CG_EVENT_SCROLL_WHEEL);

                loop {
                    if assistant.is_exiting() {
                        unsafe {
                            drop(Box::from_raw(ctx_ptr));
                        }
                        return;
                    }

                    let tap = unsafe {
                        CGEventTapCreate(
                            K_CG_SESSION_EVENT_TAP,
                            K_CG_HEAD_INSERT_EVENT_TAP,
                            K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                            event_mask,
                            event_tap_callback,
                            ctx_ptr.cast::<c_void>(),
                        )
                    };

                    if tap.is_null() {
                        std::thread::sleep(TAP_RETRY_INTERVAL);
                        continue;
                    }

                    unsafe {
                        (*ctx_ptr).tap_port.store(tap, Ordering::SeqCst);
                    }
                    let _tap_guard = CfGuard(tap.cast_const());

                    let source =
                        unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0) };
                    if source.is_null() {
                        unsafe {
                            (*ctx_ptr)
                                .tap_port
                                .store(std::ptr::null_mut(), Ordering::SeqCst);
                        }
                        std::thread::sleep(TAP_RETRY_INTERVAL);
                        continue;
                    }
                    let _source_guard = CfGuard(source.cast_const());

                    unsafe {
                        let rl = CFRunLoopGetCurrent();
                        CFRunLoopAddSource(rl, source, K_CF_RUN_LOOP_COMMON_MODES);
                        CGEventTapEnable(tap, true);
                        CFRunLoopRun();
                        (*ctx_ptr)
                            .tap_port
                            .store(std::ptr::null_mut(), Ordering::SeqCst);
                    }

                    if assistant.is_exiting() {
                        unsafe {
                            drop(Box::from_raw(ctx_ptr));
                        }
                        return;
                    }
                    std::thread::sleep(TAP_RETRY_INTERVAL);
                }
            })
            .expect("spawn selection event tap thread");
    }
}

#[cfg(target_os = "macos")]
pub use macos::{is_accessibility_trusted, read_ax_selected_text, start_selection_monitor};

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn read_ax_selected_text() -> AxSelectionRead {
    AxSelectionRead::Unavailable
}

#[cfg(not(target_os = "macos"))]
pub fn start_selection_monitor(_app: AppHandle, _assistant: std::sync::Arc<SelectionAssistantState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_detector_dismisses_plain_click_and_short_jitter_click() {
        let mut detector = SelectionGestureDetector::new();

        assert_eq!(
            detector.handle_event(MouseGestureEvent::LeftMouseDown {
                point: Point { x: 200, y: 300 },
                at_ms: 1_000,
                click_count: 1,
            }),
            GestureOutcome::Ignore
        );
        assert_eq!(
            detector.handle_event(MouseGestureEvent::LeftMouseUp {
                point: Point { x: 200, y: 300 },
                at_ms: 1_080,
                click_count: 1,
            }),
            GestureOutcome::DismissPill
        );

        assert_eq!(
            detector.handle_event(MouseGestureEvent::LeftMouseDown {
                point: Point { x: 200, y: 300 },
                at_ms: 2_000,
                click_count: 1,
            }),
            GestureOutcome::Ignore
        );
        assert_eq!(
            detector.handle_event(MouseGestureEvent::LeftMouseDragged {
                point: Point { x: 205, y: 304 },
            }),
            GestureOutcome::Ignore
        );
        assert_eq!(
            detector.handle_event(MouseGestureEvent::LeftMouseUp {
                point: Point { x: 205, y: 304 },
                at_ms: 2_090,
                click_count: 1,
            }),
            GestureOutcome::DismissPill
        );
    }

    #[test]
    fn gesture_detector_accepts_valid_drag_and_enforces_duration_bounds() {
        let mut detector = SelectionGestureDetector::new();

        detector.on_mouse_down(Point { x: 100, y: 120 }, 500, 1);
        detector.on_mouse_drag(Point { x: 180, y: 122 });
        assert_eq!(
            detector.on_mouse_up(Point { x: 260, y: 124 }, 760, 1),
            GestureOutcome::Candidate {
                cursor: Point { x: 260, y: 124 }
            }
        );

        detector.on_mouse_down(Point { x: 320, y: 200 }, 800, 1);
        detector.on_mouse_drag(Point { x: 180, y: 196 });
        assert_eq!(
            detector.on_mouse_up(Point { x: 140, y: 194 }, 960, 1),
            GestureOutcome::Candidate {
                cursor: Point { x: 140, y: 194 }
            }
        );

        detector.on_mouse_down(Point { x: 100, y: 120 }, 1_000, 1);
        detector.on_mouse_drag(Point { x: 140, y: 120 });
        assert_eq!(
            detector.on_mouse_up(Point { x: 140, y: 120 }, 1_020, 1),
            GestureOutcome::Ignore
        );

        detector.on_mouse_down(Point { x: 100, y: 120 }, 2_000, 1);
        detector.on_mouse_drag(Point { x: 240, y: 120 });
        assert_eq!(
            detector.on_mouse_up(Point { x: 240, y: 120 }, 18_000, 1),
            GestureOutcome::Ignore
        );

        detector.on_mouse_down(Point { x: 100, y: 120 }, 3_000, 1);
        assert_eq!(
            detector.on_mouse_up(Point { x: 240, y: 120 }, 3_200, 1),
            GestureOutcome::Ignore
        );
    }

    #[test]
    fn gesture_detector_detects_double_and_triple_clicks_and_dismisses_other_interactions() {
        let mut detector = SelectionGestureDetector::new();

        detector.on_mouse_down(Point { x: 420, y: 260 }, 100, 2);
        assert_eq!(
            detector.on_mouse_up(Point { x: 424, y: 262 }, 140, 2),
            GestureOutcome::Candidate {
                cursor: Point { x: 424, y: 262 }
            }
        );

        detector.on_mouse_down(Point { x: 420, y: 260 }, 300, 3);
        assert_eq!(
            detector.on_mouse_up(Point { x: 420, y: 260 }, 340, 3),
            GestureOutcome::Candidate {
                cursor: Point { x: 420, y: 260 }
            }
        );

        detector.on_mouse_down(Point { x: 420, y: 260 }, 500, 2);
        assert_eq!(
            detector.on_mouse_up(Point { x: 450, y: 260 }, 540, 2),
            GestureOutcome::Ignore
        );

        detector.on_mouse_down(Point { x: 100, y: 100 }, 600, 1);
        detector.on_mouse_drag(Point { x: 180, y: 100 });
        assert_eq!(
            detector.handle_event(MouseGestureEvent::OtherInteraction),
            GestureOutcome::DismissPill
        );
        assert_eq!(
            detector.on_mouse_up(Point { x: 180, y: 100 }, 700, 1),
            GestureOutcome::Ignore
        );
    }

    #[test]
    fn is_analyzable_selection_filters_noise_and_accepts_english_text() {
        assert!(is_analyzable_selection(
            "The book Maya recommended won a prize."
        ));
        assert!(is_analyzable_selection("  syntax  "));
        assert!(is_analyzable_selection(&"a".repeat(2_400)));

        assert!(!is_analyzable_selection(""));
        assert!(!is_analyzable_selection("   \n\t  "));
        assert!(!is_analyzable_selection("2026"));
        assert!(!is_analyzable_selection("ok"));
        assert!(!is_analyzable_selection("纯中文界面标签 123"));
        assert!(!is_analyzable_selection(&"a".repeat(2_401)));
    }

    #[test]
    fn generation_guarded_auto_dismiss_never_clobbers_newer_pending_selection() {
        let state = SelectionAssistantState::new();
        assert!(state.is_popup_enabled());
        assert!(!state.is_exiting());
        assert!(!state.has_pending());

        let gen1 = state.record_pending(PendingSelection::Resolved {
            text: "First selected sentence.".into(),
            cursor: Point { x: 120, y: 200 },
        });
        assert!(state.has_pending());

        let gen2 = state.record_pending(PendingSelection::DeferredCopy {
            cursor: Point { x: 340, y: 420 },
            source_pid: Some(4242),
        });
        assert!(gen2 > gen1);
        assert!(state.has_pending_fast());

        assert!(!state.expire_generation(gen1));
        assert!(state.has_pending());
        assert!(state.has_pending_fast());

        assert!(state.expire_generation(gen2));
        assert!(!state.has_pending());
        assert!(!state.has_pending_fast());

        let gen3 = state.record_pending(PendingSelection::Resolved {
            text: "Confirmed sentence.".into(),
            cursor: Point { x: 90, y: 110 },
        });
        assert_eq!(
            state.take_pending(),
            Some(PendingSelection::Resolved {
                text: "Confirmed sentence.".into(),
                cursor: Point { x: 90, y: 110 },
            })
        );
        assert!(!state.has_pending_fast());
        assert!(!state.expire_generation(gen3));

        let gen4 = state.record_pending(PendingSelection::DeferredCopy {
            cursor: Point { x: 50, y: 60 },
            source_pid: None,
        });
        assert!(state.set_popup_enabled(false));
        assert!(!state.is_popup_enabled());
        assert!(!state.has_pending());
        assert!(!state.expire_generation(gen4));
    }
}
