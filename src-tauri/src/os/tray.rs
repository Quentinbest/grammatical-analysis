use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::os::selection_monitor::{apply_popup_enabled, SelectionAssistantState};

const TRAY_ID: &str = "main-tray";
const MENU_ID_SHOW_MAIN: &str = "tray_show_main";
const MENU_ID_TOGGLE_POPUP: &str = "tray_toggle_popup";
const MENU_ID_QUIT: &str = "tray_quit";
const ICON_SIZE: u32 = 22;

fn fill_rect(rgba: &mut [u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32, alpha: u8) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            let idx = ((y * width + x) * 4) as usize;
            if idx + 3 < rgba.len() {
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = alpha;
            }
        }
    }
}

pub fn build_tray_template_icon() -> Image<'static> {
    let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];

    fill_rect(&mut rgba, ICON_SIZE, 2, 4, 3, 17, 255);
    fill_rect(&mut rgba, ICON_SIZE, 2, 4, 5, 5, 255);
    fill_rect(&mut rgba, ICON_SIZE, 2, 16, 5, 17, 255);

    fill_rect(&mut rgba, ICON_SIZE, 18, 4, 19, 17, 255);
    fill_rect(&mut rgba, ICON_SIZE, 16, 4, 19, 5, 255);
    fill_rect(&mut rgba, ICON_SIZE, 16, 16, 19, 17, 255);

    fill_rect(&mut rgba, ICON_SIZE, 8, 6, 14, 7, 255);
    fill_rect(&mut rgba, ICON_SIZE, 7, 7, 8, 10, 255);
    fill_rect(&mut rgba, ICON_SIZE, 8, 10, 13, 11, 255);
    fill_rect(&mut rgba, ICON_SIZE, 13, 11, 14, 14, 255);
    fill_rect(&mut rgba, ICON_SIZE, 7, 14, 13, 15, 255);

    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

pub fn show_and_focus_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn setup_tray(app: &AppHandle, assistant: Arc<SelectionAssistantState>) -> tauri::Result<()> {
    let show_main = MenuItem::with_id(app, MENU_ID_SHOW_MAIN, "显示主窗口", true, None::<&str>)?;
    let toggle_popup = CheckMenuItem::with_id(
        app,
        MENU_ID_TOGGLE_POPUP,
        "开启全局划词浮标",
        true,
        assistant.is_popup_enabled(),
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        MENU_ID_QUIT,
        "退出 Grammatical Analysis",
        true,
        None::<&str>,
    )?;

    assistant.set_tray_toggle_item(toggle_popup.clone());

    let menu = MenuBuilder::new(app)
        .item(&show_main)
        .item(&toggle_popup)
        .item(&separator)
        .item(&quit)
        .build()?;

    let assistant_for_menu = Arc::clone(&assistant);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(build_tray_template_icon())
        .icon_as_template(true)
        .tooltip("Grammatical Analysis")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app_handle, event| match event.id().as_ref() {
            MENU_ID_SHOW_MAIN => {
                show_and_focus_main(app_handle);
            }
            MENU_ID_TOGGLE_POPUP => {
                let next = !assistant_for_menu.is_popup_enabled();
                let _ = apply_popup_enabled(app_handle, &assistant_for_menu, next);
            }
            MENU_ID_QUIT => {
                assistant_for_menu.set_exiting(true);
                app_handle.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_and_focus_main(tray.app_handle());
            }
        })
        .build(app)?;

    assistant.set_resident_in_tray(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_monochrome_22x22_template_icon_with_transparent_margins() {
        let icon = build_tray_template_icon();
        assert_eq!(icon.width(), 22);
        assert_eq!(icon.height(), 22);
        assert_eq!(icon.rgba().len(), 22 * 22 * 4);

        let mut opaque_pixels = 0usize;
        for px in icon.rgba().chunks_exact(4) {
            assert_eq!(px[0], 0);
            assert_eq!(px[1], 0);
            assert_eq!(px[2], 0);
            if px[3] == 255 {
                opaque_pixels += 1;
            }
        }
        assert!(opaque_pixels > 40);
        assert_eq!(icon.rgba()[3], 0);
    }
}
