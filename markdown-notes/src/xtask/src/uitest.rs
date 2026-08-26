//! Scripted tests that drive the real plugin's window with real input.
//!
//! Each test is a file in `src/tools/uitests`. The lines before the blank one
//! are steps — clicks and keystrokes, delivered by the platform's capture
//! tool — and the `expect:` lines after it are what the plugin's state must
//! say once the window closes.
//!
//! These exist because the interesting failures are not in the model: they are
//! keystrokes reaching the wrong widget, or nothing at all. Nothing short of
//! real input into the real window finds those.

use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;

use markdown_notes_core::PluginState;

/// How far down the mini host's own strip pushes the plugin's window.
///
/// It is the host's `chrome::HEIGHT`. The two live in separate projects, and a
/// mismatch shows up as clicks landing one row off.
const HOST_STRIP_HEIGHT: i32 = 26;

/// Where the mini host keeps this plugin's presets: beside the executable it
/// was run from, which is the shared `dist`.
fn presets_dir(root: &Path) -> PathBuf {
    root.parent()
        .map(|parent| parent.join("dist"))
        .unwrap_or_else(|| root.join("dist"))
        .join("presets")
        .join("Markdown Notes")
}

/// The suites of UI tests, and where each keeps its files.
///
/// A test belongs to whichever project it is about: what the plugin's editor
/// does with clicks and keys is the plugin's, and what the host does with the
/// preset buttons is the host's. They share a runner because they share a
/// harness — driving a real window is the same job either way — and because
/// the host has nothing to host without a plugin.
fn suites(root: &Path) -> Vec<(String, PathBuf)> {
    let alongside = |project: &str| {
        root.parent()
            .map(|parent| parent.join(project))
            .unwrap_or_else(|| root.to_path_buf())
            .join("src")
            .join("tools")
            .join("uitests")
    };
    vec![
        ("markdown-notes".to_string(), root.join("src").join("tools").join("uitests")),
        ("mini-host".to_string(), alongside("mini-host")),
    ]
}

/// The window driver, which belongs to neither project: both suites use it.
#[cfg(target_os = "windows")]
fn tools_dir(root: &Path) -> PathBuf {
    root.parent()
        .map(|parent| parent.join("tools"))
        .unwrap_or_else(|| root.join("tools"))
}

/// Whether a suite holds a test of this name.
fn has_test(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.txt")).exists()
}

/// Write down where the run is, so an interrupted one can say what ran.
///
/// One line per event in `.cache/uitests/progress.txt`: `running:` when a
/// test starts, `ok:` or `fail:` when it ends. A run that was cut off leaves
/// its last line as `running:`, which names the test it died in.
fn note(root: &Path, line: &str) {
    let dir = root.join(".cache").join("uitests");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("progress.txt");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(line);
    text.push('\n');
    let _ = std::fs::write(&path, text);
}

/// The suite's tests, in the order its `order.txt` says they run.
///
/// The manifest is the only thing that decides the order: file names do not.
/// Every test file must be listed and every listed test must exist, so a test
/// can neither run in a surprise position nor sit unrun in the directory.
fn ordered_tests(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let manifest = dir.join("order.txt");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("every suite needs an order.txt: {}: {e}", manifest.display()))?;

    let mut files = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let file = dir.join(format!("{name}.txt"));
        if !file.exists() {
            return Err(format!(
                "{} lists {name}, and there is no {name}.txt beside it",
                manifest.display()
            ));
        }
        files.push(file);
    }

    let listed: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_test = path.extension().is_some_and(|e| e == "txt")
                && path.file_name().is_some_and(|n| n != "order.txt");
            if is_test && !listed.contains(&path.as_path()) {
                return Err(format!(
                    "{} is not listed in {}, so it would never run",
                    path.display(),
                    manifest.display()
                ));
            }
        }
    }
    Ok(files)
}

struct Test {
    name: String,
    steps: Vec<String>,
    expects: Vec<String>,
    /// Files to take away once the expectations have been checked.
    ///
    /// A test that leaves a preset behind decides what the next one finds in
    /// the list, and the next one is entitled to an empty one. It cannot be a
    /// `remove:` step, because those run before anything is asserted and half
    /// of what these tests assert is that a file is there.
    cleanups: Vec<String>,
    /// Marked `baseline:` in the file: this test proves something every other
    /// test assumes, typing or clicking. When it fails, the run stops, because
    /// nothing after it can mean anything.
    baseline: bool,
}

