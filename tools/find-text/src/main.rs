//! Where a piece of text is in a picture.
//!
//! ```text
//! find-text <image.png> <text>
//! ```
//!
//! Prints the picture's `width height`, then one `x y width height` line for
//! every place the text was read, case-insensitive, in image pixels with the
//! origin at the top left. Exits 1 when the text is nowhere in the picture.
//!
//! With no text to look for, prints the picture's size and then every line it
//! could read, as `x y width height text`.

use std::process::ExitCode;
use std::time::Duration;

use windows::core::{Interface, RuntimeType, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBufferAccessMode, BitmapDecoder, BitmapInterpolationMode,
    BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode, SoftwareBitmap,
};
use windows::Win32::System::WinRT::IMemoryBufferByteAccess;
use windows::Media::Ocr::{OcrEngine, OcrLine};
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows_future::{AsyncStatus, IAsyncOperation};

/// Wait for an asynchronous call and hand back what it produced.
///
/// The runtime's operations complete on a thread of their own, so this is a
/// wait rather than a poll of anything this thread is responsible for.
fn wait<T: RuntimeType>(op: IAsyncOperation<T>) -> windows::core::Result<T> {
    while op.Status()? == AsyncStatus::Started {
        std::thread::sleep(Duration::from_millis(1));
    }
    op.GetResults()
}

fn main() -> ExitCode {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (path, needle) = match arguments.as_slice() {
        [path] => (path, None),
        [path, needle] => (path, Some(needle)),
        _ => {
            eprintln!("usage: find-text <image.png> [text]");
            return ExitCode::from(2);
        }
    };

    let lines = match read(path) {
        Ok(lines) => lines,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (width, height) = lines.size;

    // With nothing to look for, say what is readable. A picture the finder
    // reads as blank and one whose words came out differently are the two
    // ways a search fails, and they need telling apart.
    let Some(needle) = needle else {
        println!("{width} {height}");
        for line in &lines.lines {
            let (x, y, w, h) = line.box_of;
            println!("{x} {y} {w} {h} {}", line.text);
        }
        return ExitCode::SUCCESS;
    };

    // Every match, not the first: the same word can be a button and a heading
    // at once, and which one is wanted is a question about where it is, which
    // only the caller can answer. An exact match beats a containing one, so
    // the exact ones are printed first: asked for "Save" in a window that also
    // shows "Save preset" and "Save As...", the Save button leads the answer.
    let wanted = needle.to_lowercase();
    let mut exact = Vec::new();
    let mut containing = Vec::new();
    for line in &lines.lines {
        let lowered = line.text.to_lowercase();
        // The box of the matched words, not of the whole recognised line:
        // neighbouring buttons can be read as one line, and the centre of that
        // line is the gap between them, not the button.
        let box_of = words_matching(line, &wanted).unwrap_or(line.box_of);
        if lowered.trim() == wanted {
            exact.push(box_of);
        } else if lowered.contains(&wanted) {
            containing.push(box_of);
        }
    }

    exact.append(&mut containing);
    if exact.is_empty() {
        return ExitCode::from(1);
    }
    println!("{width} {height}");
    for (x, y, w, h) in exact {
        println!("{x} {y} {w} {h}");
    }
    ExitCode::SUCCESS
}

/// A recognised line: its text, its box, and the box of each word in it.
struct Line {
    text: String,
    box_of: (i32, i32, i32, i32),
    words: Vec<(String, (i32, i32, i32, i32))>,
}

struct Reading {
    lines: Vec<Line>,
    size: (u32, u32),
}

/// The narrowest run of words in `line` whose text contains `wanted`.
///
/// Recognition puts a box round every word, so the run that spells the wanted
/// text can be boxed on its own even when the line holds more than it.
fn words_matching(line: &Line, wanted: &str) -> Option<(i32, i32, i32, i32)> {
    let count = line.words.len();
    let mut best: Option<(usize, usize)> = None;
    for start in 0..count {
        for end in start..count {
            let run = line.words[start..=end]
                .iter()
                .map(|(text, _)| text.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            if !run.contains(wanted) {
                continue;
            }
            let shorter = best.is_none_or(|(a, b)| end - start < b - a);
            if shorter {
                best = Some((start, end));
            }
            break;
        }
    }
    let (start, end) = best?;
    let boxes = line.words[start..=end].iter().map(|(_, b)| *b);
    boxes.reduce(|a, b| {
        let left = a.0.min(b.0);
        let top = a.1.min(b.1);
        let right = (a.0 + a.2).max(b.0 + b.2);
        let bottom = (a.1 + a.3).max(b.1 + b.3);
        (left, top, right - left, bottom - top)
    })
}

fn read(path: &str) -> Result<Reading, String> {
    let full = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("could not read {path}: {e}"))?;
    // `canonicalize` gives a verbatim path, which the storage API will not
    // open.
    let full = full.to_string_lossy().replace(r"\\?\", "");

    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(&full))
        .and_then(wait)
        .map_err(|e| format!("could not open {path}: {e}"))?;
    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .and_then(wait)
        .map_err(|e| format!("could not read {path}: {e}"))?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .and_then(wait)
        .map_err(|e| format!("{path} is not a picture: {e}"))?;
    let size = (
        decoder.PixelWidth().map_err(|e| e.to_string())?,
        decoder.PixelHeight().map_err(|e| e.to_string())?,
    );
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .ok()
        .filter(|engine| engine.RecognizerLanguage().is_ok())
        .or_else(|| OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("en-US")).ok()?).ok())
        .ok_or("Windows has no text recognition language installed")?;

    // Read the picture at several sizes and keep everything found at any of
    // them. Which labels come out depends on how large the lettering is, and
    // not in a way that can be predicted: a button read at one size is missed
    // at the next. Whatever one pass misses another catches.
    let mut lines = Vec::new();
    for scale in enlargements(size) {
        lines.extend(at_scale(path, &decoder, &engine, size, scale)?);
    }
    Ok(Reading { lines, size })
}

