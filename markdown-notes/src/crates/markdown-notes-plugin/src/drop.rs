//! Files dropped on the editor's window.
//!
//! The window is baseview's, and baseview registers a drop target of its own
//! on it, whose drops egui-baseview does not pass on. The plugin puts its own
//! target on the same window in its place: on Windows an `IDropTarget`
//! registered with OLE, on macOS a view registered for dragged files laid
//! over the editor's. Either one reads the dropped paths, keeps the PNG and
//! JPEG files among them, and queues them for the next frame, which puts them
//! into the note at the caret.

use crate::gui::{Incoming, ParentWindow};

/// The target on the editor's window, for as long as the window is open.
/// Dropping it takes the target off the window.
pub struct DropTarget {
    #[cfg(target_os = "windows")]
    hwnd: windows::Win32::Foundation::HWND,
    #[cfg(target_os = "macos")]
    overlay: objc2::rc::Retained<macos::DropView>,
}

/// Put a drop target on the editor's window, which is the child window that
/// baseview opened inside `parent`. Nothing when there is no such window yet,
/// or the platform has no way to.
pub fn accept(parent: &ParentWindow, incoming: Incoming) -> Option<DropTarget> {
    #[cfg(target_os = "windows")]
    {
        windows_target::accept(parent, incoming)
    }
    #[cfg(target_os = "macos")]
    {
        macos::accept(parent, incoming)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (parent, incoming);
        None
    }
}

/// Queue the PNG and JPEG files among `paths`.
fn take(incoming: &Incoming, paths: impl IntoIterator<Item = std::path::PathBuf>) {
    let pictures: Vec<crate::pictures::Incoming> = paths
        .into_iter()
        .filter_map(|path| crate::pictures::Incoming::from_file(&path))
        .collect();
    if pictures.is_empty() {
        return;
    }
    if let Ok(mut queue) = incoming.lock() {
        queue.extend(pictures);
    }
}

#[cfg(target_os = "windows")]
mod windows_target {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::implement;
    use windows::Win32::Foundation::{HWND, POINTL};
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{
        IDropTarget, IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop, CF_HDROP,
        DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindow, GW_CHILD};
    use windows_core::Ref;

    use super::DropTarget;
    use crate::gui::{Incoming, ParentWindow};

    pub fn accept(parent: &ParentWindow, incoming: Incoming) -> Option<DropTarget> {
        let RawWindowHandle::Win32(handle) = parent.window_handle().ok()?.as_raw() else {
            return None;
        };
        let parent = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        // The editor is the one child baseview put in the host's window.
        let hwnd = unsafe { GetWindow(parent, GW_CHILD) }.ok()?;
        let target: IDropTarget = Target { incoming }.into();
        unsafe {
            // baseview's own target comes off first: a window takes one.
            let _ = RevokeDragDrop(hwnd);
            RegisterDragDrop(hwnd, &target).ok()?;
        }
        Some(DropTarget { hwnd })
    }

    impl Drop for DropTarget {
        fn drop(&mut self) {
            unsafe {
                let _ = RevokeDragDrop(self.hwnd);
            }
        }
    }

    #[implement(IDropTarget)]
    struct Target {
        incoming: Incoming,
    }

