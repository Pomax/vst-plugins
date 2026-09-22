//! Build tasks.
//!
//! A VST3 plugin is not a bare shared library — it is a *bundle*: a directory
//! named `Something.vst3` with a prescribed layout that differs per platform.
//! Hosts scan for that directory, so a raw `.dll` sitting in `target/debug`
//! will not be found by any DAW. This task builds the library and assembles
//! the bundle around it.
//!
//! ```text
//! Markdown Notes.vst3/
//!   Contents/
//!     x86_64-win/Markdown Notes.vst3        (Windows: the DLL, renamed)
//!     MacOS/Markdown Notes           (macOS: the dylib, no extension)
//!     Info.plist                     (macOS only)
//!     PkgInfo                        (macOS only)
//! ```
//!
//! Usage:
//! ```text
//! cargo run -p xtask -- bundle [--release] [--target <triple>]
//! ```

#[cfg(target_os = "macos")]
mod macos;
mod uitest;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PLUGIN_CRATE: &str = "markdown-notes-plugin";
const BUNDLE_NAME: &str = "Markdown Notes";
const BUNDLE_ID: &str = "com.markdown-notes.markdown-notes";
const VERSION: &str = "0.1.0";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("bundle");

    match command {
        "bundle" => match bundle(&args[1..]) {
            Ok(path) => {
                println!("bundle: {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        "test" => match test(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        "uitest" => match uitest(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        "clean" => {
            remove_build_dir(&workspace_root());
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            println!(
                "tasks:\n  \
                 bundle [--release] [--target <triple>]   assemble the VST3 bundle\n  \
                 test [--release] [--full]                every test but the UI tests\n  \
                 test --only <name>                       one test and nothing else\n  \
                 test --ui <name>                         one UI test, or one suite\n  \
                 uitest [name]                            drive the real window\n  \
                 clean                                    empty the build cache\n  \
                 \n  \
                 Build output goes to .cache/ and stays there, so a run only\n  \
                 recompiles what changed. `clean` empties it.\n  \
                 test runs the unit tests, the scenarios and the headless pixel\n  \
                 tests. --full also runs the UI tests, which drive a real\n  \
                 window and so need a desktop to run on.\n  \
                 --only takes the name of a pixel test file, optionally with\n  \
                 ::<test> after it, or a filter on the unit tests. It builds\n  \
                 nothing else and leaves binaries/ alone."
            );
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown task: {other}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    release: bool,
    target: Option<String>,
}

fn parse(args: &[String]) -> Options {
    let mut opts = Options {
        release: false,
        target: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--release" => opts.release = true,
            "--target" => {
                opts.target = args.get(i + 1).cloned();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Where cargo puts build output.
///
/// `.cargo/config.toml` points this at `.cache`; `CARGO_TARGET_DIR` overrides
/// that, and CI may set it.
fn build_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => root.join(BUILD_DIR),
    }
}

const BUILD_DIR: &str = ".cache";

fn workspace_root() -> PathBuf {
    // xtask lives at <root>/src/xtask.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

fn bundle(args: &[String]) -> Result<PathBuf, String> {
    let opts = parse(args);
    let root = workspace_root();

    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(&root).arg("build").arg("-p").arg(PLUGIN_CRATE);
    if opts.release {
        cmd.arg("--release");
    }
    if let Some(target) = &opts.target {
        cmd.arg("--target").arg(target);
    }
    let status = cmd.status().map_err(|e| format!("running cargo: {e}"))?;
    if !status.success() {
        return Err("building the plugin failed".into());
    }

    let profile_dir = if opts.release { "release" } else { "debug" };
    let mut out_dir = build_dir(&root);
    if let Some(target) = &opts.target {
        out_dir = out_dir.join(target);
    }
    let out_dir = out_dir.join(profile_dir);

    let triple = opts.target.clone().unwrap_or_else(host_triple);
    let lib = out_dir.join(library_name(&triple));
    if !lib.exists() {
        return Err(format!("expected {} to exist", lib.display()));
    }

    let bundle_root = out_dir.join("bundle").join(format!("{BUNDLE_NAME}.vst3"));
    if bundle_root.exists() {
        fs::remove_dir_all(&bundle_root).map_err(|e| format!("clearing old bundle: {e}"))?;
    }
    let contents = bundle_root.join("Contents");

    if is_macos(&triple) {
        let macos = contents.join("MacOS");
        fs::create_dir_all(&macos).map_err(|e| format!("creating {}: {e}", macos.display()))?;
        copy(&lib, &macos.join(BUNDLE_NAME))?;
        write(&contents.join("Info.plist"), &info_plist())?;
        // 'BNDL????' is what the VST3 SDK writes for plugin bundles.
        write(&contents.join("PkgInfo"), "BNDL????")?;
    } else {
        let arch_dir = contents.join(platform_dir(&triple));
        fs::create_dir_all(&arch_dir)
            .map_err(|e| format!("creating {}: {e}", arch_dir.display()))?;
        // On Windows and Linux the binary inside the bundle keeps the .vst3
        // extension rather than .dll/.so.
        copy(&lib, &arch_dir.join(format!("{BUNDLE_NAME}.vst3")))?;
    }

    match write_module_info(&bundle_root, &contents) {
        Ok(path) => println!("info:   {}", path.display()),
        Err(e) => println!("note:   could not write moduleinfo.json: {e}"),
    }

    // The whole bundle, not just the binary inside it.
    if is_macos(&triple) && cfg!(target_os = "macos") {
        sign(&bundle_root)?;
    }

    // So the build result can be picked up without digging through the cache.
    let built = write_binary(&root, &lib, &bundle_root, &triple)?;
    if is_macos(&triple) && cfg!(target_os = "macos") {
        sign(&built)?;
    }
    println!("binary: {}", built.display());

    // Load what is about to ship, the way a host will.
    if runnable_here(&triple) {
        verify_bundle(&built)?;
    } else {
        println!("note:   built for {triple}, which cannot be loaded here");
    }

    Ok(bundle_root)
}

/// Sign a bundle so its signature covers the whole thing.
///
/// The linker ad-hoc signs the binary it produces, which leaves the bundle
/// around it unsigned: `codesign` reports `Sealed Resources=none` and
/// `Info.plist=not bound`. A host running under the hardened runtime with
/// library validation refuses to load a bundle in that state, and the plugin
/// never appears. Signing the assembled directory seals `Contents` and binds
/// the `Info.plist` to the signature.
fn sign(bundle: &Path) -> Result<(), String> {
    let status = Command::new("codesign")
        .args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(bundle)
        .status()
        .map_err(|e| format!("running codesign: {e}"))?;
    if !status.success() {
        return Err(format!("signing {} failed", bundle.display()));
    }
    println!("signed: {}", bundle.display());
    Ok(())
}

/// Whether a bundle built for `triple` can be loaded by this process.
fn runnable_here(triple: &str) -> bool {
    let os = if is_macos(triple) {
        cfg!(target_os = "macos")
    } else if triple.contains("windows") {
        cfg!(target_os = "windows")
    } else if triple.contains("linux") {
        cfg!(target_os = "linux")
    } else {
        false
    };
    let arch = if triple.starts_with("aarch64") {
        cfg!(target_arch = "aarch64")
    } else if triple.starts_with("x86_64") {
        cfg!(target_arch = "x86_64")
    } else {
        false
    };
    os && arch
}

/// Load the finished bundle and ask it for its classes.
///
/// A plug-in's entry points are looked up by name at load time, so anything
/// that removes or renames them — stripping, the wrong crate type, a missing
/// `#[no_mangle]` — compiles and links cleanly and then fails in the host. The
/// only way to know is to load it.
fn verify_bundle(bundle: &Path) -> Result<(), String> {
    let module = vst3_loader::Module::load(bundle)
        .map_err(|e| format!("{} does not load: {e}", bundle.display()))?;

    let count = module.class_count();
    if count == 0 {
        return Err(format!("{}: the factory reports no classes", bundle.display()));
    }
    for index in 0..count {
        let Some((name, category)) = module.class_info(index) else {
            return Err(format!("{}: class {index} has no readable info", bundle.display()));
        };
        println!("loads:  {name} ({category})");
    }
    Ok(())
}

/// Drive the real plugin window with real clicks and keystrokes.
///
/// `uitest [name]` runs one test; with no name it runs all of them.
fn uitest(args: &[String]) -> Result<(), String> {
    let root = workspace_root();
    let only = args.iter().find(|a| !a.starts_with("--")).map(String::as_str);

    // Both halves have to exist: this plugin, and the host that opens it.
    let bundle = bundle(&["--release".to_string()])?;
    let plugin = binaries_dir(&root).join(format!("{BUNDLE_NAME}.vst3"));
    let plugin = if plugin.exists() { plugin } else { bundle };

    let host = mini_host(&root)?;
    println!("host:   {}", host.display());
    println!("plugin: {}\n", plugin.display());

    uitest::run(&root, only, &host, &plugin)
}

/// The mini host's executable, built if it is not there yet.
fn mini_host(root: &Path) -> Result<PathBuf, String> {
    let project = root
        .parent()
        .ok_or("no directory above this project")?
        .join("tools")
        .join("mini-host");
    let name = if cfg!(windows) { "mini-host.exe" } else { "mini-host" };

    for candidate in [
        root.parent().map(|p| p.join("binaries").join(name)),
        Some(project.join(".cache").join("release").join(name)),
    ]
    .into_iter()
    .flatten()
    {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let status = Command::new("cargo")
        .args(["build", "--release", "--quiet"])
        .current_dir(&project)
        .status()
        .map_err(|e| format!("building the mini host: {e}"))?;
    if !status.success() {
        return Err("building the mini host failed".into());
    }
    let built = project.join(".cache").join("release").join(name);
    built
        .exists()
        .then_some(built)
        .ok_or_else(|| "the mini host did not build".to_string())
}

/// Where finished builds go: one directory shared by every project here, so
/// the results sit together rather than one level down inside each.
fn binaries_dir(root: &Path) -> PathBuf {
    match root.parent() {
        Some(parent) => parent.join("binaries"),
        None => root.join("binaries"),
    }
}

/// Delete a file or directory, whichever it is.
fn remove_path(path: &Path) -> Result<(), String> {
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else if path.exists() {
        fs::remove_file(path)
    } else {
        return Ok(());
    };
    result.map_err(|e| {
        format!(
            "removing {}: {e}\n  (if this says the file is in use, something still \
             has the plugin loaded)",
            path.display()
        )
    })
}

/// Take this project's result back out of the shared `binaries/`.
fn remove_binary(root: &Path) {
    let target = binaries_dir(root).join(format!("{BUNDLE_NAME}.vst3"));
    if !target.exists() {
        return;
    }
    match remove_path(&target) {
        Ok(()) => println!("removed: {}", target.display()),
        Err(e) => println!("note:   {e}"),
    }
}

/// Empty the build cache.
///
/// The one thing that can survive is the build tool itself: on Windows a
/// running executable cannot be deleted, and cargo runs xtask out of the same
/// directory. Whatever is left is reported.
fn remove_build_dir(root: &Path) {
    let target = build_dir(root);
    if !target.exists() {
        return;
    }
    let name = BUILD_DIR;
    let before = directory_size(&target);
    let running = std::env::current_exe().ok();

    if fs::remove_dir_all(&target).is_ok() {
        println!("removed: {name}/ ({})", human_size(before));
        return;
    }

    // Only the running executable is in the way. Clear everything else now and
    // hand the remainder to a process that outlives this one.
    purge(&target, running.as_deref());
    if schedule_removal(&target) {
        println!("removed: {name}/ ({})", human_size(before));
    } else {
        println!(
            "removed: {} from {name}/, {} left (the build tool running this)",
            human_size(before.saturating_sub(directory_size(&target))),
            human_size(directory_size(&target))
        );
    }
}

/// Delete `dir` from a process that outlives this one.
///
/// Windows refuses to unlink a running executable, and `cargo dist` runs this
/// tool out of `target/`, so the last of it has to go after this process ends.
fn schedule_removal(dir: &Path) -> bool {
    // Forward slashes so the path works in a POSIX shell on every platform.
    let path = dir.display().to_string().replace('\\', "/");
    Command::new("sh")
        .arg("-c")
        .arg(format!("sleep 2; rm -rf '{path}'"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Delete everything under `dir` except the path to `keep`, if given.
fn purge(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let needed = keep.is_some_and(|k| k.starts_with(&path));
        if needed {
            if path.is_dir() {
                purge(&path, keep);
            }
            continue;
        }
        let _ = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
}

fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", bytes / MB)
    }
}

fn directory_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => directory_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Write `Contents/Resources/moduleinfo.json`, part of the bundle layout since
/// VST 3.7.5.
///
/// It declares the plugin's classes and their subcategories so a host can see
/// what kind of plugin this is — effect or instrument — without loading the
/// binary at all. The contents are read back out of the binary that was just
/// built, rather than written from constants here, so the description cannot
/// disagree with the thing it describes.
fn write_module_info(bundle_root: &Path, contents: &Path) -> Result<PathBuf, String> {
    let module = vst3_loader::Module::load(bundle_root).map_err(|e| e.to_string())?;
    let factory = module.factory_info();

    let mut classes = Vec::new();
    for index in 0..module.class_count() {
        let Some(info) = module.class_info2(index) else {
            continue;
        };
        let subs: Vec<String> = info
            .sub_categories
            .split('|')
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{s}\""))
            .collect();
        classes.push(format!(
            "    {{
      \"CID\": \"{cid}\",
      \"Category\": \"{category}\",
      \"Name\": \"{name}\",
      \"Vendor\": \"{vendor}\",
      \"Version\": \"{version}\",
      \"SDKVersion\": \"{sdk}\",
      \"Sub Categories\": [{subs}],
      \"Class Flags\": {flags},
      \"Cardinality\": {cardinality}
    }}",
            cid = info.cid_string(),
            category = info.category,
            name = info.name,
            vendor = info.vendor,
            version = info.version,
            sdk = info.sdk_version,
            subs = subs.join(", "),
            flags = info.class_flags,
            cardinality = info.cardinality,
        ));
    }

    // `kUnicode` is bit 4 of the factory flags.
    let unicode = factory.flags & (1 << 4) != 0;
    let json = format!(
        "{{
  \"Name\": \"{name}\",
  \"Version\": \"{version}\",
  \"Factory Info\": {{
    \"Vendor\": \"{vendor}\",
    \"URL\": \"{url}\",
    \"E-Mail\": \"{email}\",
    \"Flags\": {{
      \"Unicode\": {unicode},
      \"Classes Discardable\": false,
      \"Component Non Discardable\": false
    }}
  }},
  \"Classes\": [
{classes}
  ],
  \"Compatibility\": []
}}
",
        name = BUNDLE_NAME,
        version = VERSION,
        vendor = factory.vendor,
        url = factory.url,
        email = factory.email,
        classes = classes.join(",\n"),
    );

    let resources = contents.join("Resources");
    fs::create_dir_all(&resources).map_err(|e| format!("creating {}: {e}", resources.display()))?;
    let path = resources.join("moduleinfo.json");
    write(&path, &json)?;
    Ok(path)
}


