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


def matte_one(path, out, mode):
    w, h = image_size(path)
    handler = handler_for(path)
    if mode == "person":
        req = Vision.VNGeneratePersonSegmentationRequest.alloc().initWithCompletionHandler_(None)
        req.setQualityLevel_(Vision.VNGeneratePersonSegmentationRequestQualityLevelAccurate)
        ok, _ = handler.performRequests_error_([req], None)
        res = req.results() if ok else None
        if res:
            write_matte(CIImage.imageWithCVPixelBuffer_(res[0].pixelBuffer()), w, h, out)
            return
    else:
        req = Vision.VNGenerateForegroundInstanceMaskRequest.alloc().initWithCompletionHandler_(None)
        ok, _ = handler.performRequests_error_([req], None)
        res = req.results() if ok else None
        if res:
            inst = res[0].allInstances()
            mask, err = res[0].generateScaledMaskForImageForInstances_fromRequestHandler_error_(inst, handler, None)
            if mask is not None:
                write_matte(CIImage.imageWithCVPixelBuffer_(mask), w, h, out)
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
