import type { AnimatableProp, Clip } from "./model";
import type { Time } from "./time";

/** Value of an animatable property at clip-relative time `t` (linear between keyframes). */
export function valueAt(clip: Clip, prop: AnimatableProp, t: Time): number {
  const kfs = clip.keyframes[prop];
  if (!kfs || kfs.length === 0) return clip[prop];
  const first = kfs[0]!;
  if (t <= first.t) return first.value;
  const last = kfs[kfs.length - 1]!;
  if (t >= last.t) return last.value;
  for (let i = 0; i < kfs.length - 1; i++) {
    const a = kfs[i]!, b = kfs[i + 1]!;
    if (t >= a.t && t <= b.t) return a.value + ((b.value - a.value) * (t - a.t)) / (b.t - a.t);
  }
  return last.value;
}
