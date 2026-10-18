//! The OCR server, `tools/ocr-server`, which the window drivers hand their
//! pictures to for reading.
//!
//! It is started on the first request and kept for the whole run, so its
//! networks are loaded once. It ends when this process does: its input
//! closes, and it stops reading.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

struct Server {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

fn server() -> &'static Mutex<Result<Server, String>> {
    static SERVER: OnceLock<Mutex<Result<Server, String>>> = OnceLock::new();
    SERVER.get_or_init(|| Mutex::new(start()))
}

/// Start the OCR server with the system's python3 and the packages its setup
/// put in `.cache/site`.
fn start() -> Result<Server, String> {
    let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("no directory above this project")?
        .join("tools")
        .join("ocr-server");
    let site = tools.join(".cache").join("site");
    if !site.exists() {
        return Err(format!(
            "the OCR server is not set up: run {}",
            tools.join("setup.sh").display()
        ));
    }
    let mut child = Command::new("python3")
        .arg(tools.join("ocr_server.py"))
        .env("PYTHONPATH", &site)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting the OCR server: {e}"))?;
    let input = child.stdin.take().ok_or("the OCR server has no input")?;
    let output = child.stdout.take().ok_or("the OCR server has no output")?;
    Ok(Server { _child: child, input, output: BufReader::new(output) })
}

/// Hand the OCR server one request and collect its answer, up to its `end`.
fn ask(request: &str) -> Result<String, String> {
    let mut held = server().lock().map_err(|_| "the OCR server's lock is poisoned")?;
    let server = held.as_mut().map_err(|e| e.clone())?;
    writeln!(server.input, "{request}").map_err(|e| format!("writing to the OCR server: {e}"))?;
    server.input.flush().map_err(|e| format!("writing to the OCR server: {e}"))?;
    let mut answer = String::new();
    loop {
        let mut line = String::new();
        let read = server
            .output
            .read_line(&mut line)
            .map_err(|e| format!("reading from the OCR server: {e}"))?;
        if read == 0 {
            return Err("the OCR server stopped".to_string());
        }
        if line.trim_end() == "end" {
            return Ok(answer);
        }
        answer.push_str(&line);
    }
}

/// What the OCR server says about a picture: with `text`, every place that
/// text is, exact matches first, as `width height` and then `x y width
/// height` a line; with none, every region it read, as `x y width height
/// text` a line after the size. Empty when the text is nowhere in it.
pub fn read(picture: &Path, text: Option<&str>) -> Result<String, String> {
    let said = ask(&format!("{}\t{}", picture.display(), text.unwrap_or("")))?;
    match said.strip_prefix("error ") {
        Some(error) => Err(format!(
            "the OCR server could not read {}: {}",
            picture.display(),
            error.trim()
        )),
        None => Ok(said),
    }
}
