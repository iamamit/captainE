import { describe, expect, it } from "vitest";
import { apply, CommandError } from "../src/core/commands";
import { History } from "../src/core/history";
import { newProject, projectDuration, trackClips } from "../src/core/model";
import { valueAt } from "../src/core/animate";
import { formatTimecode, fromSeconds, snapToFrame } from "../src/core/time";

const asset = { id: "a", name: "a.mp4", kind: "video" as const, duration: fromSeconds(10), url: "blob:x" };

function setup() {
  const h = new History(newProject());
  h.run({ type: "addAsset", asset });
  h.run({ type: "addClip", trackId: "v1", assetId: "a", clipId: "c1" });
  return h;
}

describe("time", () => {
  it("formats timecode and snaps to frames", () => {
    expect(formatTimecode(fromSeconds(61.5), 30)).toBe("00:01:01:15");
    expect(snapToFrame(fromSeconds(1.01), 30)).toBe(Math.round((30 / 30) * 1_000_000));
  });
});

describe("commands", () => {
  it("adds clips back to back without overlap", () => {
    const h = setup();
    h.run({ type: "addClip", trackId: "v1", assetId: "a", start: 0, clipId: "c2" });
    expect(h.present.clips.c2!.start).toBe(fromSeconds(10));
    expect(projectDuration(h.present)).toBe(fromSeconds(20));
  });

  it("splits a clip and keeps the source range continuous", () => {
    const h = setup();
    h.run({ type: "split", clipId: "c1", at: fromSeconds(4), newClipId: "c2" });
    const [l, r] = trackClips(h.present, "v1");
    expect(l!.duration).toBe(fromSeconds(4));
    expect(r!.start).toBe(fromSeconds(4));
    expect(r!.sourceIn).toBe(fromSeconds(4));
    expect(r!.duration).toBe(fromSeconds(6));
  });

  it("rejects a split outside the clip", () => {
    expect(() => apply(setup().present, { type: "split", clipId: "c1", at: fromSeconds(11) })).toThrow(CommandError);
  });

  it("trims the start, advancing the source offset", () => {
    const h = setup();
    h.run({ type: "trim", clipId: "c1", edge: "start", to: fromSeconds(2) });
    const c = h.present.clips.c1!;
    expect(c.start).toBe(fromSeconds(2));
    expect(c.sourceIn).toBe(fromSeconds(2));
    expect(c.duration).toBe(fromSeconds(8));
  });

  it("cannot extend the end past the source", () => {
    expect(() => apply(setup().present, { type: "trim", clipId: "c1", edge: "end", to: fromSeconds(12) })).toThrow();
  });

  it("ripple delete closes the gap", () => {
    const h = setup();
    h.run({ type: "addClip", trackId: "v1", assetId: "a", clipId: "c2" });
    h.run({ type: "delete", clipId: "c1", ripple: true });
    expect(h.present.clips.c2!.start).toBe(0);
    expect(h.present.tracks[0]!.clipIds).toEqual(["c2"]);
  });

  it("speed change rescales duration over the same source range", () => {
    const h = setup();
    h.run({ type: "setProps", clipId: "c1", props: { speed: 2 } });
    expect(h.present.clips.c1!.duration).toBe(fromSeconds(5));
  });

  it("moves clips between tracks and refuses overlaps", () => {
    const h = setup();
    h.run({ type: "addClip", trackId: "v1", assetId: "a", clipId: "c2" });
    expect(() => h.run({ type: "move", clipId: "c2", start: fromSeconds(5) })).toThrow();
    h.run({ type: "move", clipId: "c2", start: 0, trackId: "a1" });
    expect(h.present.tracks[1]!.clipIds).toEqual(["c2"]);
    expect(h.present.tracks[0]!.clipIds).toEqual(["c1"]);
  });
});

describe("history", () => {
  it("undoes, redoes and logs", () => {
    const h = setup();
    h.run({ type: "split", clipId: "c1", at: fromSeconds(4), newClipId: "c2" });
    h.undo();
    expect(Object.keys(h.present.clips)).toEqual(["c1"]);
    h.redo();
    expect(Object.keys(h.present.clips)).toHaveLength(2);
    expect(h.log.map((c) => c.type)).toEqual(["addAsset", "addClip", "split"]);
  });

  it("a failed command leaves state untouched", () => {
    const h = setup();
    const before = h.present;
    expect(() => h.run({ type: "split", clipId: "nope", at: 1 })).toThrow();
    expect(h.present).toBe(before);
  });

  it("batch is all-or-nothing and one undo step", () => {
    const h = setup();
    const before = h.present;
    expect(() => h.batch([
      { type: "split", clipId: "c1", at: fromSeconds(4), newClipId: "c2" },
      { type: "split", clipId: "missing", at: 1 },
    ])).toThrow();
    expect(h.present).toBe(before);
    h.batch([
      { type: "split", clipId: "c1", at: fromSeconds(4), newClipId: "c2" },
      { type: "split", clipId: "c2", at: fromSeconds(7), newClipId: "c3" },
    ]);
    expect(Object.keys(h.present.clips)).toHaveLength(3);
    h.undo();
    expect(h.present).toBe(before);
  });
});

describe("keyframes", () => {
  it("interpolates linearly and clamps outside the range", () => {
    const h = setup();
    h.run({ type: "setKeyframe", clipId: "c1", prop: "scale", t: 0, value: 1 });
    h.run({ type: "setKeyframe", clipId: "c1", prop: "scale", t: fromSeconds(2), value: 2 });
    const c = h.present.clips.c1!;
    expect(valueAt(c, "scale", fromSeconds(1))).toBeCloseTo(1.5);
    expect(valueAt(c, "scale", fromSeconds(5))).toBe(2);
    expect(valueAt(c, "opacity", 0)).toBe(1);
  });
});
