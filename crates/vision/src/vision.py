"""captainE's window onto Apple's Vision framework (run with a Python that has PyObjC).

    vision.py matte    FRAMES_DIR OUT_DIR [foreground|person]   a grey matte PNG per frame
    vision.py faces    FRAMES_DIR                                JSON: face boxes per frame
    vision.py saliency FRAMES_DIR                                JSON: where the eye goes, per frame
    vision.py track    FRAMES_DIR X Y W H                        JSON: follow one box (normalised, top-left origin)

FRAMES_DIR holds PNG files; they are read in name order. Boxes are fractions of the frame, origin top-left.
"""
import glob
import json
import os
import sys

import Quartz
import Vision
from Foundation import NSURL
from Quartz import CIContext, CIImage


def frames(d):
    return sorted(glob.glob(os.path.join(d, "*.png")))


def handler_for(path):
    return Vision.VNImageRequestHandler.alloc().initWithURL_options_(NSURL.fileURLWithPath_(path), {})


def image_size(path):
    src = Quartz.CGImageSourceCreateWithURL(NSURL.fileURLWithPath_(path), None)
    props = Quartz.CGImageSourceCopyPropertiesAtIndex(src, 0, None)
    return int(props["PixelWidth"]), int(props["PixelHeight"])


def top_left(box):
    """Vision boxes have their origin bottom-left; ours is top-left."""
    o, s = box.origin, box.size
    return [float(o.x), float(1.0 - o.y - s.height), float(s.width), float(s.height)]


_ctx = None


def write_matte(ci, w, h, out):
    global _ctx
    if _ctx is None:
        _ctx = CIContext.alloc().init()
    ext = ci.extent()
    if int(ext.size.width) != w or int(ext.size.height) != h:
        ci = ci.imageByApplyingTransform_(Quartz.CGAffineTransformMakeScale(w / ext.size.width, h / ext.size.height))
    gray = Quartz.CGColorSpaceCreateWithName(Quartz.kCGColorSpaceGenericGrayGamma2_2)
    ok, err = _ctx.writePNGRepresentationOfImage_toURL_format_colorSpace_options_error_(
        ci, NSURL.fileURLWithPath_(out), Quartz.kCIFormatL8, gray, {}, None
    )
    if not ok:
        raise RuntimeError("could not write %s: %s" % (out, err))


def empty_matte(w, h, out):
    black = CIImage.imageWithColor_(Quartz.CIColor.colorWithRed_green_blue_(0, 0, 0))
    write_matte(black.imageByCroppingToRect_(Quartz.CGRectMake(0, 0, w, h)), w, h, out)


def segment(handler, mode):
    """The subject matte for the picture behind `handler` as a CIImage at the model's own size, or None."""
    if mode == "person":
        req = Vision.VNGeneratePersonSegmentationRequest.alloc().initWithCompletionHandler_(None)
        req.setQualityLevel_(Vision.VNGeneratePersonSegmentationRequestQualityLevelAccurate)
        ok, _ = handler.performRequests_error_([req], None)
        res = req.results() if ok else None
        return CIImage.imageWithCVPixelBuffer_(res[0].pixelBuffer()) if res else None
    req = Vision.VNGenerateForegroundInstanceMaskRequest.alloc().initWithCompletionHandler_(None)
    ok, _ = handler.performRequests_error_([req], None)
    res = req.results() if ok else None
    if not res:
        return None
    mask, _ = res[0].generateScaledMaskForImageForInstances_fromRequestHandler_error_(
        res[0].allInstances(), handler, None
    )
    return CIImage.imageWithCVPixelBuffer_(mask) if mask is not None else None


def brightest(ci):
    """The largest value in a grey image, 0..1."""
    global _ctx
    if _ctx is None:
        _ctx = CIContext.alloc().init()
    f = Quartz.CIFilter.filterWithName_("CIAreaMaximum")
    f.setValue_forKey_(ci, "inputImage")
    f.setValue_forKey_(Quartz.CIVector.vectorWithCGRect_(ci.extent()), "inputExtent")
    px = bytearray(4)
    _ctx.render_toBitmap_rowBytes_bounds_format_colorSpace_(
        f.outputImage(), px, 4, Quartz.CGRectMake(0, 0, 1, 1), Quartz.kCIFormatRGBA8, None
    )
    return px[0] / 255.0


def fit(ci, w, h, x=0.0, y=0.0):
    """`ci` scaled to w x h and moved to (x, y) (bottom-left origin)."""
    ext = ci.extent()
    t = Quartz.CGAffineTransformMakeScale(w / ext.size.width, h / ext.size.height)
    t = Quartz.CGAffineTransformConcat(
        Quartz.CGAffineTransformMakeTranslation(-ext.origin.x, -ext.origin.y), t
    )
    t = Quartz.CGAffineTransformConcat(t, Quartz.CGAffineTransformMakeTranslation(x, y))
    return ci.imageByApplyingTransform_(t)


