import AppKit
import Foundation

guard CommandLine.arguments.count >= 2 else {
    fputs("Usage: swift gen_icon.swift <output_1024.png>\n", stderr)
    exit(2)
}

let outputPath = CommandLine.arguments[1]
let size = 1024

guard let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: size,
    pixelsHigh: size,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
) else {
    fputs("Failed to create bitmap representation\n", stderr)
    exit(1)
}

rep.size = NSSize(width: size, height: size)

NSGraphicsContext.saveGraphicsState()
guard let ctx = NSGraphicsContext(bitmapImageRep: rep) else {
    fputs("Failed to create graphics context\n", stderr)
    exit(1)
}
NSGraphicsContext.current = ctx
ctx.imageInterpolation = .high
ctx.shouldAntialias = true

func rgba(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1.0) -> NSColor {
    NSColor(srgbRed: r / 255.0, green: g / 255.0, blue: b / 255.0, alpha: a)
}

// 1. Apple HIG squircle base (824x824 inset inside 1024x1024 canvas)
let cardRect = NSRect(x: 100, y: 100, width: 824, height: 824)
let cardPath = NSBezierPath(roundedRect: cardRect, xRadius: 186, yRadius: 186)

NSGraphicsContext.saveGraphicsState()
let cardShadow = NSShadow()
cardShadow.shadowOffset = NSSize(width: 0, height: -18)
cardShadow.shadowBlurRadius = 34
cardShadow.shadowColor = NSColor.black.withAlphaComponent(0.36)
cardShadow.set()
rgba(11, 24, 38).setFill()
cardPath.fill()
NSGraphicsContext.restoreGraphicsState()

NSGraphicsContext.saveGraphicsState()
cardPath.addClip()
if let bgGrad = NSGradient(colors: [
    rgba(10, 22, 36),
    rgba(13, 39, 54),
    rgba(11, 56, 53),
]) {
    bgGrad.draw(in: cardPath, angle: -52)
}

// Ambient glow highlights
if let topGlow = NSGradient(
    starting: rgba(56, 189, 248, 0.24),
    ending: rgba(56, 189, 248, 0.0)
) {
    topGlow.draw(fromCenter: NSPoint(x: 340, y: 770), radius: 0, toCenter: NSPoint(x: 340, y: 770), radius: 440, options: [])
}
if let bottomGlow = NSGradient(
    starting: rgba(16, 185, 129, 0.28),
    ending: rgba(16, 185, 129, 0.0)
) {
    bottomGlow.draw(fromCenter: NSPoint(x: 720, y: 260), radius: 0, toCenter: NSPoint(x: 720, y: 260), radius: 420, options: [])
}
NSGraphicsContext.restoreGraphicsState()

// Subtle inner rim
rgba(255, 255, 255, 0.16).setStroke()
cardPath.lineWidth = 3.0
cardPath.stroke()

// Helper to draw tilted orbital arc (back half or front half)
func drawOrbitHalf(center: NSPoint, rx: CGFloat, ry: CGFloat, angleDeg: CGFloat, front: Bool) {
    NSGraphicsContext.saveGraphicsState()
    let xform = NSAffineTransform()
    xform.translateX(by: center.x, yBy: center.y)
    xform.rotate(byDegrees: angleDeg)
    xform.concat()

    let arc = NSBezierPath()
    let startDeg: CGFloat = front ? 180 : 0
    let endDeg: CGFloat = front ? 360 : 180
    let steps = 90
    for i in 0...steps {
        let t = CGFloat(i) / CGFloat(steps)
        let rad = (startDeg + (endDeg - startDeg) * t) * .pi / 180.0
        let pt = NSPoint(x: cos(rad) * rx, y: sin(rad) * ry)
        if i == 0 {
            arc.move(to: pt)
        } else {
            arc.line(to: pt)
        }
    }
    arc.lineWidth = 28
    arc.lineCapStyle = .butt
    if front {
        rgba(110, 231, 183, 0.94).setStroke()
    } else {
        rgba(56, 189, 248, 0.42).setStroke()
    }
    arc.stroke()
    NSGraphicsContext.restoreGraphicsState()
}

let globeCenter = NSPoint(x: 470, y: 545)
let globeRadius: CGFloat = 235

// 2. Back half of orbital ring
drawOrbitHalf(center: globeCenter, rx: 328, ry: 108, angleDeg: -22, front: false)

// 3. Earth globe body
let globeRect = NSRect(
    x: globeCenter.x - globeRadius,
    y: globeCenter.y - globeRadius,
    width: globeRadius * 2,
    height: globeRadius * 2
)
let globePath = NSBezierPath(ovalIn: globeRect)

