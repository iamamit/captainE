import type { AnimatableProp } from "../core/model";
import { useEditor } from "../store";

const SLIDERS: { prop: AnimatableProp | "volume" | "speed"; label: string; min: number; max: number; step: number }[] = [
  { prop: "scale", label: "Scale", min: 0.1, max: 4, step: 0.05 },
  { prop: "rotation", label: "Rotate", min: -180, max: 180, step: 1 },
  { prop: "opacity", label: "Opacity", min: 0, max: 1, step: 0.01 },
  { prop: "x", label: "Position X", min: -1, max: 1, step: 0.01 },
  { prop: "y", label: "Position Y", min: -1, max: 1, step: 0.01 },
  { prop: "volume", label: "Volume", min: 0, max: 2, step: 0.05 },
  { prop: "speed", label: "Speed", min: 0.25, max: 4, step: 0.05 },
];

export function Inspector() {
  const { project, selectedClipId, playhead, run } = useEditor();
  const clip = selectedClipId ? project.clips[selectedClipId] : undefined;
  if (!clip) return <aside className="inspector"><p className="muted">Select a clip to edit it.</p></aside>;
  const rel = playhead - clip.start;
  const canKey = rel >= 0 && rel <= clip.duration;

  return (
    <aside className="inspector" data-testid="inspector">
      <h3>{clip.kind === "text" ? "Text" : project.assets[clip.assetId!]?.name}</h3>
      {clip.kind === "text" && (
        <label>Text
          <input value={clip.text} onChange={(e) => run({ type: "setProps", clipId: clip.id, props: { text: e.target.value } })} />
        </label>
      )}
      {SLIDERS.map(({ prop, label, min, max, step }) => {
        const animatable = prop !== "volume" && prop !== "speed";
        const keyed = animatable && (clip.keyframes[prop]?.length ?? 0) > 0;
        return (
          <label key={prop}>
            <span>{label} <output>{Number(clip[prop]).toFixed(2)}</output></span>
            <span className="row">
              <input type="range" min={min} max={max} step={step} value={clip[prop]}
                onChange={(e) => run({ type: "setProps", clipId: clip.id, props: { [prop]: Number(e.target.value) } })} />
              {animatable && (
                <button
                  title="Add keyframe at playhead" disabled={!canKey} className={keyed ? "key on" : "key"}
                  onClick={() => run({ type: "setKeyframe", clipId: clip.id, prop, t: rel, value: clip[prop] })}>◆</button>
              )}
            </span>
          </label>
        );
      })}
    </aside>
  );
}
