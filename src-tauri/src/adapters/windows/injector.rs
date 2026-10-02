/*!
 * WHAT:  Windows implementation of TextInjector.
 * WHY:   Uses Win32 SendInput (Ctrl + V) and arboard clipboard to paste into active window.
 */

use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

use crate::adapters::windows::permissions::WindowsPermissions;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::ports::injector::{
    FrontmostApp, InjectionOutcome, InjectionRequest, TextInjector,
};
use crate::types::DeliveryKind;

#[derive(Debug, Clone, Copy)]
pub struct PasteTiming {
    pub before_paste: Duration,
    pub before_restore: Duration,
}

impl Default for PasteTiming {
    fn default() -> Self {
        Self {
            before_paste: Duration::from_millis(40),
            before_restore: Duration::from_millis(150),
        }
    }
}

impl PasteTiming {
    fn from_request(request: &InjectionRequest) -> Self {
        Self {
            before_paste: Duration::from_millis(request.paste_delay_ms),
            before_restore: Duration::from_millis(request.clipboard_restore_delay_ms),
        }
    }
}

pub struct WindowsInjector {
    _permissions: WindowsPermissions,
}

impl WindowsInjector {
    pub fn new() -> Self {
        Self {
            _permissions: WindowsPermissions::new(),
        }
    }

    fn clipboard() -> AppResult<Clipboard> {
        Clipboard::new().map_err(|err| {
            AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Murmur could not reach the clipboard.",
            )
            .with_detail(err)
        })
    }

    fn post_paste() -> AppResult<()> {
        unsafe {
            let mut inputs: [INPUT; 4] = std::mem::zeroed();

            // 1. Ctrl Down
            inputs[0].r#type = INPUT_KEYBOARD;
            inputs[0].Anonymous.ki.wVk = VK_CONTROL;

            // 2. V Down
            inputs[1].r#type = INPUT_KEYBOARD;
            inputs[1].Anonymous.ki.wVk = VK_V;

            // 3. V Up
            inputs[2].r#type = INPUT_KEYBOARD;
            inputs[2].Anonymous.ki.wVk = VK_V;
            inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

            // 4. Ctrl Up
            inputs[3].r#type = INPUT_KEYBOARD;
            inputs[3].Anonymous.ki.wVk = VK_CONTROL;
            inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

            let sent = SendInput(
                4,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );

            if sent != 4 {
                return Err(AppError::new(
                    ErrorCode::InjectionFailed,
                    "Murmur could not send the paste (SendInput failed). Your text is on the clipboard.",
                ));
            }
        }
        Ok(())
    }
}

impl Default for WindowsInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInjector for WindowsInjector {
    fn can_inject(&self) -> bool {
        true
    }

    fn frontmost_app(&self) -> Option<FrontmostApp> {
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }

            let mut process_id: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut process_id);

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
            let name = if len > 0 {
                String::from_utf16_lossy(&title_buf[..len as usize])
            } else {
                format!("PID-{process_id}")
            };

            Some(FrontmostApp {
                bundle_id: format!("win-{process_id}"),
                name,
            })
        }
    }

    fn deliver(&self, request: &InjectionRequest) -> AppResult<InjectionOutcome> {
        let timing = PasteTiming::from_request(request);
        let mut clipboard = Self::clipboard()?;

        let previous = if request.restore_clipboard {
            clipboard.get_text().ok()
        } else {
            None
        };

        let clipboard_started = Instant::now();
        if let Err(e) = clipboard.set_text(&request.text) {
            return Err(AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Failed to write text to Windows clipboard.",
            )
            .with_detail(e));
        }
        let clipboard_write_ms = clipboard_started.elapsed().as_secs_f64() * 1000.0;

        if !request.auto_paste {
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: Some("Auto-paste is disabled in settings.".into()),
                clipboard_write_ms,
            });
        }

        thread::sleep(timing.before_paste);

        if let Err(err) = Self::post_paste() {
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: Some(format!("Paste failed: {err}")),
                clipboard_write_ms,
            });
        }

        if request.restore_clipboard {
            if let Some(prev_text) = previous {
                thread::sleep(timing.before_restore);
                let _ = clipboard.set_text(prev_text);
            }
        }

        Ok(InjectionOutcome {
            delivery: DeliveryKind::Pasted,
            reason: None,
            clipboard_write_ms,
        })
    }
}