/// Put the build result in `<root>/binaries`.
///
/// On Windows and Linux that is the plugin binary; on macOS it is the bundle,
/// which is the only loadable form there.
fn write_binary(
    root: &Path,
    lib: &Path,
    bundle_root: &Path,
    triple: &str,
) -> Result<PathBuf, String> {
    let binaries = binaries_dir(root);
    let target = binaries.join(format!("{BUNDLE_NAME}.vst3"));
    // Shared with the other projects alongside this one, so only this
    // project's own result is cleared out.
    remove_path(&target)?;
    fs::create_dir_all(&binaries)
        .map_err(|e| format!("creating {}: {e}", binaries.display()))?;

    if is_macos(triple) {
        copy_tree(bundle_root, &target)?;
    } else {
        copy(lib, &target)?;
    }
    Ok(target)
}

/// Recursively copy a directory.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("creating {}: {e}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|e| format!("reading {}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            copy(&source, &destination)?;
        }
    }
    Ok(())
}

fn is_macos(triple: &str) -> bool {
    triple.contains("apple") || triple.contains("darwin")
}

/// Build the plugin, then run the unit tests and the scenario suite.
///
/// The scenario runner loads the plugin binary at runtime rather than linking
/// it, so cargo does not know to rebuild it first. Running the two in the right
/// order is the whole point of this task.
fn test(args: &[String]) -> Result<(), String> {
    let opts = parse(args);
    let root = workspace_root();

    let run = |what: &str, extra: &[&str]| -> Result<(), String> {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.current_dir(&root);
        cmd.args(extra);
        if opts.release {
            cmd.arg("--release");
        }
        let status = cmd.status().map_err(|e| format!("running cargo: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{what} failed"))
        }
    };

    // One real-window test, or one suite of them, and nothing else.
    if let Some(at) = args.iter().position(|a| a == "--ui") {
        let name = args
            .get(at + 1)
            .ok_or("--ui needs the name of a UI test or of a suite")?;
        return uitest(std::slice::from_ref(name));
    }

    if let Some(at) = args.iter().position(|a| a == "--only") {
        let name = args
            .get(at + 1)
            .ok_or("--only needs the name of a test")?;
        return test_only(&root, name, opts.release);
    }

    // A test run compiles the plugin but is not a build of it, so anything in
    // binaries/ is now describing older code. Remove it rather than leave
    // something stale that looks current.
    remove_binary(&root);

    run("building the plugin", &["build", "-p", "markdown-notes-plugin"])?;
    run("unit tests", &["test", "--workspace"])?;
    run("scenarios", &["run", "-q", "-p", "markdown-notes-testrunner"])?;

    let full = args.iter().any(|a| a == "--full");

    // The pixel tests are behind a feature because they need the wgpu stack,
    // which the plugin itself does not.
    let mut rendering = vec!["test", "-p", PLUGIN_CRATE, "--features", "snapshots"];
    for name in RENDERING_TESTS {
        rendering.extend(["--test", name]);
    }
    run("rendering tests", &rendering)?;

    // The UI tests drive a real window with real clicks and keystrokes, so
    // they need a desktop to do it on. GitHub's hosted Windows runners have
    // none, which is why they are not part of an ordinary run.
    if full {
        println!("\nUI tests");
        uitest(&[])?;
    }

    Ok(())
}

