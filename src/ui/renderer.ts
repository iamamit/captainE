import { valueAt } from "../core/animate";
import type { Clip, Project } from "../core/model";
import { trackClips } from "../core/model";
import type { Time } from "../core/time";

type Source = HTMLVideoElement | HTMLAudioElement | HTMLImageElement;
const sources = new Map<string, Source>();

function sourceFor(url: string, kind: "video" | "image" | "audio"): Source | null {
  let s = sources.get(url);
  if (!s) {
    s = kind === "image" ? Object.assign(new Image(), { src: url })
      : Object.assign(document.createElement(kind), { preload: "auto", src: url });
    sources.set(url, s);
  }
  return s;
}

export const clipAt = (p: Project, trackId: string, t: Time): Clip | undefined =>
  trackClips(p, trackId).find((c) => t >= c.start && t < c.start + c.duration);

/** Seek the video element for a clip so the frame at timeline time `t` is ready. */
export function prepareFrame(p: Project, t: Time, playing: boolean): void {
  for (const track of p.tracks) {
    const clip = clipAt(p, track.id, t);
    if (!clip || clip.kind !== "media") continue;
    const asset = p.assets[clip.assetId!];
    if (!asset || asset.kind === "image") continue;
    const v = sourceFor(asset.url, asset.kind) as HTMLMediaElement;
    v.volume = Math.min(1, Math.max(0, clip.volume));
    const want = (clip.sourceIn + (t - clip.start) * clip.speed) / 1e6;
    v.playbackRate = clip.speed;
    if (playing) {
      if (Math.abs(v.currentTime - want) > 0.25) v.currentTime = want;
      if (v.paused) void v.play().catch(() => {});
    } else {
      if (!v.paused) v.pause();
      if (Math.abs(v.currentTime - want) > 0.01) v.currentTime = want;
    }
  }
  // pause videos no longer under the playhead
  for (const [url, s] of sources) {
    if (s instanceof HTMLImageElement || s.paused) continue;
    const active = p.tracks.some((tr) => {
      const c = clipAt(p, tr.id, t);
      return c?.kind === "media" && p.assets[c.assetId!]?.url === url;
    });
    if (!active) s.pause();
  }
}

export function drawFrame(ctx: CanvasRenderingContext2D, p: Project, t: Time): void {
  const { width: W, height: H } = ctx.canvas;
  ctx.fillStyle = "#000";
  ctx.fillRect(0, 0, W, H);
  // bottom track first so upper tracks composite on top
  for (const track of [...p.tracks].reverse()) {
    if (track.kind !== "video") continue;
    const clip = clipAt(p, track.id, t);
    if (!clip) continue;
    const rel = t - clip.start;
    const opacity = valueAt(clip, "opacity", rel);
    const scale = valueAt(clip, "scale", rel);
    const rot = (valueAt(clip, "rotation", rel) * Math.PI) / 180;
    const cx = W / 2 + (valueAt(clip, "x", rel) * W) / 2;
    const cy = H / 2 + (valueAt(clip, "y", rel) * H) / 2;
    ctx.save();
    ctx.globalAlpha = opacity;
    ctx.translate(cx, cy);
    ctx.rotate(rot);
    ctx.scale(scale, scale);
    if (clip.kind === "text") {
      ctx.fillStyle = "#fff";
      ctx.strokeStyle = "#000";
      ctx.lineWidth = H * 0.008;
      ctx.font = `700 ${Math.round(H * 0.07)}px system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.strokeText(clip.text ?? "", 0, 0);
      ctx.fillText(clip.text ?? "", 0, 0);
    } else {
      const asset = p.assets[clip.assetId!];
      const src = asset && asset.kind !== "audio" ? sourceFor(asset.url, asset.kind) : null;
      const w = src instanceof HTMLVideoElement ? src.videoWidth : src instanceof HTMLImageElement ? src.naturalWidth : 0;
      const h = src instanceof HTMLVideoElement ? src.videoHeight : src instanceof HTMLImageElement ? src.naturalHeight : 0;
      if (src && !(src instanceof HTMLAudioElement) && w && h && (!(src instanceof HTMLVideoElement) || src.readyState >= 2)) {
        const fit = Math.min(W / w, H / h); // letterbox: fit inside the canvas
        ctx.drawImage(src, (-w * fit) / 2, (-h * fit) / 2, w * fit, h * fit);
      }
    }
    ctx.restore();
  }
}
