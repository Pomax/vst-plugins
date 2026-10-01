//! The plugin's editor sits under the host's strip and fills the rest of the
//! window.
//!
//! This opens the host's window, so it only runs when it is asked for:
//!
//! ```text
//! cargo test --test editor_under_the_strip -- --ignored
//! ```
//!
//! It needs the plugin in `binaries/`, which `markdown-notes/build.sh` puts
//! there.

#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// How long the host is given to open its window and report on it.
const LIMIT: Duration = Duration::from_secs(30);

/// The host, stopped however the test ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "opens the host's window"]
fn the_editor_fills_the_window_below_the_strip() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin = manifest.join("../../../../../binaries/Markdown Notes.vst3");
    assert!(
        plugin.exists(),
        "{} is not there: markdown-notes/build.sh builds it",
        plugin.display()
    );

    // A report of this run's own, so that one left by an earlier run cannot
    // answer for this one.
    let scratch = manifest.join("../../../.cache/tests");
    std::fs::create_dir_all(&scratch).expect("could not make .cache/tests");
    let report = scratch.join(format!("editor-under-the-strip-{}.geometry", std::process::id()));

    let host = Command::new(env!("CARGO_BIN_EXE_mini-host"))
        .arg(&plugin)
        .arg("--geometry")
        .arg(&report)
        .spawn()
        .expect("the host did not start");
    let _host = Running(host);

    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < LIMIT {
        last = std::fs::read_to_string(&report).unwrap_or_default();
        if last.contains("editor=0,26,") && last.contains("inset=0,26,0,0") {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if last.is_empty() {
        panic!("the host wrote no report within {} seconds", LIMIT.as_secs());
    }
    panic!(
        "the editor is not under the strip and filling the window. The host's last report:\n{last}"
    );
}