/// The headless pixel tests: one file each under the plugin's `tests/`, all of
/// them behind the `snapshots` feature.
const RENDERING_TESTS: &[&str] = &[
    "theme_rendering",
    "caret_in_view",
    "caret_rendering",
    "clicking_the_document",
    "block_spacing",
    "mermaid_blocks",
    "mermaid_renderers",
    "opening_focus",
    "section_dragging",
    "selection_rendering",
    "source_view",
    "text_area",
    "title_field",
    "view_mode_button",
];

/// Run one test and nothing else: no build of the plugin, no clearing of
/// `binaries/`, no scenarios.
///
/// `name` is one of [`RENDERING_TESTS`], optionally followed by `::` and the
/// name of a test inside it, or else a filter on the workspace's unit tests,
/// such as `block::tests` or the name of a single test.
fn test_only(root: &Path, name: &str, release: bool) -> Result<(), String> {
    let (file, inside) = match name.split_once("::") {
        Some((file, inside)) if RENDERING_TESTS.contains(&file) => (Some(file), Some(inside)),
        _ if RENDERING_TESTS.contains(&name) => (Some(name), None),
        _ => (None, Some(name)),
    };

    let mut cmd = Command::new(env!("CARGO"));
    cmd.current_dir(root).arg("test");
    match file {
        Some(file) => {
            cmd.args(["-p", PLUGIN_CRATE, "--features", "snapshots", "--test", file]);
        }
        None => {
            cmd.args(["--workspace", "--lib", "--bins"]);
        }
    }
    if release {
        cmd.arg("--release");
    }
    if let Some(inside) = inside {
        cmd.args(["--", inside]);
    }

    let status = cmd.status().map_err(|e| format!("running cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed"))
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copying {} -> {}: {e}", from.display(), to.display()))
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// File name cargo gives the cdylib for a target.
fn library_name(triple: &str) -> String {
    if triple.contains("windows") {
        "markdown_notes_plugin.dll".into()
    } else if triple.contains("apple") || triple.contains("darwin") {
        "libmarkdown_notes_plugin.dylib".into()
    } else {
        "libmarkdown_notes_plugin.so".into()
    }
}

/// The architecture directory name the VST3 spec expects inside `Contents`.
fn platform_dir(triple: &str) -> String {
    let arch = if triple.starts_with("x86_64") {
        "x86_64"
    } else if triple.starts_with("aarch64") {
        "arm64"
    } else if triple.starts_with("i686") || triple.starts_with("i586") {
        "x86"
    } else {
        "unknown"
    };
    if triple.contains("windows") {
        format!("{arch}-win")
    } else {
        format!("{arch}-linux")
    }
}

fn host_triple() -> String {
    // Good enough for the host build; explicit --target covers everything else.
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    let os = if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-gnu"
    };
    format!("{arch}-{os}")
}

fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>{BUNDLE_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_ID}</string>
    <key>CFBundleName</key>
    <string>{BUNDLE_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{BUNDLE_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleSignature</key>
    <string>????</string>
    <key>CFBundleVersion</key>
    <string>{VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>{VERSION}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>
"#
    )
}