fn read_test(path: &Path) -> Result<Test, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut steps = Vec::new();
    let mut expects = Vec::new();
    let mut cleanups = Vec::new();
    let mut baseline = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("cleanup:") {
            cleanups.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("expect:") {
            expects.push(rest.trim().to_string());
        } else if line.trim() == "baseline:" {
            baseline = true;
        } else {
            steps.push(line.to_string());
        }
    }
    if expects.is_empty() {
        return Err(format!("{name}: a test with nothing to assert is not a test"));
    }
    Ok(Test { name, steps, expects, cleanups, baseline })
}

/// A path as a test file writes it, as this platform spells it.
///
/// The tests separate directories with a backslash. Anywhere else that names
/// one file with a backslash in it, so the separators are swapped. Only paths:
/// the text expectations write `\n` for a newline, and that is not a path.
fn platform_path(path: &str) -> String {
    if cfg!(target_os = "windows") {
        path.to_string()
    } else {
        path.replace('\\', "/")
    }
}

/// Check one `expect:` line against the state the plugin wrote.
fn check(expect: &str, state: &PluginState) -> Result<(), String> {
    let unescape = |s: &str| s.replace("\\n", "\n");

    if let Some(want) = expect.strip_prefix("title is") {
        let want = want.trim();
        return (state.title == want)
            .then_some(())
            .ok_or_else(|| format!("title is {:?}, expected {want:?}", state.title));
    }
    // The sections are not part of the state: they are cut from the document by
    // its headings, the same way the plugin cuts them when it opens one.
    let sections = markdown_notes_core::split_document(&state.notes);

    if let Some(want) = expect.strip_prefix("sections is") {
        let want: usize = want
            .trim()
            .parse()
            .map_err(|_| format!("not a number: {expect}"))?;
        return (sections.len() == want)
            .then_some(())
            .ok_or_else(|| format!("{} sections, expected {want}", sections.len()));
    }
    if let Some(rest) = expect.strip_prefix("section ") {
        let (index, want) = rest
            .split_once(" is")
            .ok_or_else(|| format!("cannot read: {expect}"))?;
        let index: usize = index
            .trim()
            .parse()
            .map_err(|_| format!("not a section number: {expect}"))?;
        let want = unescape(want.trim());
        let got = sections
            .get(index)
            .cloned()
            .ok_or_else(|| format!("there is no section {index}"))?;
        return (got == want)
            .then_some(())
            .ok_or_else(|| format!("section {index} is {got:?}, expected {want:?}"));
    }
    if let Some(path) = expect.strip_prefix("file exists") {
        let path = platform_path(path.trim());
        return Path::new(&path)
            .exists()
            .then_some(())
            .ok_or_else(|| format!("{path} was not written"));
    }
    if let Some(path) = expect.strip_prefix("no file at") {
        // For a step that is supposed to write nothing: cancelling out of a
        // dialog that has a filename typed into it, for one.
        let path = platform_path(path.trim());
        return (!Path::new(&path).exists())
            .then_some(())
            .ok_or_else(|| format!("{path} was written, and should not have been"));
    }
    if let Some(rest) = expect.strip_prefix("file at") {
        // `file at PATH has KEY at most N`: a number out of a file of
        // `key=value` lines, bounded above. What the window driver measured
        // about the run lands in one of those.
        if let Some((path, rest)) = rest.split_once(" has ") {
            let (key, limit) = rest
                .split_once(" at most ")
                .ok_or_else(|| format!("cannot read: {expect}"))?;
            let key = key.trim();
            let limit: u64 = limit
                .trim()
                .parse()
                .map_err(|_| format!("not a number: {expect}"))?;
            let path = platform_path(path.trim());
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("{path} was not written: {e}"))?;
            let got = text
                .lines()
                .filter_map(|line| line.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .map(|(_, v)| v.trim().to_string())
                .ok_or_else(|| format!("{path} says nothing about {key}"))?;
            let got: u64 = got
                .parse()
                .map_err(|_| format!("{key} in {path} is not a number: {got}"))?;
            return (got <= limit)
                .then_some(())
                .ok_or_else(|| format!("{key} is {got}, expected at most {limit}"));
        }

        // The file on disk, not the plugin's idea of it: a save that reports
        // success and writes nothing is still a failure.
        let (path, want) = rest
            .split_once(" contains")
            .ok_or_else(|| format!("cannot read: {expect}"))?;
        let path = platform_path(path.trim());
        let want = unescape(want.trim());
        let got = std::fs::read_to_string(&path)
            .map_err(|e| format!("{path} was not written: {e}"))?;
        return got
            .contains(&want)
            .then_some(())
            .ok_or_else(|| format!("{path} holds {got:?}, expected it to contain {want:?}"));
    }
    if let Some(want) = expect.strip_prefix("notes contain") {
        let want = unescape(want.trim());
        return state
            .notes
            .contains(&want)
            .then_some(())
            .ok_or_else(|| format!("notes do not contain {want:?}: {:?}", state.notes));
    }
    Err(format!("unknown expectation: {expect}"))
}