/// Read the picture once, enlarged `scale` times over.
fn at_scale(
    path: &str,
    decoder: &BitmapDecoder,
    engine: &OcrEngine,
    size: (u32, u32),
    scale: u32,
) -> Result<Vec<Line>, String> {
    // Cubic rather than the smoothing default: interface text is drawn a
    // stroke wide, and smoothing an enlargement spreads those strokes into the
    // background until a label is not there to be read at all.
    let transform = BitmapTransform::new().map_err(|e| e.to_string())?;
    transform
        .SetScaledWidth(size.0 * scale)
        .and_then(|()| transform.SetScaledHeight(size.1 * scale))
        .and_then(|()| transform.SetInterpolationMode(BitmapInterpolationMode::Cubic))
        .map_err(|e| format!("could not enlarge {path}: {e}"))?;

    // Recognition reads one pixel format. A PNG decodes to whatever it was
    // written in, and anything else is read as a blank picture.
    let bitmap = decoder
        .GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .and_then(wait)
        .map_err(|e| format!("could not decode {path}: {e}"))?;
    deepen(&bitmap)?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(wait)
        .map_err(|e| format!("reading {path} failed: {e}"))?;

    let mut lines = Vec::new();
    for line in result.Lines().map_err(|e| e.to_string())? {
        lines.push(one_line(&line, scale)?);
    }
    Ok(lines)
}

/// Pull the picture's greys apart, in place.
///
/// Interface text is a mid grey on a light grey, which recognition reads as
/// nothing at all: a button's label is simply not in the answer. Pushing dark
/// pixels towards black and light ones towards white is what puts it there.
/// Not all the way to black and white: a selected control is dark text on a
/// strong fill, and flattening that would take the label with it.
fn deepen(bitmap: &SoftwareBitmap) -> Result<(), String> {
    let buffer = bitmap
        .LockBuffer(BitmapBufferAccessMode::ReadWrite)
        .map_err(|e| format!("could not read the picture's pixels: {e}"))?;
    let reference = buffer.CreateReference().map_err(|e| e.to_string())?;
    let access: IMemoryBufferByteAccess = reference.cast().map_err(|e| e.to_string())?;

    let mut data = std::ptr::null_mut();
    let mut length = 0u32;
    let pixels = unsafe {
        access
            .GetBuffer(&mut data, &mut length)
            .map_err(|e| e.to_string())?;
        std::slice::from_raw_parts_mut(data, length as usize)
    };
    for pixel in pixels.chunks_exact_mut(4) {
        let grey =
            (pixel[0] as u32 * 29 + pixel[1] as u32 * 150 + pixel[2] as u32 * 77) / 256;
        let pulled = ((grey as i32 - 128) * 2 + 128).clamp(0, 255) as u8;
        pixel[0] = pulled;
        pixel[1] = pulled;
        pixel[2] = pulled;
    }
    Ok(())
}

/// The sizes to read a picture at, largest first.
///
/// What has to grow is the lettering rather than the picture, and there is no
/// one size that reads everything: two and four times over between them get
/// interface text that either alone misses. A picture too large to enlarge is
/// read as it is.
fn enlargements(size: (u32, u32)) -> Vec<u32> {
    let wide = size.0.max(1);
    let fits: Vec<u32> = [4, 3, 2].into_iter().filter(|s| wide * s <= 6000).collect();
    if fits.is_empty() { vec![1] } else { fits }
}

fn one_line(line: &OcrLine, scale: u32) -> Result<Line, String> {
    let mut words = Vec::new();
    for word in line.Words().map_err(|e| e.to_string())? {
        let text = word.Text().map_err(|e| e.to_string())?.to_string_lossy();
        let rect = word.BoundingRect().map_err(|e| e.to_string())?;
        let back = scale as f32;
        words.push((
            text,
            (
                (rect.X / back) as i32,
                (rect.Y / back) as i32,
                (rect.Width / back) as i32,
                (rect.Height / back) as i32,
            ),
        ));
    }
    let text = words
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let box_of = words
        .iter()
        .map(|(_, b)| *b)
        .reduce(|a, b| {
            let left = a.0.min(b.0);
            let top = a.1.min(b.1);
            let right = (a.0 + a.2).max(b.0 + b.2);
            let bottom = (a.1 + a.3).max(b.1 + b.3);
            (left, top, right - left, bottom - top)
        })
        .unwrap_or((0, 0, 0, 0));
    Ok(Line { text, box_of, words })
}
