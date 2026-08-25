//! The plugin's window, inside the host's.
//!
//! A plugin editor is a window the plugin makes and the host is handed. Where
//! it sits, how big it is and whether it has the keyboard are the host's to
//! say, and saying so means talking to the platform: `SetWindowPos` and
//! `SetFocus` on Windows, `NSView::setFrame` and `makeFirstResponder` on macOS.
//!
//! The host's own window is not here. That is eframe's, along with its title
//! bar, its position between runs and the windows its dialogs open in.

use std::ffi::c_void;

#[cfg(target_os = "windows")]
mod platform {
    use super::c_void;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindow, GetWindowLongPtrW, GetWindowRect, SendMessageW,
        SetWindowLongPtrW, SetWindowPos, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_SIZE, WM_TIMER,
        WM_WINDOWPOSCHANGED, WS_CLIPCHILDREN,
    };

    /// The host's window, which the plugin's is a child of.
    fn top_level(handle: *mut c_void) -> HWND {
        HWND(handle)
    }

    /// Whether the host's window takes input.
    ///
    /// A dialog is a window of its own, and turning this off for as long as one
    /// is open is what makes it modal: the host and everything inside it stop
    /// taking input, so the focus has nowhere else to go.
    pub fn set_enabled(handle: *mut c_void, enabled: bool) {
        unsafe {
            let _ = EnableWindow(top_level(handle), enabled);
        }
    }

    /// Which message carries the size the window has just been given, and what
    /// the strip above the plugin takes off the top. The subclass has one piece
    /// of data to carry, so the height rides in it.
    const TRACK_ID: usize = 1;

    /// Keep the plugin's window filling the host's as the host is dragged.
    ///
    /// Windows resizes the host's window as the pointer moves and repaints it
    /// straight away. Anything that puts the plugin's window right afterwards,
    /// on the next frame of a drawing loop, is a frame late every frame: the
    /// host paints the new shape while the plugin still has the old one, which
    /// is the flicker and the bare strip along the edge.
    ///
    /// So the plugin's window is moved from inside the resize itself, in the
    /// message that says the host's window changed. By the time anything is
    /// painted, both windows are the right size. `WS_CLIPCHILDREN` keeps the
    /// host out of the area the plugin owns, so there is nothing to paint over
    /// it even for an instant.
    pub fn track_editor(handle: *mut c_void, top: i32) {
        unsafe {
            let parent = top_level(handle);
            let style = GetWindowLongPtrW(parent, GWL_STYLE) as u32;
            SetWindowLongPtrW(parent, GWL_STYLE, (style | WS_CLIPCHILDREN.0) as isize);
            let _ = SetWindowPos(
                parent,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            let _ = SetWindowSubclass(parent, Some(track_proc), TRACK_ID, top as usize);
        }
    }

    unsafe extern "system" fn track_proc(
        hwnd: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
        _id: usize,
        top: usize,
    ) -> LRESULT {
        if message == WM_WINDOWPOSCHANGED || message == WM_SIZE {
            inset_editor(hwnd.0, top as i32);
            unsafe { draw_now(hwnd) };
        }
        unsafe { DefSubclassProc(hwnd, message, w, l) }
    }

    /// Ask the plugin to paint, now, before this resize is over.
    ///
    /// A window that has just been made bigger has an area it has never drawn
    /// in, and until something paints there it shows whatever the graphics card
    /// left behind, which is black. `RDW_UPDATENOW` is how a window is asked
    /// for that paint without waiting: it delivers `WM_PAINT` and returns once
    /// the window has answered it.
    ///
    /// The nudge afterwards is for a plugin that draws on a timer rather than
    /// on `WM_PAINT`, which is what baseview does and so what this project's
    /// own plugin does: its frame timer is id 4242, and Windows only delivers a
    /// timer message when nothing else is waiting. Nothing is ever waiting less
    /// than during a drag, so the frame it would draw never comes. A timer
    /// message it does not recognise costs any other plugin nothing.
    unsafe fn draw_now(parent: HWND) {
        unsafe {
            let Some(child) = editor_window(parent) else {
                return;
            };
            let _ = RedrawWindow(Some(child), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
            const BASEVIEW_FRAME_TIMER: usize = 4242;
            SendMessageW(child, WM_TIMER, Some(WPARAM(BASEVIEW_FRAME_TIMER)), None);
        }
    }

    /// Push the plugin's own window down, leaving `top` pixels for the host's
    /// strip, and size it to what is left.
    ///
    /// A plugin places its window at the top left of whatever it is given and
    /// sizes it to its editor, so making room has to be done from out here.
    pub fn inset_editor(handle: *mut c_void, top: i32) {
        unsafe {
            let parent = top_level(handle);
            let Some(child) = editor_window(parent) else {
                return;
            };
            let mut client = RECT::default();
            if GetClientRect(parent, &mut client).is_err() {
                return;
            }
            let width = client.right - client.left;
            let height = (client.bottom - client.top - top).max(1);
            let _ = SetWindowPos(child, None, 0, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    /// Give the plugin's editor the keyboard.
    ///
    /// A host opens a plugin's editor and hands it the keyboard, the same way
    /// it would hand it to any window it puts on screen. A plugin asking for
    /// it back on its own is a plugin fighting whatever else is open.
    pub fn focus_editor(handle: *mut c_void) {
        unsafe {
            let parent = top_level(handle);
            if let Some(child) = editor_window(parent) {
                let _ = SetFocus(Some(child));
            }
        }
    }

    /// The plugin's window: the child that fills the frame.
    ///
    /// `GW_CHILD` alone gives whichever child is first in Z-order, and a
    /// plugin may own more than one, so the biggest one wins.
    unsafe fn editor_window(parent: HWND) -> Option<HWND> {
        let mut best: Option<(HWND, i64)> = None;
        let mut child = unsafe { GetWindow(parent, GW_CHILD) }.ok();
        while let Some(window) = child {
            if window.0.is_null() {
                break;
            }
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(window, &mut rect) }.is_ok() {
                let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
                if best.is_none_or(|(_, best)| area > best) {
                    best = Some((window, area));
                }
            }
            child = unsafe { GetWindow(window, GW_HWNDNEXT) }.ok();
        }
        best.map(|(window, _)| window)
    }

}

#[cfg(target_os = "macos")]
mod platform {
    use super::c_void;
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// Whether the host's window takes input.
    ///
    /// AppKit has no equivalent of disabling a window, and a dialog opened as
    /// its own window is already the one AppKit makes key, so there is nothing
    /// to do here.
    pub fn set_enabled(_handle: *mut c_void, _enabled: bool) {}

    /// Keep the plugin's view filling the host's as the host is dragged.
    ///
    /// AppKit will do it: a view told it may stretch is resized with the view
    /// it is in, inside the resize rather than after it, so the two are never
    /// out of step and there is nothing to redraw late.
    pub fn track_editor(handle: *mut c_void, _top: i32) {
        let host: &NSView = unsafe { &*(handle as *const NSView) };
        host.setAutoresizesSubviews(true);
        let subviews = unsafe { host.subviews() };
        let Some(editor) = subviews.iter().next() else {
            return;
        };
        editor.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
    }

    /// Push the plugin's own view down, leaving `top` points for the host's
    /// strip, and size it to what is left.
    ///
    /// AppKit measures from the bottom left, so the room comes off the top by
    /// leaving the origin at zero and shortening the view.
    pub fn inset_editor(handle: *mut c_void, top: i32) {
        let host: &NSView = unsafe { &*(handle as *const NSView) };
        let bounds = host.bounds();
        let subviews = unsafe { host.subviews() };
        let Some(editor) = subviews.iter().next() else {
            return;
        };
        let height = (bounds.size.height - top as f64).max(1.0);
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(bounds.size.width, height),
        );
        unsafe { editor.setFrame(frame) };
    }

    /// Give the plugin's editor the keyboard, the way a host hands it to any
    /// window it puts on screen.
    pub fn focus_editor(handle: *mut c_void) {
        let host: &NSView = unsafe { &*(handle as *const NSView) };
        let subviews = unsafe { host.subviews() };
        let Some(editor) = subviews.iter().next() else {
            return;
        };
        if let Some(window) = unsafe { host.window() } {
            window.makeFirstResponder(Some(&editor));
        }
    }

}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::c_void;

    pub fn set_enabled(_handle: *mut c_void, _enabled: bool) {}

    pub fn track_editor(_handle: *mut c_void, _top: i32) {}

    pub fn inset_editor(_handle: *mut c_void, _top: i32) {}

    pub fn focus_editor(_handle: *mut c_void) {}
}

pub use platform::{focus_editor, inset_editor, set_enabled, track_editor};