/// Run one test and report what failed, if anything.
/// Turn a step written in the plugin's coordinates into one in the window's.
///
/// `plugin:X,Y` is a click, and `plugin:VERB:X,Y` is whatever else the driver
/// can do at a point: `plugin:hold:60,87` presses there. Anything after a `|`
/// is carried over untouched, which is how `plugin:cursor:60,87|grabbing` says
/// what it expects to find.
fn in_the_plugin(step: &str) -> Option<String> {
    let rest = step.strip_prefix("plugin:")?;
    let (verb, value) = match rest.split_once(':') {
        Some((verb, value)) => (verb, value),
        None => ("click", rest),
    };
    let (coordinates, tail) = match value.split_once('|') {
        Some((coordinates, tail)) => (coordinates, Some(tail)),
        None => (value, None),
    };
    let (x, y) = coordinates.split_once(',')?;
    let y: i32 = y.trim().parse().ok()?;
    let moved = format!("{}:{},{}", verb, x.trim(), y + HOST_STRIP_HEIGHT);
    Some(match tail {
        Some(tail) => format!("{moved}|{tail}"),
        None => moved,
    })
}

fn run_one(root: &Path, test: &Test, host: &Path, plugin: &Path) -> Result<(), String> {
    let work = root.join(".cache").join("uitests");
    std::fs::create_dir_all(&work).map_err(|e| format!("creating {}: {e}", work.display()))?;
    let step_file = work.join(format!("{}.steps", test.name));
    let state_file = work.join(format!("{}.state", test.name));
    let shot = work.join(format!("{}.png", test.name));
    let _ = std::fs::remove_file(&state_file);
    // `%CACHE%` becomes this run's working directory, so a test can name a
    // file to save without depending on where a dialog happens to open.
    //
    // `plugin:X,Y` is a click in the plugin's own window, which sits below the
    // host's strip. Written as a window coordinate it would move every time
    // the host's chrome changed height.
    let steps: Vec<String> = test
        .steps
        .iter()
        .map(|step| in_the_plugin(step).unwrap_or_else(|| step.clone()))
        .collect();
    let steps = steps
        .join("\n")
        .replace("%CACHE%", &work.display().to_string())
        .replace("%PRESETS%", &presets_dir(root).display().to_string());
    std::fs::write(&step_file, steps)
        .map_err(|e| format!("writing {}: {e}", step_file.display()))?;

    drive(root, host, plugin, &step_file, &state_file, &shot)?;

    // A test that ends with `kill:` chose not to have state: a killed program
    // writes none, and that test asserts on files instead.
    let killed = test.steps.iter().any(|step| step.trim() == "kill:");
    let bytes = match std::fs::read(&state_file) {
        Ok(bytes) => bytes,
        Err(_) if killed => Vec::new(),
        Err(_) => {
            return Err(format!(
                "the plugin wrote no state — the window did not close cleanly. \
                 The screenshot of what was on screen is at {}",
                shot.display()
            ))
        }
    };
    let state = PluginState::from_bytes(&bytes);

    let mut failures = Vec::new();
    for expect in &test.expects {
        let expect = expect
            .replace("%CACHE%", &work.display().to_string())
            .replace("%PRESETS%", &presets_dir(root).display().to_string());
        if let Err(e) = check(&expect, &state) {
            failures.push(e);
        }
    }

    // Whatever the test made, gone — including when it failed, so a run that
    // went wrong does not decide what the next one sees.
    for path in &test.cleanups {
        let path = platform_path(
            &path
                .replace("%CACHE%", &work.display().to_string())
                .replace("%PRESETS%", &presets_dir(root).display().to_string()),
        );
        // A trailing `*` takes everything that starts the same way, for a test
        // that made more files than it is worth naming one by one. Only that
        // form, and only in the last part of the path: nothing else is a
        // pattern, so nothing else can widen by accident.
        match path.strip_suffix('*') {
            Some(prefix) => {
                let path = Path::new(prefix);
                let (Some(dir), Some(start)) = (path.parent(), path.file_name()) else {
                    continue;
                };
                let start = start.to_string_lossy().into_owned();
                let Ok(entries) = std::fs::read_dir(dir) else {
                    continue;
                };
                for found in entries.flatten().map(|entry| entry.path()) {
                    let is_ours = found
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(&start));
                    if is_ours {
                        let _ = std::fs::remove_file(found);
                    }
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}\n      screenshot: {}",
            failures.join("\n      "),
            shot.display()
        ))
    }
}

