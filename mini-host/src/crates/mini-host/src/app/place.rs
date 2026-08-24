//! Reading and setting the window's position on screen, and opening the
//! separate windows the host's dialogs live in.
//!
//! baseview sizes a window but has no say in where it lands, so this goes to
//! the platform: `SetWindowPos` on Windows, `NSWindow::setFrameOrigin` on
//! macOS. The saved position lives in the build cache, next to everything else
//! this tool produces.
//!
//! baseview can open a window inside another one, or one that blocks the
//! thread until it closes. A dialog is neither: it is a window of its own,
//! with a title bar, that can be moved about while the host keeps drawing. So
//! the frame is made here, through the platform, and the dialog's egui view is
//! parented into it.

use std::ffi::c_void;
use std::path::PathBuf;

/// Where the position is remembered between runs.
fn store() -> PathBuf {
    PathBuf::from(".cache").join("mini-host-window.txt")
}

pub fn load() -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(store()).ok()?;
    let (x, y) = text.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

pub fn save(position: (i32, i32)) {
    let path = store();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{},{}", position.0, position.1));
}

#[cfg(target_os = "windows")]
mod platform {
    use super::c_void;
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetActiveWindow, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRect, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindow,
        GetWindowRect, IsWindow, LoadCursorW, RegisterClassW, SetWindowPos, CW_USEDEFAULT,
        GW_CHILD, GW_HWNDNEXT, IDC_ARROW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WINDOW_EX_STYLE,
        WNDCLASSW, WS_CAPTION, WS_SYSMENU, WS_VISIBLE,
    };

    /// The top-level window owning `handle`, which is the one that moves.
    fn top_level(handle: *mut c_void) -> HWND {
        HWND(handle)
    }

    pub fn get(handle: *mut c_void) -> Option<(i32, i32)> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(top_level(handle), &mut rect).ok()? };
        Some((rect.left, rect.top))
    }

    pub fn set(handle: *mut c_void, x: i32, y: i32) {
        unsafe {
            let _ = SetWindowPos(
                top_level(handle),
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
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

    /// A window of its own: title bar, close button, and draggable.
    ///
    /// It is owned by the host's window, so it stays in front of it and goes
    /// away with it, but it is not a child: it has its own frame and can be
    /// moved anywhere on the desktop. Messages reach it through the host's own
    /// loop, because a Win32 message loop serves every window on its thread.
    ///
    /// While it is up, the owner is disabled. That is what makes it modal: the
    /// owner and every window inside it stop taking input, so the focus has
    /// nowhere else to go and nothing can take it back.
    pub struct Frame {
        hwnd: HWND,
        owner: HWND,
    }

    /// The class the dialog frames are made from. The default handling is all
    /// the behaviour needed: the egui view inside does everything else, and
    /// `WM_CLOSE` falls through to destroying the window.
    const CLASS: PCWSTR = w!("MiniHostDialogFrame");

    unsafe extern "system" fn frame_proc(
        hwnd: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, message, w, l) }
    }

    fn register(instance: HINSTANCE) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(frame_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
                ..Default::default()
            };
            unsafe { RegisterClassW(&class) };
        });
    }

    pub fn open_frame(title: &str, width: i32, height: i32, owner: *mut c_void) -> Option<Frame> {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).ok()?.into();
            register(instance);

            // A fixed size: there is nothing in either dialog worth resizing,
            // and no resizing means no plumbing to keep the view in step.
            let style = WS_CAPTION | WS_SYSMENU | WS_VISIBLE;
            let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
            let _ = AdjustWindowRect(&mut rect, style, false);
            let outer = (rect.right - rect.left, rect.bottom - rect.top);
            let (x, y) = centred_over(owner, outer.0, outer.1);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                &HSTRING::from(title),
                style,
                x,
                y,
                outer.0,
                outer.1,
                Some(HWND(owner)),
                None,
                Some(instance),
                None,
            )
            .ok()?;
            let owner = HWND(owner);
            let _ = EnableWindow(owner, false);
            let _ = SetActiveWindow(hwnd);
            Some(Frame { hwnd, owner })
        }
    }

    /// Where a window of this size sits to be centred on the host's.
    fn centred_over(owner: *mut c_void, width: i32, height: i32) -> (i32, i32) {
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(HWND(owner), &mut rect).is_err() {
                return (CW_USEDEFAULT, CW_USEDEFAULT);
            }
            (
                rect.left + (rect.right - rect.left - width) / 2,
                rect.top + (rect.bottom - rect.top - height) / 2,
            )
        }
    }

    impl Frame {
        /// The handle the dialog's view is parented into.
        pub fn handle(&self) -> *mut c_void {
            self.hwnd.0
        }

        /// False once the title bar's close button has been used.
        pub fn is_open(&self) -> bool {
            unsafe { IsWindow(Some(self.hwnd)).as_bool() }
        }

        /// Size the view inside to the whole of the frame, and give it the
        /// keyboard.
        ///
        /// Both are the window manager's business and are done once, here.
        /// Activating a window puts the keyboard on the frame, and the view
        /// drawing inside it is a window of its own, so the frame passes it
        /// on. Nothing above this needs to know or ask.
        pub fn fit_contents(&self) {
            unsafe {
                let Ok(child) = GetWindow(self.hwnd, GW_CHILD) else {
                    return;
                };
                let mut client = RECT::default();
                if GetClientRect(self.hwnd, &mut client).is_err() {
                    return;
                }
                let _ = SetWindowPos(
                    child,
                    None,
                    0,
                    0,
                    client.right - client.left,
                    client.bottom - client.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = SetFocus(Some(child));
            }
        }
    }

    impl Drop for Frame {
        fn drop(&mut self) {
            unsafe {
                // The owner takes input again before the dialog goes, so the
                // focus lands back on it rather than on whatever is behind.
                let _ = EnableWindow(self.owner, true);
                let _ = SetActiveWindow(self.owner);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::c_void;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSBackingStoreType, NSView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    /// The window the view sits in. A view that is not in one yet has none.
    fn window(handle: *mut c_void) -> Option<Retained<NSWindow>> {
        let view: &NSView = unsafe { &*(handle as *const NSView) };
        unsafe { view.window() }
    }

    pub fn get(handle: *mut c_void) -> Option<(i32, i32)> {
        let window = window(handle)?;
        let frame = window.frame();
        Some((frame.origin.x as i32, frame.origin.y as i32))
    }

    pub fn set(handle: *mut c_void, x: i32, y: i32) {
        if let Some(window) = window(handle) {
            unsafe { window.setFrameOrigin(NSPoint::new(x as f64, y as f64)) };
        }
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

    /// A window of its own: title bar, close button, and draggable.
    ///
    /// AppKit's run loop is already going, with the host's window running in
    /// it, so this is an ordinary `NSWindow` ordered to the front, not a
    /// second application.
    ///
    /// While it is up it is a child of the host's window: it stays above it,
    /// moves with it, and is the window AppKit makes key. `runModal` would
    /// give a stronger guarantee, but it spins a run loop of its own, and this
    /// is opened from inside a frame the existing one is drawing.
    pub struct Frame {
        window: Retained<NSWindow>,
        owner: Option<Retained<NSWindow>>,
    }

    pub fn open_frame(title: &str, width: i32, height: i32, owner: *mut c_void) -> Option<Frame> {
        let mtm = MainThreadMarker::new()?;
        let content = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width as f64, height as f64),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                content,
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(title));
        window.setReleasedWhenClosed(false);

        // Centred on the host's window, or on the screen if it has none yet.
        let host: &NSView = unsafe { &*(owner as *const NSView) };
        let owner = unsafe { host.window() };
        match &owner {
            Some(over) => {
                let frame = over.frame();
                window.setFrameOrigin(NSPoint::new(
                    frame.origin.x + (frame.size.width - width as f64) / 2.0,
                    frame.origin.y + (frame.size.height - height as f64) / 2.0,
                ));
                over.addChildWindow_ordered(&window, NSWindowOrderingMode::Above);
            }
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
        Some(Frame { window, owner })
    }

    impl Frame {
        /// The view the dialog's own view is parented into.
        pub fn handle(&self) -> *mut c_void {
            match self.window.contentView() {
                Some(view) => Retained::as_ptr(&view) as *mut c_void,
                None => std::ptr::null_mut(),
            }
        }

        /// False once the title bar's close button has been used.
        pub fn is_open(&self) -> bool {
            self.window.isVisible()
        }

        /// Size the view inside to the whole of the frame, and give it the
        /// keyboard. Both are the window's business and are done once, here.
        pub fn fit_contents(&self) {
            let Some(content) = self.window.contentView() else {
                return;
            };
            let bounds = content.bounds();
            let subviews = unsafe { content.subviews() };
            let Some(view) = subviews.iter().next() else {
                return;
            };
            unsafe { view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), bounds.size)) };
            self.window.makeFirstResponder(Some(&view));
        }
    }

    impl Drop for Frame {
        fn drop(&mut self) {
            if let Some(owner) = &self.owner {
                owner.removeChildWindow(&self.window);
                owner.makeKeyAndOrderFront(None);
            }
            self.window.close();
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod platform {
    use super::c_void;

    pub fn get(_handle: *mut c_void) -> Option<(i32, i32)> {
        None
    }

    pub fn set(_handle: *mut c_void, _x: i32, _y: i32) {}

    pub fn inset_editor(_handle: *mut c_void, _top: i32) {}

    pub fn focus_editor(_handle: *mut c_void) {}

    pub struct Frame;

    pub fn open_frame(
        _title: &str,
        _width: i32,
        _height: i32,
        _owner: *mut c_void,
    ) -> Option<Frame> {
        None
    }

    impl Frame {
        pub fn handle(&self) -> *mut c_void {
            std::ptr::null_mut()
        }

        pub fn is_open(&self) -> bool {
            false
        }

        pub fn fit_contents(&self) {}
    }
}

pub use platform::{focus_editor, get, inset_editor, open_frame, set, Frame};

/// So a dialog's egui view can be opened inside the frame.
impl raw_window_handle::HasWindowHandle for Frame {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let handle = self.handle();
        if handle.is_null() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }

        #[cfg(target_os = "windows")]
        let raw = {
            let hwnd = std::num::NonZeroIsize::new(handle as isize)
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(
                hwnd,
            ))
        };
        #[cfg(target_os = "macos")]
        let raw = {
            let view = std::ptr::NonNull::new(handle)
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            raw_window_handle::RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(
                view,
            ))
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let raw: raw_window_handle::RawWindowHandle =
            return Err(raw_window_handle::HandleError::NotSupported);

        // Safety: the frame outlives the view opened into it. The view is
        // dropped first, in `Dialog`'s field order.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}
