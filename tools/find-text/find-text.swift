// Where a piece of text is in a picture.
//
// ```
// find-text <image.png> <text>
// ```
//
// Prints `x y width height imagewidth imageheight` for the first recognised
// line containing the text, case-insensitive, in image pixels with the origin
// at the top left. Exits 1 when the text is nowhere in the picture.

import AppKit
import Foundation
import Vision

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    FileHandle.standardError.write("usage: find-text <image.png> <text>\n".data(using: .utf8)!)
    exit(2)
}
guard let image = NSImage(contentsOfFile: arguments[1]),
    let bitmap = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
else {
    FileHandle.standardError.write("could not read \(arguments[1])\n".data(using: .utf8)!)
    exit(2)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
try? VNImageRequestHandler(cgImage: bitmap).perform([request])

// An exact match beats a containing one: asked for "Save" in a window that
// also shows "Save preset" and "Save As…", the Save button is the answer.
let needle = arguments[2].lowercased()
var containing: VNRecognizedTextObservation?
var exact: VNRecognizedTextObservation?
for line in request.results ?? [] {
    guard let text = line.topCandidates(1).first?.string else { continue }
    let lowered = text.lowercased()
    if lowered.trimmingCharacters(in: .whitespaces) == needle {
        exact = line
        break
    }
    if containing == nil && lowered.contains(needle) {
        containing = line
    }
}
if let line = exact ?? containing, let candidate = line.topCandidates(1).first {
    // The box of the matched text itself, not of the whole recognised line:
    // neighbouring buttons can be read as one line, and the centre of that
    // line is the gap between them, not the button.
    var box = line.boundingBox
    if let range = candidate.string.range(of: arguments[2], options: .caseInsensitive),
        let part = try? candidate.boundingBox(for: range)
    {
        box = part.boundingBox
    }
    let width = CGFloat(bitmap.width)
    let height = CGFloat(bitmap.height)
    let x = Int(box.minX * width)
    let y = Int((1 - box.maxY) * height)
    print("\(x) \(y) \(Int(box.width * width)) \(Int(box.height * height)) \(bitmap.width) \(bitmap.height)")
    exit(0)
}
exit(1)
