import { useRef } from "react";
import { clipEnd, projectDuration, trackClips } from "../core/model";
import { formatTimecode, fromSeconds, snapToFrame } from "../core/time";
import { useEditor } from "../store";

const PX_PER_SEC = 80;
const toPx = (us: number) => (us / 1e6) * PX_PER_SEC;
const toUs = (px: number) => fromSeconds(px / PX_PER_SEC);

export function Timeline() {
  const { project, playhead, selectedClipId, setPlayhead, select, run } = useEditor();
  const drag = useRef<{ id: string; startX: number; origin: number; moved: boolean } | null>(null);
  const width = Math.max(toPx(projectDuration(project)) + 400, 1000);

  const seek = (e: React.MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    setPlayhead(snapToFrame(toUs(e.clientX - rect.left), project.fps));
  };

  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (Math.abs(e.clientX - d.startX) > 3) d.moved = true;
    if (!d.moved) return;
    run({ type: "move", clipId: d.id, start: snapToFrame(Math.max(0, d.origin + toUs(e.clientX - d.startX)), project.fps) });
  };

  return (
    <div className="timeline" data-testid="timeline">
      <div className="timeline-scroll" style={{ width }}>
        <div className="ruler" onClick={seek} data-testid="ruler">
          {Array.from({ length: Math.ceil(width / PX_PER_SEC) }, (_, s) => (
            <span key={s} style={{ left: s * PX_PER_SEC }}>{s}s</span>
          ))}
        </div>
        {project.tracks.map((track) => (
          <div key={track.id} className={`track track-${track.kind}`} data-testid={`track-${track.id}`}>
            <div className="track-label">{track.kind === "video" ? "Video" : "Audio"} {track.id.slice(1)}</div>
            {trackClips(project, track.id).map((c) => (
              <div
                key={c.id}
                data-testid="clip"
                className={`clip ${c.kind} ${selectedClipId === c.id ? "selected" : ""}`}
                style={{ left: toPx(c.start), width: Math.max(toPx(c.duration), 4) }}
                title={`${formatTimecode(c.start, project.fps)} – ${formatTimecode(clipEnd(c), project.fps)}`}
                onPointerDown={(e) => {
                  e.stopPropagation();
                  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                  select(c.id);
                  drag.current = { id: c.id, startX: e.clientX, origin: c.start, moved: false };
                }}
                onPointerMove={onMove}
                onPointerUp={() => { drag.current = null; }}
              >
                {c.kind === "text" ? c.text : project.assets[c.assetId!]?.name}
              </div>
            ))}
          </div>
        ))}
        <div className="playhead" style={{ left: toPx(playhead) }} data-testid="playhead" />
      </div>
    </div>
  );
}