def small_subjects(path, w, h, mode):
    """Matte for subjects too small in the frame for the segmentation model: find them (people, or for
    foreground what draws the eye), segment a square crop around each at a size the model can see, and put the
    pieces back in place. None when nothing is found."""
    full = CIImage.imageWithContentsOfURL_(NSURL.fileURLWithPath_(path))
    finder = (
        Vision.VNDetectHumanRectanglesRequest.alloc().initWithCompletionHandler_(None)
        if mode == "person"
        else Vision.VNGenerateObjectnessBasedSaliencyImageRequest.alloc().initWithCompletionHandler_(None)
    )
    if mode == "person":
        finder.setUpperBodyOnly_(False)
    ok, _ = handler_for(path).performRequests_error_([finder], None)
    found = finder.results() if ok else None
    if not found:
        return None
    boxes = (
        [o.boundingBox() for o in found]
        if mode == "person"
        else [o.boundingBox() for o in (found[0].salientObjects() or [])]
    )
    canvas = None
    for b in boxes:
        bw, bh = b.size.width * w, b.size.height * h
        side = min(max(bw, bh) * 1.6, w, h)
        side = max(side, min(256.0, w, h))
        cx, cy = (b.origin.x + b.size.width / 2) * w, (b.origin.y + b.size.height / 2) * h
        x = min(max(cx - side / 2, 0.0), w - side)
        y = min(max(cy - side / 2, 0.0), h - side)
        crop = fit(full.imageByCroppingToRect_(Quartz.CGRectMake(x, y, side, side)), side, side)
        piece = segment(Vision.VNImageRequestHandler.alloc().initWithCIImage_options_(crop, {}), mode)
        if piece is None:
            continue
        piece = fit(piece, side, side, x, y)
        if canvas is None:
            canvas = piece
        else:
            f = Quartz.CIFilter.filterWithName_("CIMaximumCompositing")
            f.setValue_forKey_(piece, "inputImage")
            f.setValue_forKey_(canvas, "inputBackgroundImage")
            canvas = f.outputImage()
    if canvas is None:
        return None
    # a crop's matte keeps a faint haze around the subject that shows as a ghost of the background: take
    # everything under 15% to nothing and stretch the rest back to 0..1
    lift = Quartz.CIFilter.filterWithName_("CIColorMatrix")
    lift.setValue_forKey_(canvas, "inputImage")
    k, b = 1 / 0.85, -0.15 / 0.85
    for name, vec in (("inputRVector", (k, 0, 0, 0)), ("inputGVector", (0, k, 0, 0)), ("inputBVector", (0, 0, k, 0))):
        lift.setValue_forKey_(Quartz.CIVector.vectorWithX_Y_Z_W_(*vec), name)
    lift.setValue_forKey_(Quartz.CIVector.vectorWithX_Y_Z_W_(b, b, b, 0), "inputBiasVector")
    clamp = Quartz.CIFilter.filterWithName_("CIColorClamp")
    clamp.setValue_forKey_(lift.outputImage(), "inputImage")
    canvas = clamp.outputImage()
    black = CIImage.imageWithColor_(Quartz.CIColor.colorWithRed_green_blue_(0, 0, 0))
    return canvas.imageByCompositingOverImage_(black).imageByCroppingToRect_(Quartz.CGRectMake(0, 0, w, h))


def matte_one(path, out, mode):
    w, h = image_size(path)
    ci = segment(handler_for(path), mode)
    if ci is not None and brightest(ci) > 0.5:
        write_matte(ci, w, h, out)
        return
    # nothing found in the whole frame: the subject may just be small in it
    ci = small_subjects(path, w, h, mode)
    if ci is not None:
        write_matte(ci, w, h, out)
        return
    empty_matte(w, h, out)


def cmd_matte(d, out_dir, mode="foreground"):
    os.makedirs(out_dir, exist_ok=True)
    for p in frames(d):
        matte_one(p, os.path.join(out_dir, os.path.basename(p)), mode)
    print(json.dumps({"frames": len(frames(d))}))


def cmd_faces(d):
    out = []
    for p in frames(d):
        req = Vision.VNDetectFaceRectanglesRequest.alloc().initWithCompletionHandler_(None)
        ok, _ = handler_for(p).performRequests_error_([req], None)
        out.append([top_left(r.boundingBox()) for r in (req.results() or [])] if ok else [])
    print(json.dumps(out))


def cmd_saliency(d):
    out = []
    for p in frames(d):
        req = Vision.VNGenerateAttentionBasedSaliencyImageRequest.alloc().initWithCompletionHandler_(None)
        ok, _ = handler_for(p).performRequests_error_([req], None)
        boxes = []
        if ok and req.results():
            for o in req.results()[0].salientObjects() or []:
                boxes.append(top_left(o.boundingBox()))
        out.append(boxes)
    print(json.dumps(out))


def cmd_track(d, x, y, w, h):
    seq = Vision.VNSequenceRequestHandler.alloc().init()
    # Vision wants bottom-left origin
    box = Quartz.CGRectMake(x, 1.0 - y - h, w, h)
    obs = Vision.VNDetectedObjectObservation.observationWithBoundingBox_(box)
    out = []
    for p in frames(d):
        req = Vision.VNTrackObjectRequest.alloc().initWithDetectedObjectObservation_(obs)
        req.setTrackingLevel_(Vision.VNRequestTrackingLevelAccurate)
        ok, _ = seq.performRequests_onImageURL_error_([req], NSURL.fileURLWithPath_(p), None)
        res = req.results() if ok else None
        if res:
            obs = res[0]
            out.append(top_left(obs.boundingBox()) + [float(obs.confidence())])
        else:
            out.append(out[-1] if out else [x, y, w, h, 0.0])
    print(json.dumps(out))


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 2
    cmd, d = argv[1], argv[2]
    if cmd == "matte":
        cmd_matte(d, argv[3], argv[4] if len(argv) > 4 else "foreground")
    elif cmd == "faces":
        cmd_faces(d)
    elif cmd == "saliency":
        cmd_saliency(d)
    elif cmd == "track":
        cmd_track(d, *[float(v) for v in argv[3:7]])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
