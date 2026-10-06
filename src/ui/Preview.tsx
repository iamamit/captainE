import { useEffect, useRef } from "react";
import { projectDuration } from "../core/model";
import { useEditor } from "../store";
import { drawFrame, prepareFrame } from "./renderer";

export function Preview() {
  const ref = useRef<HTMLCanvasElement>(null);
  const project = useEditor((s) => s.project);

  // Playback clock + render loop. State is read via getState() so the loop never goes stale.
  useEffect(() => {
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const s = useEditor.getState();
      if (s.playing) {
        const next = s.playhead + (now - last) * 1000;
        if (next >= projectDuration(s.project)) s.setPlaying(false);
        else s.setPlayhead(next);
      }
      last = now;
      const ctx = ref.current?.getContext("2d");
      const st = useEditor.getState();
      if (ctx) {
        prepareFrame(st.project, st.playhead, st.playing);
        drawFrame(ctx, st.project, st.playhead);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  const aspect = project.width / project.height;
  return (
    <div className="preview">
      <canvas
        ref={ref}
        data-testid="preview"
        width={project.width / 2}
        height={project.height / 2}
        style={{ aspectRatio: String(aspect) }}
      />
    </div>
  );
}
