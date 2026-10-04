// Generates the small HEIC fixtures used by fino-core's HEIC tests, with ImageIO itself.
//
//     swift crates/fino-core/tests/fixtures/make_heic_fixtures.swift crates/fino-core/tests/fixtures
//
// Every file is a 256×192 synthetic photo (no third-party content):
//   rotated.heic    like an iPhone: `irot` and EXIF Orientation both say 6, GPS, Apple
//                   MakerNote ContentIdentifier (Live Photo id)
//   plain.heic      the same plus an XMP property in a namespace ImageIO does not know;
//                   here only `irot` carries the rotation (no EXIF Orientation tag)
//   gainmap.heic    Apple HDR gain map (+ MakerNote 33/48 that make it apply)
//   hlg.heic        10-bit HLG primary image (HDR a JPEG cannot hold)
//   isogainmap.heic ISO 21496-1 gain map (macOS 15+ only; skipped elsewhere)
import CoreGraphics
import Foundation
import ImageIO

let outDir = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
let width = 256, height = 192

/// Gradients plus a deterministic texture, so compression has real work to do.
func photo(space: CGColorSpace) -> CGImage {
    let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                        space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let px = ctx.data!.bindMemory(to: UInt8.self, capacity: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let i = (y * width + x) * 4
            let n = UInt8(((x * 7 + y * 13) ^ (x * y)) % 23)
            px[i] = UInt8(x) &+ n
            px[i + 1] = UInt8(y + 30) &+ n
            px[i + 2] = UInt8((x + y) / 2) &+ n
            px[i + 3] = 255
        }
    }
    return ctx.makeImage()!
}

func write(_ name: String, _ build: (CGImageDestination) -> Void) {
    let url = outDir.appendingPathComponent(name)
    guard let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.heic" as CFString, 1, nil) else {
        fatalError("no HEIC encoder")
    }
    build(dest)
    guard CGImageDestinationFinalize(dest) else { fatalError("could not write \(name)") }
    print("wrote \(name)")
}

let p3 = CGColorSpace(name: CGColorSpace.displayP3)!
let base = photo(space: p3)

// plain.heic: write once with properties, then re-add with metadata to inject foreign XMP
let properties: [CFString: Any] = [
    kCGImagePropertyOrientation: 6,
    kCGImageDestinationLossyCompressionQuality: 0.85,
    kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFMake: "Apple", kCGImagePropertyTIFFModel: "iPhone 15 Pro"],
    kCGImagePropertyExifDictionary: [kCGImagePropertyExifDateTimeOriginal: "2026:10:04 10:00:00",
                                     kCGImagePropertyExifLensModel: "Fixture lens"],
    kCGImagePropertyGPSDictionary: [kCGImagePropertyGPSLatitude: 4.6, kCGImagePropertyGPSLatitudeRef: "N",
                                    kCGImagePropertyGPSLongitude: 74.08, kCGImagePropertyGPSLongitudeRef: "W"],
    kCGImagePropertyMakerAppleDictionary: ["17": "6F1E2D3C-4B5A-4978-8695-A4B3C2D1E0F9"],
]
let tmp = outDir.appendingPathComponent("rotated.heic")
do {
    let dest = CGImageDestinationCreateWithURL(tmp as CFURL, "public.heic" as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, base, properties as CFDictionary)
    CGImageDestinationFinalize(dest)
    print("wrote rotated.heic")
}
let source = CGImageSourceCreateWithURL(tmp as CFURL, nil)!
let metadata = CGImageMetadataCreateMutableCopy(CGImageSourceCopyMetadataAtIndex(source, 0, nil)!)!
let ns = "http://ns.fino.app/fixture/1.0/" as CFString
CGImageMetadataRegisterNamespaceForPrefix(metadata, ns, "finofixture" as CFString, nil)
CGImageMetadataSetValueWithPath(metadata, nil, "finofixture:Label" as CFString, "kept byte for byte" as CFString)
write("plain.heic") { dest in
    // The orientation option writes HEIF's own `irot` box, which ImageIO trusts over EXIF.
    CGImageDestinationAddImageAndMetadata(dest, base, metadata,
                                          [kCGImageDestinationLossyCompressionQuality: 0.85,
                                           kCGImagePropertyOrientation: 6] as CFDictionary)
}

// gainmap.heic: Apple HDR gain map, a quarter-resolution 8-bit luminance plane
write("gainmap.heic") { dest in
    let gw = width / 2, gh = height / 2
    var plane = [UInt8](repeating: 0, count: gw * gh)
    for y in 0..<gh { for x in 0..<gw { plane[y * gw + x] = UInt8((x * 255) / gw) } }
    let gainMeta = CGImageMetadataCreateMutable()
    let gns = "http://ns.apple.com/HDRGainMap/1.0/" as CFString
    CGImageMetadataRegisterNamespaceForPrefix(gainMeta, gns, "HDRGainMap" as CFString, nil)
    CGImageMetadataSetValueWithPath(gainMeta, nil, "HDRGainMap:HDRGainMapVersion" as CFString, 65536 as CFNumber)
    let info: [CFString: Any] = [
        kCGImageAuxiliaryDataInfoData: Data(plane) as CFData,
        kCGImageAuxiliaryDataInfoDataDescription: [
            "Width": gw, "Height": gh, "BytesPerRow": gw,
            "PixelFormat": 0x4C30_3038, // kCVPixelFormatType_OneComponent8 ('L008')
        ],
        kCGImageAuxiliaryDataInfoMetadata: gainMeta,
    ]
    let props: [CFString: Any] = [kCGImagePropertyMakerAppleDictionary: ["33": 1.0, "48": 0.01]]
    CGImageDestinationAddImage(dest, base, props as CFDictionary)
    CGImageDestinationAddAuxiliaryDataInfo(dest, kCGImageAuxiliaryDataTypeHDRGainMap, info as CFDictionary)
}

// hlg.heic: 16-bit HLG pixels → 10-bit HEIC with nclx transfer 18
write("hlg.heic") { dest in
    let hlg = CGColorSpace(name: CGColorSpace.itur_2100_HLG)!
    let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 16, bytesPerRow: width * 8,
                        space: hlg, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
                            | CGBitmapInfo.byteOrder16Little.rawValue)!
    ctx.draw(base, in: CGRect(x: 0, y: 0, width: width, height: height))
    CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
}

// isogainmap.heic: needs macOS 15's ISO gain map encoder
if #available(macOS 15.0, *) {
    write("isogainmap.heic") { dest in
        let linear = CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3)!
        let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 16, bytesPerRow: width * 8,
                            space: linear, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
                                | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder16Little.rawValue)!
        ctx.draw(base, in: CGRect(x: 0, y: 0, width: width, height: height))
        // Highlights above SDR white so there is something for the gain map to encode.
        ctx.setFillColor(CGColor(colorSpace: linear, components: [3.0, 3.0, 3.0, 1.0])!)
        ctx.fill(CGRect(x: 160, y: 40, width: 64, height: 48))
        let options: [CFString: Any] = [kCGImageDestinationEncodeRequest: kCGImageDestinationEncodeToISOGainmap]
        CGImageDestinationAddImage(dest, ctx.makeImage()!, options as CFDictionary)
    }
}