/// Hand the steps to the platform's window driver.
#[cfg(target_os = "windows")]
fn drive(
    root: &Path,
    host: &Path,
    plugin: &Path,
    steps: &Path,
    state: &Path,
    shot: &Path,
) -> Result<(), String> {
    let script = tools_dir(root).join("capture-window.ps1");
    let status = Command::new("powershell")
        .args(["-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Exe")
        .arg(host)
        .arg("-ExeArgs")
        .arg(format!("{}|--state|{}", plugin.display(), state.display()))
        .args(["-Title", "Mini VST Host"])
        .arg("-StepFile")
        .arg(steps)
        .arg("-Out")
        .arg(shot)
        .args(["-SettleMs", "4000"])
        .arg("-CloseCleanly")
        .status()
        .map_err(|e| format!("running {}: {e}", script.display()))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "the window driver failed".to_string())
}

#[cfg(target_os = "macos")]
fn drive(
    root: &Path,
    host: &Path,
    plugin: &Path,
    steps: &Path,
    state: &Path,
    shot: &Path,
) -> Result<(), String> {
    crate::macos::drive(root, host, plugin, steps, state, shot)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn drive(
    _root: &Path,
    _host: &Path,
    _plugin: &Path,
    _steps: &Path,
    _state: &Path,
    _shot: &Path,
) -> Result<(), String> {
    Err("the UI tests need a window driver for this platform".to_string())
}

/// Run every UI test, or the one named.
/// Run the UI tests, or just the named suite or test.
///
/// `only` matches either a suite — `markdown-notes`, `mini-host` — or a single
/// test.
pub fn run(root: &Path, only: Option<&str>, host: &Path, plugin: &Path) -> Result<(), String> {
    let mut ran = 0;
    let mut failed = Vec::new();
    let suites = suites(root);

    // A fresh working directory for this run: a stale file from an old run
    // makes a save dialog stop to ask about replacing it, and a stale picture
    // can satisfy a file-exists expectation the new run never earned.
    let _ = std::fs::remove_dir_all(root.join(".cache").join("uitests"));

    for (suite, dir) in &suites {
        if only.is_some_and(|name| name == suite) {
            // Naming the suite runs all of it.
        } else if only.is_some() && !has_test(dir, only.unwrap_or_default()) {
            continue;
        }

        // A project with no UI tests of its own is not an error.
        if std::fs::read_dir(dir).is_err() {
            continue;
        }
        let files = ordered_tests(dir)?;
        if files.is_empty() {
            continue;
        }
        println!("{suite}");

        for file in files {
            let test = read_test(&file)?;
            if only.is_some_and(|name| name != suite && name != test.name) {
                continue;
            }
            ran += 1;
            note(root, &format!("running: {}", test.name));
            match run_one(root, &test, host, plugin) {
                Ok(()) => {
                    println!("  ok    {}", test.name);
                    note(root, &format!("ok:      {}", test.name));
                }
                Err(e) => {
                    println!("  FAIL  {}\n      {e}", test.name);
                    note(root, &format!("fail:    {}", test.name));
                    failed.push(test.name.clone());
                    if test.baseline {
                        return Err(format!(
                            "{} is a baseline test: it proves something every \
                             later test assumes, so nothing after it ran",
                            test.name
                        ));
                    }
                }
            }
        }
    }

    if ran == 0 {
        let known: Vec<&str> = suites.iter().map(|(name, _)| name.as_str()).collect();
        return Err(match only {
            Some(name) => format!("no UI test or suite called {name}; suites are {known:?}"),
            None => "no UI tests anywhere".to_string(),
        });
    }
    println!();
    if failed.is_empty() {
        println!("{ran} UI tests, all passed");
        Ok(())
    } else {
        Err(format!("{} of {ran} UI tests failed", failed.len()))
    }
}