    /// The files a drag is carrying, or none when it carries something else.
    fn files_in(data: &IDataObject) -> Vec<PathBuf> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut paths = Vec::new();
        unsafe {
            let Ok(medium) = data.GetData(&format) else {
                return paths;
            };
            let hdrop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            for index in 0..count {
                let length = DragQueryFileW(hdrop, index, None) as usize;
                let mut buffer = vec![0u16; length + 1];
                DragQueryFileW(hdrop, index, Some(&mut buffer));
                paths.push(PathBuf::from(OsString::from_wide(&buffer[..length])));
            }
            windows::Win32::System::Ole::ReleaseStgMedium(&mut { medium });
        }
        paths
    }

    fn carries_files(data: Ref<IDataObject>) -> bool {
        data.as_ref().is_some_and(|data| !files_in(data).is_empty())
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for Target_Impl {
        fn DragEnter(
            &self,
            data: Ref<IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            _at: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows_core::Result<()> {
            let accepted = if carries_files(data) { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
            unsafe { effect.write(accepted) };
            Ok(())
        }

        fn DragOver(
            &self,
            _keys: MODIFIERKEYS_FLAGS,
            _at: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows_core::Result<()> {
            unsafe { effect.write(DROPEFFECT_COPY) };
            Ok(())
        }

        fn DragLeave(&self) -> windows_core::Result<()> {
            Ok(())
        }

        fn Drop(
            &self,
            data: Ref<IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            _at: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows_core::Result<()> {
            let paths = data.as_ref().map(files_in).unwrap_or_default();
            super::take(&self.incoming, paths);
            unsafe { effect.write(DROPEFFECT_COPY) };
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSDragOperation, NSDraggingDestination, NSDraggingInfo,
        NSPasteboardTypeFileURL, NSView,
    };
    use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSURL};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::DropTarget;
    use crate::gui::{Incoming, ParentWindow};

    pub struct Ivars {
        incoming: Incoming,
    }

    define_class!(
        /// A view over the editor's that takes the dropped files. It takes no
        /// clicks: `hitTest:` answers nothing, so the pointer reaches the
        /// editor's view under it as before.
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[name = "MarkdownNotesDropView"]
        #[ivars = Ivars]
        pub struct DropView;

        unsafe impl NSObjectProtocol for DropView {}

        impl DropView {
            #[unsafe(method_id(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
                None
            }
        }

        unsafe impl NSDraggingDestination for DropView {
            #[unsafe(method(draggingEntered:))]
            fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
                if files_in(info).is_empty() {
                    NSDragOperation::None
                } else {
                    NSDragOperation::Copy
                }
            }

            #[unsafe(method(draggingUpdated:))]
            fn dragging_updated(&self, _info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
                NSDragOperation::Copy
            }

            #[unsafe(method(performDragOperation:))]
            fn perform_drag_operation(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
                let paths = files_in(info);
                let any = !paths.is_empty();
                super::take(&self.ivars().incoming, paths);
                any
            }
        }
    );

    /// The files a drag is carrying, as paths.
    fn files_in(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<PathBuf> {
        let pasteboard = info.draggingPasteboard();
        let Some(items) = pasteboard.pasteboardItems() else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        for item in items.iter() {
            // The pasteboard type is an extern static, which is what the
            // unsafe is for: the call itself is safe.
            let Some(url) = item.stringForType(unsafe { NSPasteboardTypeFileURL }) else {
                continue;
            };
            let Some(path) = NSURL::URLWithString(&url).and_then(|url| url.path()) else {
                continue;
            };
            paths.push(PathBuf::from(path.to_string()));
        }
        paths
    }

    pub fn accept(parent: &ParentWindow, incoming: Incoming) -> Option<DropTarget> {
        let RawWindowHandle::AppKit(handle) = parent.window_handle().ok()?.as_raw() else {
            return None;
        };
        let mtm = MainThreadMarker::new()?;
        // The host's view, from the pointer it handed over.
        let parent: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        // The editor is the one view baseview put in the host's.
        let editor = parent.subviews().firstObject()?;

        // baseview's view stops taking drags, so nothing under the overlay
        // answers for them.
        editor.unregisterDraggedTypes();

        let overlay = DropView::alloc(mtm).set_ivars(Ivars { incoming });
        let overlay: Retained<DropView> =
            unsafe { msg_send![super(overlay), initWithFrame: editor.bounds()] };
        overlay.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let types = NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]);
        overlay.registerForDraggedTypes(&types);
        editor.addSubview(&overlay);
        Some(DropTarget { overlay })
    }

    impl Drop for DropTarget {
        fn drop(&mut self) {
            self.overlay.removeFromSuperview();
        }
    }
}
