// SPDX-License-Identifier: AGPL-3.0-or-later
// Cuts the app icon out of a source picture of the rounded icon tile: a transparent PNG/WebP is used
// as drawn (cropped to its artwork); a tile on a white background is found and clipped.
//
//   swift scripts/cut-icon.swift app/assets/icon/source.png app/assets/icon
//
// Finds the warm (cream) tile against the white/grey background and shadow, measures its corner
// radius along the diagonal, clips it with an anti-aliased rounded rectangle, and writes:
//   icon-macos.png  1024×1024: the tile at 824 px, inset on a transparent canvas, with a soft shadow
//   icon.png        1024×1024: the tile filling the canvas (Windows, Linux, Android, web)
import AppKit
import CoreGraphics

let args = CommandLine.arguments
guard args.count == 3, let src = NSImage(contentsOfFile: args[1]),
      let cg = src.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    FileHandle.standardError.write("usage: swift cut-icon.swift <source image> <output dir>\n".data(using: .utf8)!)
    exit(2)
}
let outDir = URL(fileURLWithPath: args[2])

// RGBA bytes of the source.
let w = cg.width, h = cg.height
var px = [UInt8](repeating: 0, count: w * h * 4)
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let readCtx = CGContext(data: &px, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
readCtx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))

func alpha(_ x: Int, _ y: Int) -> Int { Int(px[((h - 1 - y) * w + x) * 4 + 3]) }

func writePNG(_ img: CGImage, _ name: String) {
    let url = outDir.appendingPathComponent(name)
    try! NSBitmapImageRep(cgImage: img).representation(using: .png, properties: [:])!.write(to: url)
    print("wrote \(url.path)")
}

/// Draws `art` fitted (aspect kept) and centered in `box` on a transparent square canvas.
func place(_ art: CGImage, size: Int, box: CGRect, shadow: Bool) -> CGImage {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    let k = min(box.width / CGFloat(art.width), box.height / CGFloat(art.height))
    let dw = CGFloat(art.width) * k, dh = CGFloat(art.height) * k
    let rect = CGRect(x: box.midX - dw / 2, y: box.midY - dh / 2, width: dw, height: dh)
    if shadow {
        ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: CGColor(srgbRed: 0.25, green: 0.2, blue: 0.12, alpha: 0.3))
    }
    ctx.draw(art, in: rect)
    return ctx.makeImage()!
}

// A source that is already cut out (transparent corners): use the artwork as it is, with its own
// anti-aliased edges, cropped to what it draws.
if alpha(0, 0) < 10 && alpha(w - 1, h - 1) < 10 {
    var minX = w, minY = h, maxX = 0, maxY = 0
    for y in 0..<h { for x in 0..<w where alpha(x, y) > 8 {
        minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
    } }
    let art = cg.cropping(to: CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1))!
    print("transparent source: artwork \(art.width)×\(art.height) at (\(minX), \(minY)), used as is")
    writePNG(place(art, size: 1024, box: CGRect(x: 100, y: 100, width: 824, height: 824), shadow: true), "icon-macos.png")
    writePNG(place(art, size: 1024, box: CGRect(x: 0, y: 0, width: 1024, height: 1024), shadow: false), "icon.png")
    exit(0)
}

/// A tile pixel is warm: red clearly above blue. White background and grey shadow are not.
func isTile(_ x: Int, _ y: Int) -> Bool {
    guard x >= 0, y >= 0, x < w, y < h else { return false }
    let i = ((h - 1 - y) * w + x) * 4   // y from the top
    let r = Int(px[i]), g = Int(px[i + 1]), b = Int(px[i + 2]), a = Int(px[i + 3])
    return a > 200 && r - b > 8 && min(r, g, b) > 190 || a > 200 && r - b > 8 && r < 200 && r - b > 30
}

// Edges along the middle row and column (the artwork never reaches the tile's edges there).
let midY = h / 2, midX = w / 2
guard let left = (0..<w).first(where: { isTile($0, midY) }),
      let right = (0..<w).reversed().first(where: { isTile($0, midY) }),
      let top = (0..<h).first(where: { isTile(midX, $0) }),
      let bottom = (0..<h).reversed().first(where: { isTile(midX, $0) }) else {
    FileHandle.standardError.write("could not find the icon tile\n".data(using: .utf8)!)
    exit(1)
}

// Corner radius: walk the diagonal in from the top-left corner to the first tile pixel.
// For a circular corner of radius r that point sits r·(1 − 1/√2) in from each edge.
let t = (0..<min(right - left, bottom - top)).first(where: { isTile(left + $0, top + $0) }) ?? 0
let radius = CGFloat(t) / (1 - 1 / 2.squareRoot())
// Icon tiles are square; the drop shadow below can extend the measured height, so trust the width.
let side = min(right - left + 1, bottom - top + 1)
let tile = CGRect(x: left, y: top, width: side, height: side)
print("tile \(Int(tile.width))×\(Int(tile.height)) at (\(left), \(top)), corner radius ≈ \(Int(radius)) px")

// The tile, clipped to its rounded rectangle (inset 1.5 px to drop the anti-aliased white rim).
let cropped = cg.cropping(to: tile)!
func render(size: Int, tileRect: CGRect, shadow: Bool) -> CGImage {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    let scale = tileRect.width / tile.width
    let r = radius * scale
    let path = CGPath(roundedRect: tileRect.insetBy(dx: 1.5 * scale, dy: 1.5 * scale), cornerWidth: r, cornerHeight: r, transform: nil)
    if shadow {
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: CGColor(srgbRed: 0.25, green: 0.2, blue: 0.12, alpha: 0.3))
        ctx.addPath(path)
        ctx.setFillColor(CGColor(srgbRed: 1, green: 0.98, blue: 0.94, alpha: 1))
        ctx.fillPath()
        ctx.restoreGState()
    }
    ctx.addPath(path)
    ctx.clip()
    ctx.draw(cropped, in: tileRect)
    return ctx.makeImage()!
}

func write(_ img: CGImage, _ name: String) {
    let rep = NSBitmapImageRep(cgImage: img)
    let url = outDir.appendingPathComponent(name)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
    print("wrote \(url.path)")
}

// Square output even if the tile is a pixel or two off square.
write(render(size: 1024, tileRect: CGRect(x: 100, y: 100, width: 824, height: 824), shadow: true), "icon-macos.png")
write(render(size: 1024, tileRect: CGRect(x: 0, y: 0, width: 1024, height: 1024), shadow: false), "icon.png")
