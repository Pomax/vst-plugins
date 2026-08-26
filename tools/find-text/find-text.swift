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

let needle = arguments[2].lowercased()
for line in request.results ?? [] {
    guard let text = line.topCandidates(1).first?.string else { continue }
    if text.lowercased().contains(needle) {
        let box = line.boundingBox
        let width = CGFloat(bitmap.width)
        let height = CGFloat(bitmap.height)
        let x = Int(box.minX * width)
        let y = Int((1 - box.maxY) * height)
        print("\(x) \(y) \(Int(box.width * width)) \(Int(box.height * height)) \(bitmap.width) \(bitmap.height)")
        exit(0)
    }
}
exit(1)
