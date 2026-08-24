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
use std::process::Command;

use notepad_core::PluginState;

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
        .join("Notepad")
}

/// The suites of UI tests, and where each keeps its files.
///
/// A test belongs to whichever project it is about: what the notepad's editor
/// does with clicks and keys is the notepad's, and what the host does with the
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
        ("notepad".to_string(), root.join("src").join("tools").join("uitests")),
        ("mini-host".to_string(), alongside("mini-host")),
    ]
}

/// The window driver, which belongs to neither project: both suites use it.
fn tools_dir(root: &Path) -> PathBuf {
    root.parent()
        .map(|parent| parent.join("tools"))
        .unwrap_or_else(|| root.join("tools"))
}

/// Whether a suite holds a test of this name.
fn has_test(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.txt")).exists()
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
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("cleanup:") {
            cleanups.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("expect:") {
            expects.push(rest.trim().to_string());
        } else {
            steps.push(line.to_string());
        }
    }
    if expects.is_empty() {
        return Err(format!("{name}: a test with nothing to assert is not a test"));
    }
    Ok(Test { name, steps, expects, cleanups })
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
    // Tabs are not part of the state: they are cut from the document by its
    // headings, the same way the plugin cuts them when it opens one.
    let tabs = notepad_core::split_document(&state.notes);

    if let Some(want) = expect.strip_prefix("tabs is") {
        let want: usize = want
            .trim()
            .parse()
            .map_err(|_| format!("not a number: {expect}"))?;
        return (tabs.len() == want)
            .then_some(())
            .ok_or_else(|| format!("{} tabs, expected {want}", tabs.len()));
    }
    if let Some(rest) = expect.strip_prefix("tab ") {
        let (index, want) = rest
            .split_once(" is")
            .ok_or_else(|| format!("cannot read: {expect}"))?;
        let index: usize = index
            .trim()
            .parse()
            .map_err(|_| format!("not a tab number: {expect}"))?;
        let want = unescape(want.trim());
        let got = tabs
            .get(index)
            .cloned()
            .ok_or_else(|| format!("there is no tab {index}"))?;
        return (got == want)
            .then_some(())
            .ok_or_else(|| format!("tab {index} is {got:?}, expected {want:?}"));
    }
    if let Some(path) = expect.strip_prefix("file exists") {
        let path = path.trim();
        return Path::new(path)
            .exists()
            .then_some(())
            .ok_or_else(|| format!("{path} was not written"));
    }
    if let Some(path) = expect.strip_prefix("no file at") {
        // For a step that is supposed to write nothing: cancelling out of a
        // dialog that has a filename typed into it, for one.
        let path = path.trim();
        return (!Path::new(path).exists())
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
            let path = path.trim();
            let text = std::fs::read_to_string(path)
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
        let path = path.trim();
        let want = unescape(want.trim());
        let got = std::fs::read_to_string(path)
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
        .map(|step| match step.strip_prefix("plugin:") {
            Some(at) => match at.split_once(',') {
                Some((x, y)) => {
                    let y: i32 = y.trim().parse().unwrap_or(0);
                    format!("click:{},{}", x.trim(), y + HOST_STRIP_HEIGHT)
                }
                None => step.clone(),
            },
            None => step.clone(),
        })
        .collect();
    let steps = steps
        .join("\n")
        .replace("%CACHE%", &work.display().to_string())
        .replace("%PRESETS%", &presets_dir(root).display().to_string());
    std::fs::write(&step_file, steps)
        .map_err(|e| format!("writing {}: {e}", step_file.display()))?;

    drive(root, host, plugin, &step_file, &state_file, &shot)?;

    let bytes = std::fs::read(&state_file).map_err(|_| {
        format!(
            "the plugin wrote no state — the window did not close cleanly. \
             The screenshot of what was on screen is at {}",
            shot.display()
        )
    })?;
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
        let path = path
            .replace("%CACHE%", &work.display().to_string())
            .replace("%PRESETS%", &presets_dir(root).display().to_string());
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

#[cfg(not(target_os = "windows"))]
fn drive(
    _root: &Path,
    _host: &Path,
    _plugin: &Path,
    _steps: &Path,
    _state: &Path,
    _shot: &Path,
) -> Result<(), String> {
    Err("the UI tests need a window driver for this platform; \
         tools/capture-window.sh does not take steps yet"
        .to_string())
}

/// Run every UI test, or the one named.
/// Run the UI tests, or just the named suite or test.
///
/// `only` matches either a suite — `notepad`, `mini-host` — or a single test.
pub fn run(root: &Path, only: Option<&str>, host: &Path, plugin: &Path) -> Result<(), String> {
    let mut ran = 0;
    let mut failed = Vec::new();
    let suites = suites(root);

    for (suite, dir) in &suites {
        if only.is_some_and(|name| name == suite) {
            // Naming the suite runs all of it.
        } else if only.is_some() && !has_test(dir, only.unwrap_or_default()) {
            continue;
        }

        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "txt"))
                .collect(),
            // A project with no UI tests of its own is not an error.
            Err(_) => continue,
        };
        files.sort();
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
            match run_one(root, &test, host, plugin) {
                Ok(()) => println!("  ok    {}", test.name),
                Err(e) => {
                    println!("  FAIL  {}\n      {e}", test.name);
                    failed.push(test.name);
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