NSGraphicsContext.saveGraphicsState()
let globeShadow = NSShadow()
globeShadow.shadowOffset = NSSize(width: 0, height: -12)
globeShadow.shadowBlurRadius = 28
globeShadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
globeShadow.set()
rgba(16, 185, 129).setFill()
globePath.fill()
NSGraphicsContext.restoreGraphicsState()

NSGraphicsContext.saveGraphicsState()
globePath.addClip()
if let globeGrad = NSGradient(colors: [
    rgba(52, 211, 153),
    rgba(16, 185, 129),
    rgba(14, 165, 233),
    rgba(3, 105, 161),
]) {
    globeGrad.draw(in: globePath, angle: -55)
}

// Latitude / longitude grid lines inside globe
rgba(255, 255, 255, 0.34).setStroke()
for rxRatio in [0.42, 0.78] as [CGFloat] {
    let mer = NSBezierPath(ovalIn: NSRect(
        x: globeCenter.x - globeRadius * rxRatio,
        y: globeCenter.y - globeRadius,
        width: globeRadius * 2 * rxRatio,
        height: globeRadius * 2
    ))
    mer.lineWidth = 6.5
    mer.stroke()
}
let axisV = NSBezierPath()
axisV.move(to: NSPoint(x: globeCenter.x, y: globeCenter.y - globeRadius))
axisV.line(to: NSPoint(x: globeCenter.x, y: globeCenter.y + globeRadius))
axisV.lineWidth = 6.5
axisV.stroke()

for yOffset in [-95.0, 0.0, 95.0] as [CGFloat] {
    let lat = NSBezierPath()
    lat.move(to: NSPoint(x: globeCenter.x - globeRadius, y: globeCenter.y + yOffset))
    lat.curve(
        to: NSPoint(x: globeCenter.x + globeRadius, y: globeCenter.y + yOffset),
        controlPoint1: NSPoint(x: globeCenter.x - globeRadius * 0.4, y: globeCenter.y + yOffset - 24),
        controlPoint2: NSPoint(x: globeCenter.x + globeRadius * 0.4, y: globeCenter.y + yOffset - 24)
    )
    lat.lineWidth = 6.5
    lat.stroke()
}

// Soft specular gleam on top-left
if let sheen = NSGradient(
    starting: rgba(255, 255, 255, 0.36),
    ending: rgba(255, 255, 255, 0.0)
) {
    sheen.draw(
        fromCenter: NSPoint(x: globeCenter.x - 88, y: globeCenter.y + 96),
        radius: 0,
        toCenter: NSPoint(x: globeCenter.x - 88, y: globeCenter.y + 96),
        radius: 195,
        options: []
    )
}
NSGraphicsContext.restoreGraphicsState()

// 4. Front half of orbital ring passing over globe
drawOrbitHalf(center: globeCenter, rx: 328, ry: 108, angleDeg: -22, front: true)

// 5. Clipboard card badge at bottom-right
let clipRect = NSRect(x: 565, y: 188, width: 266, height: 314)
let clipPath = NSBezierPath(roundedRect: clipRect, xRadius: 46, yRadius: 46)

NSGraphicsContext.saveGraphicsState()
let badgeShadow = NSShadow()
badgeShadow.shadowOffset = NSSize(width: 0, height: -10)
badgeShadow.shadowBlurRadius = 24
badgeShadow.shadowColor = NSColor.black.withAlphaComponent(0.38)
badgeShadow.set()
rgba(248, 250, 252, 0.98).setFill()
clipPath.fill()
NSGraphicsContext.restoreGraphicsState()

rgba(203, 213, 225, 0.9).setStroke()
clipPath.lineWidth = 3.0
clipPath.stroke()

// Clipboard top clasp
let claspRect = NSRect(x: 643, y: 472, width: 110, height: 46)
let claspPath = NSBezierPath(roundedRect: claspRect, xRadius: 16, yRadius: 16)
rgba(16, 185, 129).setFill()
claspPath.fill()
rgba(255, 255, 255, 0.35).setStroke()
claspPath.lineWidth = 2.5
claspPath.stroke()

// Stylized clipboard content lines
let lineSpecs: [(CGFloat, CGFloat, NSColor)] = [
    (392, 168, rgba(16, 185, 129)),
    (330, 140, rgba(51, 65, 85, 0.78)),
    (268, 160, rgba(51, 65, 85, 0.48)),
]
for (ly, lw, color) in lineSpecs {
    let lrect = NSRect(x: 614, y: ly, width: lw, height: 24)
    let lpath = NSBezierPath(roundedRect: lrect, xRadius: 12, yRadius: 12)
    color.setFill()
    lpath.fill()
}

NSGraphicsContext.restoreGraphicsState()

guard let pngData = rep.representation(using: .png, properties: [:]) else {
    fputs("Failed to encode PNG\n", stderr)
    exit(1)
}

try pngData.write(to: URL(fileURLWithPath: outputPath))
print("Generated icon PNG: \(outputPath) (\(size)x\(size))")
