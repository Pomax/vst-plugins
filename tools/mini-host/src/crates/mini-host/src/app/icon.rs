//! The window's icon.
//!
//! Drawn in memory rather than shipped as a file, so there is no resource to
//! embed and nothing to keep in step with the build. Handed to eframe with the
//! rest of the window's description, which is where an application says what
//! its window looks like.

/// Icon side, in pixels. The platform scales this for the taskbar and title
/// bar.
const SIDE: usize = 32;

/// The icon, as eframe wants it.
pub fn image() -> std::sync::Arc<egui::IconData> {
    std::sync::Arc::new(egui::IconData {
        rgba: pixels(),
        width: SIDE as u32,
        height: SIDE as u32,
    })
}

/// An empty frame with something seated in it, in RGBA.
///
/// A host is a case that holds whichever plugin is loaded, so that is what it
/// shows: the outer frame is the host, the block inside is the plugin. The
/// design is symmetric top to bottom, which sidesteps the question of which
/// way up the platform wants the rows.
fn pixels() -> Vec<u8> {
    let case = [235u8, 235, 235, 255];
    let edge = [70u8, 70, 70, 255];
    let slot = [200u8, 130, 70, 255];
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Four bytes a pixel, and something drawn in them.
    #[test]
    fn the_icon_is_a_full_square_of_pixels() {
        let bits = pixels();
        assert_eq!(bits.len(), SIDE * SIDE * 4);
        assert!(bits.chunks(4).any(|p| p[3] == 255), "every pixel is clear");
    }
}
