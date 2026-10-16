//! Files dropped on the editor's window.
//!
//! The window is baseview's, and baseview registers a drop target of its own
//! on it, whose drops egui-baseview does not pass on. The plugin puts its own
//! target on the same window in its place: on Windows an `IDropTarget`
//! registered with OLE, on macOS a view registered for dragged files laid
//! over the editor's, on Linux an XDND proxy on the window the window manager
//! manages, which is the host's. Each one reads the dropped paths, keeps the
//! markdown files and the PNG and JPEG files among them, and queues them for
//! the next frame, which opens a markdown file and puts a picture into the
//! document at the caret.

use crate::gui::{Dropped, Incoming, ParentWindow};

/// The target on the editor's window, for as long as the window is open.
/// Dropping it takes the target off the window.
pub struct DropTarget {
    #[cfg(target_os = "windows")]
    hwnd: windows::Win32::Foundation::HWND,
    #[cfg(target_os = "macos")]
    overlay: objc2::rc::Retained<macos::DropView>,
    #[cfg(target_os = "linux")]
    proxy: linux::Proxy,
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
    #[cfg(target_os = "linux")]
    {
        linux::accept(parent, incoming)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (parent, incoming);
        None
    }
}

/// Queue the markdown, PNG and JPEG files among `paths`.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn take(incoming: &Incoming, paths: impl IntoIterator<Item = std::path::PathBuf>) {
    let files: Vec<Dropped> = paths
        .into_iter()
        .filter_map(|path| Dropped::from_file(&path))
        .collect();
    if files.is_empty() {
        return;
    }
    if let Ok(mut queue) = incoming.lock() {
        queue.extend(files);
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

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::Arc;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask,
        PropMode, Window, WindowClass,
    };
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;

    use crate::gui::{Incoming, ParentWindow};

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            XdndAware,
            XdndProxy,
            XdndEnter,
            XdndPosition,
            XdndStatus,
            XdndLeave,
            XdndDrop,
            XdndFinished,
            XdndTypeList,
            XdndSelection,
            XdndActionCopy,
            WM_STATE,
            INCR,
            TEXT_URI_LIST: b"text/uri-list",
            MARKDOWN_NOTES_DROP,
        }
    }

    /// The version of XDND spoken here.
    const VERSION: u32 = 5;

    /// The plugin's stake in the host's window: `XdndProxy` on the window the
    /// window manager manages, pointing at a window of the plugin's own.
    ///
    /// A drag's source looks for `XdndAware` on the managed window under the
    /// pointer and nowhere below it, so a window inside the host's can take
    /// drops only by being named there. The proxy answers for the editor's
    /// area and passes everything else through to whatever took drops on the
    /// host's window before, so the host loses nothing while the editor is
    /// open. Dropping this puts the window back as it was.
    pub struct Proxy {
        conn: Arc<RustConnection>,
        atoms: Atoms,
        toplevel: Window,
        proxy: Window,
        relay: Window,
        /// What `XdndProxy` on the host's window named before, if anything.
        before: Option<Window>,
    }

    impl Drop for Proxy {
        fn drop(&mut self) {
            // Back as it was, unless something else has taken the place since.
            if window_named(&self.conn, self.toplevel, self.atoms.XdndProxy) == Some(self.proxy) {
                let _ = match self.before {
                    Some(before) => self.conn.change_property32(
                        PropMode::REPLACE,
                        self.toplevel,
                        self.atoms.XdndProxy,
                        AtomEnum::WINDOW,
                        &[before],
                    ),
                    None => self.conn.delete_property(self.toplevel, self.atoms.XdndProxy),
                };
            }
            let _ = self.conn.destroy_window(self.relay);
            let _ = self.conn.destroy_window(self.proxy);
            let _ = self.conn.flush();
        }
    }

    pub fn accept(parent: &ParentWindow, incoming: Incoming) -> Option<super::DropTarget> {
        let RawWindowHandle::Xcb(handle) = parent.window_handle().ok()?.as_raw() else {
            return None;
        };
        let socket = handle.window.get();
        let (conn, screen) = x11rb::connect(None).ok()?;
        let conn = Arc::new(conn);
        let root = conn.setup().roots.get(screen)?.root;
        let atoms = Atoms::new(&*conn).ok()?.reply().ok()?;
        // The editor is the one window baseview put in the host's.
        let editor = conn
            .query_tree(socket)
            .ok()?
            .reply()
            .ok()?
            .children
            .into_iter()
            .next()?;
        let toplevel = toplevel_of(&conn, &atoms, socket, root)?;

        // Two windows that draw nothing and take no clicks: the proxy, which
        // the drag's source talks to, and the relay, which the host answers
        // when a drag is passed through to it. Both go with the editor's
        // window when that goes.
        let proxy = conn.generate_id().ok()?;
        let relay = conn.generate_id().ok()?;
        for window in [proxy, relay] {
            conn.create_window(
                0,
                window,
                editor,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .ok()?;
        }
        conn.change_property32(PropMode::REPLACE, proxy, atoms.XdndAware, AtomEnum::ATOM, &[VERSION])
            .ok()?;
        conn.change_property32(PropMode::REPLACE, proxy, atoms.XdndProxy, AtomEnum::WINDOW, &[proxy])
            .ok()?;
        let before = window_named(&conn, toplevel, atoms.XdndProxy);
        conn.change_property32(PropMode::REPLACE, toplevel, atoms.XdndProxy, AtomEnum::WINDOW, &[proxy])
            .ok()?;
        conn.flush().ok()?;

        // Whatever took drops on the host's window before: the host itself,
        // or a proxy something else put there. Nothing, when it took none.
        let behind = before.unwrap_or(toplevel);
        let host = has_property(&conn, behind, atoms.XdndAware).then_some(behind);

        let target = Target {
            conn: Arc::clone(&conn),
            atoms,
            root,
            editor,
            proxy,
            relay,
            host,
            incoming,
            drag: None,
        };
        std::thread::Builder::new()
            .name("markdown-notes-drops".to_string())
            .spawn(move || target.run())
            .ok()?;
        Some(super::DropTarget { proxy: Proxy { conn, atoms, toplevel, proxy, relay, before } })
    }

    /// The window the window manager manages that `window` is inside: the
    /// first one going up with `WM_STATE` on it. The one just under the root
    /// when none has it.
    fn toplevel_of(conn: &RustConnection, atoms: &Atoms, window: Window, root: Window) -> Option<Window> {
        let mut at = window;
        loop {
            if has_property(conn, at, atoms.WM_STATE) {
                return Some(at);
            }
            let tree = conn.query_tree(at).ok()?.reply().ok()?;
            if tree.parent == root || tree.parent == 0 {
                return Some(at);
            }
            at = tree.parent;
        }
    }

    fn has_property(conn: &RustConnection, window: Window, property: Atom) -> bool {
        conn.get_property(false, window, property, AtomEnum::ANY, 0, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.type_ != 0)
    }

    /// The window a property of type WINDOW names, if it is there.
    fn window_named(conn: &RustConnection, window: Window, property: Atom) -> Option<Window> {
        conn.get_property(false, window, property, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()
    }

    /// A drag that has entered the host's window and not left it.
    struct Drag {
        source: Window,
        /// The `XdndEnter` as the source sent it, to pass on to the host.
        enter: [u32; 5],
        /// Whether the source offers the files as a `text/uri-list`.
        files: bool,
        /// Whether the last position was over the editor.
        inside: bool,
        /// Whether the host has been told the drag entered, and not that it
        /// left.
        host_entered: bool,
    }

    /// The proxy's side of the protocol, on a thread of its own.
    struct Target {
        conn: Arc<RustConnection>,
        atoms: Atoms,
        root: Window,
        editor: Window,
        proxy: Window,
        relay: Window,
        host: Option<Window>,
        incoming: Incoming,
        drag: Option<Drag>,
    }

    impl Target {
        /// Answer events until the proxy window is destroyed or the
        /// connection goes.
        fn run(mut self) {
            loop {
                let event = match self.conn.wait_for_event() {
                    Ok(event) => event,
                    Err(_) => return,
                };
                match event {
                    Event::DestroyNotify(gone) if gone.window == self.proxy => return,
                    Event::ClientMessage(message) if message.format == 32 => {
                        let data = message.data.as_data32();
                        if message.window == self.proxy {
                            self.from_source(message.type_, data);
                        } else if message.window == self.relay {
                            self.from_host(message.type_, data);
                        }
                    }
                    Event::SelectionNotify(notice) if notice.requestor == self.proxy => {
                        self.data_arrived(notice.property);
                    }
                    _ => {}
                }
                let _ = self.conn.flush();
            }
        }

        fn send(&self, to: Window, kind: Atom, data: [u32; 5]) {
            let event = ClientMessageEvent::new(32, to, kind, data);
            let _ = self.conn.send_event(false, to, EventMask::NO_EVENT, event);
        }

        /// Pass a message on to the host, as if from the relay.
        fn tell_host(&self, kind: Atom, mut data: [u32; 5]) {
            if let Some(host) = self.host {
                data[0] = self.relay;
                self.send(host, kind, data);
            }
        }

        /// Whether a point on the screen is over the editor.
        fn over_editor(&self, x: i32, y: i32) -> bool {
            let Ok(Ok(place)) = self
                .conn
                .translate_coordinates(self.root, self.editor, x as i16, y as i16)
                .map(|cookie| cookie.reply())
            else {
                return false;
            };
            let Ok(Ok(size)) = self.conn.get_geometry(self.editor).map(|cookie| cookie.reply()) else {
                return false;
            };
            (0..size.width as i32).contains(&(place.dst_x as i32))
                && (0..size.height as i32).contains(&(place.dst_y as i32))
        }

        fn atoms_of(&self, window: Window, property: Atom) -> Vec<Atom> {
            self.conn
                .get_property(false, window, property, AtomEnum::ATOM, 0, 1024)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .and_then(|reply| reply.value32().map(|atoms| atoms.collect()))
                .unwrap_or_default()
        }

        fn from_source(&mut self, kind: Atom, data: [u32; 5]) {
            let a = self.atoms;
            if kind == a.XdndEnter {
                if data[1] >> 24 < 3 {
                    return;
                }
                let source = data[0];
                let types = if data[1] & 1 == 1 {
                    let types = self.atoms_of(source, a.XdndTypeList);
                    // The host reads the list off whoever sent it the enter.
                    let _ = self.conn.change_property32(
                        PropMode::REPLACE,
                        self.relay,
                        a.XdndTypeList,
                        AtomEnum::ATOM,
                        &types,
                    );
                    types
                } else {
                    data[2..].iter().copied().filter(|atom| *atom != 0).collect()
                };
                self.drag = Some(Drag {
                    source,
                    enter: data,
                    files: types.contains(&a.TEXT_URI_LIST),
                    inside: false,
                    host_entered: false,
                });
            } else if kind == a.XdndPosition {
                let Some(drag) = self.drag.as_ref() else {
                    return;
                };
                let (source, enter, files, host_entered) =
                    (drag.source, drag.enter, drag.files, drag.host_entered);
                let x = (data[2] >> 16) as i16 as i32;
                let y = (data[2] & 0xffff) as i16 as i32;
                let inside = self.over_editor(x, y);
                if inside {
                    if host_entered {
                        self.tell_host(a.XdndLeave, [0; 5]);
                    }
                    let action = if files { a.XdndActionCopy } else { 0 };
                    self.send(source, a.XdndStatus, [self.proxy, files as u32, 0, 0, action]);
                } else if self.host.is_some() {
                    if !host_entered {
                        self.tell_host(a.XdndEnter, enter);
                    }
                    self.tell_host(a.XdndPosition, data);
                } else {
                    self.send(source, a.XdndStatus, [self.proxy, 0, 0, 0, 0]);
                }
                if let Some(drag) = self.drag.as_mut() {
                    drag.inside = inside;
                    drag.host_entered = !inside && self.host.is_some();
                }
            } else if kind == a.XdndLeave {
                if let Some(drag) = self.drag.take() {
                    if drag.host_entered {
                        self.tell_host(a.XdndLeave, [0; 5]);
                    }
                }
            } else if kind == a.XdndDrop {
                let Some(drag) = self.drag.take() else {
                    return;
                };
                if drag.inside && drag.files {
                    let time = if data[2] == 0 { x11rb::CURRENT_TIME } else { data[2] };
                    let _ = self.conn.convert_selection(
                        self.proxy,
                        a.XdndSelection,
                        a.TEXT_URI_LIST,
                        a.MARKDOWN_NOTES_DROP,
                        time,
                    );
                    // Kept until the data arrives, which is when the source
                    // is told the drop is finished.
                    self.drag = Some(drag);
                } else if drag.host_entered {
                    self.tell_host(a.XdndDrop, data);
                    // The host's answer comes back through the relay.
                    self.drag = Some(drag);
                } else {
                    self.send(drag.source, a.XdndFinished, [self.proxy, 0, 0, 0, 0]);
                }
            }
        }

        /// What the host answers to a drag passed through to it, sent on to
        /// the source as the proxy's own answer.
        fn from_host(&mut self, kind: Atom, mut data: [u32; 5]) {
            let a = self.atoms;
            let Some(source) = self.drag.as_ref().map(|drag| drag.source) else {
                return;
            };
            if kind == a.XdndStatus || kind == a.XdndFinished {
                data[0] = self.proxy;
                self.send(source, kind, data);
                if kind == a.XdndFinished {
                    self.drag = None;
                }
            }
        }

        /// The dropped files have been handed over, or refused.
        fn data_arrived(&mut self, property: Atom) {
            let a = self.atoms;
            let Some(drag) = self.drag.take() else {
                return;
            };
            let mut taken = false;
            if property != 0 {
                let reply = self
                    .conn
                    .get_property(true, self.proxy, property, AtomEnum::ANY, 0, u32::MAX)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok());
                if let Some(reply) = reply {
                    if reply.type_ != a.INCR && reply.format == 8 {
                        let paths = paths_in(&reply.value);
                        taken = !paths.is_empty();
                        super::take(&self.incoming, paths);
                    }
                }
            }
            let action = if taken { a.XdndActionCopy } else { 0 };
            self.send(drag.source, a.XdndFinished, [self.proxy, taken as u32, action, 0, 0]);
        }
    }

    /// The files a `text/uri-list` names: one `file:` URL a line, the
    /// percent-encoding undone, comment lines and anything else skipped.
    fn paths_in(list: &[u8]) -> Vec<PathBuf> {
        String::from_utf8_lossy(list)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let rest = line.strip_prefix("file://")?;
                // `file:///home/x` and `file://localhost/home/x` are the same
                // file.
                let path = match rest.find('/') {
                    Some(0) => rest,
                    Some(at) => &rest[at..],
                    None => return None,
                };
                Some(PathBuf::from(std::ffi::OsString::from_vec(decoded(path))))
            })
            .collect()
    }

    /// The bytes a percent-encoded string stands for.
    fn decoded(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            let escaped = (bytes[at] == b'%' && at + 2 < bytes.len())
                .then(|| std::str::from_utf8(&bytes[at + 1..at + 3]).ok())
                .flatten()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            match escaped {
                Some(byte) => {
                    out.push(byte);
                    at += 3;
                }
                None => {
                    out.push(bytes[at]);
                    at += 1;
                }
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_uri_list_names_its_files() {
            let list = b"file:///home/me/notes%20one.md\r\n# a comment\r\n\
                         file://localhost/tmp/cat.png\r\nhttp://example.com/x\r\n";
            assert_eq!(
                paths_in(list),
                vec![PathBuf::from("/home/me/notes one.md"), PathBuf::from("/tmp/cat.png")]
            );
            assert!(paths_in(b"").is_empty());
        }

        #[test]
        fn percent_escapes_are_undone_and_a_stray_percent_is_kept() {
            assert_eq!(decoded("a%20b"), b"a b");
            assert_eq!(decoded("100%"), b"100%");
            assert_eq!(decoded("%zz"), b"%zz");
        }
    }
}
