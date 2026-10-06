import type { Time } from "./time";

export type Id = string;
export type TrackKind = "video" | "audio";

export interface Asset {
  id: Id;
  name: string;
  kind: "video" | "audio" | "image";
  duration: Time; // 0 for images
  width?: number;
  height?: number;
  url: string; // object URL in the browser
}

export interface Keyframe {
  t: Time; // relative to clip start on the timeline
  value: number;
}

export type AnimatableProp = "x" | "y" | "scale" | "rotation" | "opacity";

export interface Clip {
  id: Id;
  trackId: Id;
  kind: "media" | "text";
  assetId?: Id;
  text?: string;
  start: Time; // position on timeline
  duration: Time; // length on timeline
  sourceIn: Time; // offset into the asset
  speed: number;
  volume: number;
  x: number; // normalised -1..1 offsets from centre
  y: number;
  scale: number;
  rotation: number; // degrees
  opacity: number;
  keyframes: Partial<Record<AnimatableProp, Keyframe[]>>;
}

export interface Track {
  id: Id;
  kind: TrackKind;
  clipIds: Id[];
}

export interface Project {
  id: Id;
  name: string;
  fps: number;
  width: number;
  height: number;
  assets: Record<Id, Asset>;
  tracks: Track[];
  clips: Record<Id, Clip>;
}

export const ASPECTS = {
  "9:16": { width: 1080, height: 1920 },
  "1:1": { width: 1080, height: 1080 },
  "16:9": { width: 1920, height: 1080 },
  "4:5": { width: 1080, height: 1350 },
} as const;

export function newProject(name = "Untitled"): Project {
  return {
    id: "p1",
    name,
    fps: 30,
    width: ASPECTS["16:9"].width,
    height: ASPECTS["16:9"].height,
    assets: {},
    tracks: [
      { id: "v1", kind: "video", clipIds: [] },
      { id: "a1", kind: "audio", clipIds: [] },
    ],
    clips: {},
  };
}

export const clipEnd = (c: Clip): Time => c.start + c.duration;

export function projectDuration(p: Project): Time {
  return Object.values(p.clips).reduce((m, c) => Math.max(m, clipEnd(c)), 0);
}

/** Clips on a track, ordered by start time. */
export function trackClips(p: Project, trackId: Id): Clip[] {
  const t = p.tracks.find((x) => x.id === trackId);
  if (!t) return [];
  return t.clipIds.map((id) => p.clips[id]!).sort((a, b) => a.start - b.start);
}
