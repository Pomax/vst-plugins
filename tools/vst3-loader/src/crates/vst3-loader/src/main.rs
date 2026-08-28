//! Load a VST3 plugin and report what it declares.
//!
//! Loads any VST3 plugin the way a DAW would and prints what came back: the
//! factory, its classes, buses, parameters, editor size and saved state. It
//! opens no window, so it cannot be used to work in a plugin — that is what
//! the mini host alongside this project is for. Use it to check that a plugin
//! you have just built loads at all.
//!
//! ```text
//! vst3-loader <path-to-plugin> [options]
//!
//!   --class <n>     instantiate class n instead of the first audio module
//!   --list          list the factory's classes and stop
//!   --no-editor     do not create the editor view
//!   --type <text>   send the text to the editor as key presses
//!   --state <file>  write the plugin's state to a file
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use vst3_loader::{resolve_binary, Module};

struct Args {
    path: PathBuf,
    class: Option<i32>,
    list: bool,
    editor: bool,
    type_text: Option<String>,
    state: Option<PathBuf>,
}

fn parse() -> Option<Args> {
    parse_from(std::env::args().skip(1))
}

fn parse_from(args: impl IntoIterator<Item = String>) -> Option<Args> {
    let mut raw = args.into_iter();
    let path = PathBuf::from(raw.next()?);
    let mut args = Args {
        path,
        class: None,
        list: false,
        editor: true,
        type_text: None,
        state: None,
    };
    while let Some(flag) = raw.next() {
        match flag.as_str() {
            "--class" => args.class = raw.next().and_then(|v| v.parse().ok()),
            "--list" => args.list = true,
            "--no-editor" => args.editor = false,
            "--type" => args.type_text = raw.next(),
            "--state" => args.state = raw.next().map(PathBuf::from),
            _ => {}
        }
    }
    Some(args)
}

