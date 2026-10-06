/** Time is integer microseconds: exact, no floating-point drift across edits. */
export type Time = number;

export const US_PER_SEC = 1_000_000;

export const fromSeconds = (s: number): Time => Math.round(s * US_PER_SEC);
export const toSeconds = (t: Time): number => t / US_PER_SEC;

/** Snap a time to the nearest frame boundary at the given fps. */
export const snapToFrame = (t: Time, fps: number): Time =>
  Math.round(Math.round((t / US_PER_SEC) * fps) * (US_PER_SEC / fps));

export const frameAt = (t: Time, fps: number): number => Math.floor((t / US_PER_SEC) * fps + 1e-9);

export function formatTimecode(t: Time, fps: number): string {
  const totalFrames = Math.round((t / US_PER_SEC) * fps);
  const f = totalFrames % Math.round(fps);
  const totalSec = Math.floor(totalFrames / Math.round(fps));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(totalSec / 3600))}:${p(Math.floor(totalSec / 60) % 60)}:${p(totalSec % 60)}:${p(f)}`;
}
