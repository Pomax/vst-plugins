//! The window's icon.
//!
//! Drawn in memory rather than shipped as a file, so there is no resource to
//! embed and nothing to keep in step with the build.

use std::ffi::c_void;

/// Icon side, in pixels. Windows scales this for the taskbar and title bar.
const SIDE: usize = 32;

/// An empty frame with something seated in it, in BGRA.
///
/// A host is a case that holds whichever plugin is loaded, so that is what it
/// shows: the outer frame is the host, the block inside is the plugin. The
/// design is symmetric top to bottom, which sidesteps the question of which
/// way up the platform wants the rows.
fn pixels() -> Vec<u8> {
    let case = [235u8, 235, 235, 255];
    let edge = [70u8, 70, 70, 255];
    let slot = [70u8, 130, 200, 255];
    let clear = [0u8, 0, 0, 0];

    let mut out = Vec::with_capacity(SIDE * SIDE * 4);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let inside = (3..=28).contains(&x) && (3..=28).contains(&y);
            let border = inside
                && (x <= 4 || x >= 27 || y <= 4 || y >= 27);
            let seated = (11..=20).contains(&x) && (11..=20).contains(&y);

            let colour = if seated {
                slot
            } else if border {
                edge
            } else if inside {
                case
            } else {
                clear
            };
            out.extend_from_slice(&colour);
        }
    }
    out
}

#[cfg(target_os = "windows")]
pub fn set(handle: *mut c_void) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIcon, SendMessageW, ICON_BIG, ICON_SMALL, WM_SETICON,
    };

    let bits = pixels();
    // Every pixel's transparency comes from its alpha, so the mask is all zero.
    let mask = vec![0u8; SIDE * SIDE / 8];

    let icon = unsafe {
        CreateIcon(
            None,
            SIDE as i32,
            SIDE as i32,
            1,
            32,
            mask.as_ptr(),
            bits.as_ptr(),
        )
    };
    let Ok(icon) = icon else { return };

    let hwnd = HWND(handle);
    for which in [ICON_SMALL, ICON_BIG] {
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(which as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set(_handle: *mut c_void) {
    // macOS takes an application icon from the bundle, which a bare binary
    // built by cargo does not have.
    let _ = pixels();
}