fn main() -> ExitCode {
    let Some(args) = parse() else {
        eprintln!(
            "usage: vst3-loader <path-to-plugin.vst3> \
             [--list] [--class N] [--no-editor] [--type TEXT] [--state FILE]"
        );
        return ExitCode::FAILURE;
    };

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let binary = resolve_binary(&args.path)?;
    println!("binary   {}", binary.display());

    let module = Module::load(&args.path)?;
    println!("vendor   {}", module.vendor());

    let (f1, f2, f3) = module.factory_versions();
    println!(
        "factory  IPluginFactory={f1} IPluginFactory2={f2} IPluginFactory3={f3}"
    );

    let count = module.class_count();
    println!("classes  {count}");
    for i in 0..count {
        if let Some((name, category)) = module.class_info(i) {
            let marker = if Some(i) == module.first_audio_class() {
                "*"
            } else {
                " "
            };
            println!("  {marker}[{i}] {name}  ({category})");
            match module.class_info2(i) {
                Some(info) => {
                    let cid: String = info.cid.iter().map(|b| format!("{b:02X}")).collect();
                    println!(
                        "        subCategories={:?} flags={:#x} cardinality={} cid={cid}",
                        info.sub_categories, info.class_flags, info.cardinality
                    );
                    if let Some(w) = module.class_info_unicode(i) {
                        println!(
                            "        via factory3: name={:?} category={:?} subCategories={:?}",
                            w.name, w.category, w.sub_categories
                        );
                    } else {
                        println!("        via factory3: MISSING");
                    }
                }
                None => println!("        (no IPluginFactory2 — a host cannot tell what type this is)"),
            }
        }
    }
    if args.list {
        return Ok(());
    }

    let index = args.class.or_else(|| module.first_audio_class()).unwrap_or(0);
    println!("\ninstantiating class {index}");

    // Only the processor can be instantiated directly. Asking for a controller
    // class yields a bare E_NOINTERFACE, which is a confusing thing to be told.
    if let Some((_, category)) = module.class_info(index) {
        if category != "Audio Module Class" {
            println!("  note         this is a {category}, not an Audio Module Class.");
            println!("               Only the processor can be created directly; its");
            println!("               controller is created automatically alongside it.");
        }
    }

    let mut plugin = module.create_plugin_at(index)?;

    let shape = if !plugin.has_controller() {
        "processor only (no controller)"
    } else if plugin.has_separate_controller() {
        "processor + separate controller"
    } else {
        "single-component effect"
    };
    println!("  shape        {shape}");
    println!(
        "  audio buses  {} in, {} out",
        plugin.bus_count(true, true),
        plugin.bus_count(true, false)
    );
    println!(
        "  event buses  {} in, {} out",
        plugin.bus_count(false, true),
        plugin.bus_count(false, false)
    );

    let params = plugin.parameter_count();
    println!("  parameters   {params}");
    for i in 0..params.min(5) {
        if let Some(name) = plugin.parameter_name(i) {
            println!("      [{i}] {name}");
        }
    }
    if params > 5 {
        println!("      … {} more", params - 5);
    }

    if args.editor {
        match plugin.open_editor() {
            Ok(()) => {
                let size = plugin.view_size();
                let resizable = plugin.can_resize().unwrap_or(false);
                match size {
                    Ok((w, h)) => println!("  editor       {w}x{h}, resizable: {resizable}"),
                    Err(e) => println!("  editor       created, but getSize failed: {e}"),
                }
            }
            Err(e) => println!("  editor       none ({e})"),
        }
    }

    if let Some(text) = &args.type_text {
        plugin.type_text(text)?;
        println!("  typed        {} characters", text.chars().count());
    }

    let state = plugin.get_state()?;
    println!("  state        {} bytes", state.len());
    if let Ok(text) = std::str::from_utf8(&state) {
        if text.chars().all(|c| !c.is_control() || c == '\n') && !text.is_empty() {
            let preview: String = text.chars().take(200).collect();
            println!("  state text   {preview}");
        }
    }

    if let Some(path) = &args.state {
        std::fs::write(path, &state)?;
        println!("  wrote        {}", path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Option<Args> {
        parse_from(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_plugin_path_is_required() {
        assert!(parse(&[]).is_none());
    }

    #[test]
    fn a_path_alone_is_enough() {
        let args = parse(&["Markdown Notes.vst3"]).unwrap();
        assert_eq!(args.path, PathBuf::from("Markdown Notes.vst3"));
        assert!(args.editor, "the editor is created unless refused");
        assert!(!args.list);
        assert_eq!(args.class, None);
        assert_eq!(args.type_text, None);
        assert_eq!(args.state, None);
    }

    #[test]
    fn every_flag_is_read() {
        let args = parse(&[
            "Markdown Notes.vst3",
            "--class",
            "1",
            "--list",
            "--no-editor",
            "--type",
            "hello",
            "--state",
            "out.bin",
        ])
        .unwrap();
        assert_eq!(args.class, Some(1));
        assert!(args.list);
        assert!(!args.editor);
        assert_eq!(args.type_text.as_deref(), Some("hello"));
        assert_eq!(args.state, Some(PathBuf::from("out.bin")));
    }

    #[test]
    fn a_class_that_is_not_a_number_is_ignored_rather_than_fatal() {
        let args = parse(&["Markdown Notes.vst3", "--class", "second"]).unwrap();
        assert_eq!(args.class, None);
    }

    #[test]
    fn an_unknown_flag_is_ignored() {
        let args = parse(&["Markdown Notes.vst3", "--verbose"]).unwrap();
        assert_eq!(args.path, PathBuf::from("Markdown Notes.vst3"));
    }

    #[test]
    fn text_to_type_may_contain_spaces_and_dashes() {
        let args = parse(&["Markdown Notes.vst3", "--type", "- a list item"]).unwrap();
        assert_eq!(args.type_text.as_deref(), Some("- a list item"));
    }

    #[test]
    fn a_flag_missing_its_value_leaves_it_unset() {
        let args = parse(&["Markdown Notes.vst3", "--state"]).unwrap();
        assert_eq!(args.state, None);
    }
}
