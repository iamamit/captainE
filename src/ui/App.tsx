import { useEffect, useRef } from "react";
import { ASPECTS } from "../core/model";
import { clipEnd, projectDuration, trackClips } from "../core/model";
import { formatTimecode, fromSeconds } from "../core/time";
import { probeFile } from "../media";
import { useEditor } from "../store";
import { Inspector } from "./Inspector";
import { Preview } from "./Preview";
import { Timeline } from "./Timeline";

export function App() {
  const s = useEditor();
  const { project, playhead, playing, selectedClipId, run, undo, redo, setPlayhead, setPlaying, select } = s;
  const fileInput = useRef<HTMLInputElement>(null);

  const importFiles = async (files: FileList | null) => {
    for (const file of Array.from(files ?? [])) {
      try {
        const asset = await probeFile(file);
        run({ type: "addAsset", asset });
        const trackId = asset.kind === "audio" ? "a1" : "v1";
        const end = trackClips(useEditor.getState().project, trackId).reduce((m, c) => Math.max(m, clipEnd(c)), 0);
        run({ type: "addClip", trackId, assetId: asset.id, start: end });
      } catch (e) {
        useEditor.setState({ error: e instanceof Error ? e.message : String(e) });
      }
    }
  };

  const splitAtPlayhead = () => {
    const c = selectedClipId && project.clips[selectedClipId];
    if (c) run({ type: "split", clipId: c.id, at: playhead });
  };
  const deleteSelected = () => {
    if (selectedClipId) { run({ type: "delete", clipId: selectedClipId, ripple: true }); select(null); }
  };
  const addText = () => run({ type: "addTextClip", trackId: "v1", text: "Your text", start: playhead, duration: fromSeconds(3) });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).matches("input, textarea")) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "z") { e.preventDefault(); e.shiftKey ? redo() : undo(); }
      else if (e.key === " ") { e.preventDefault(); useEditor.getState().setPlaying(!useEditor.getState().playing); }
      else if (e.key.toLowerCase() === "s") splitAtPlayhead();
      else if (e.key === "Delete" || e.key === "Backspace") deleteSelected();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="app">
      <div><header className="toolbar">
        <strong>CaptainE</strong>
        <button onClick={() => fileInput.current?.click()}>Import</button>
        <input ref={fileInput} data-testid="file-input" type="file" accept="video/*,audio/*,image/*" multiple hidden
          onChange={(e) => { void importFiles(e.target.files); e.target.value = ""; }} />
        <button onClick={addText}>Add text</button>
        <button onClick={splitAtPlayhead} disabled={!selectedClipId}>Split (S)</button>
        <button onClick={deleteSelected} disabled={!selectedClipId}>Delete</button>
        <button onClick={undo} disabled={!s.canUndo}>Undo</button>
        <button onClick={redo} disabled={!s.canRedo}>Redo</button>
        <select value={`${project.width}x${project.height}`}
          onChange={(e) => { const [w, h] = e.target.value.split("x").map(Number); run({ type: "setCanvas", width: w!, height: h! }); }}>
          {Object.entries(ASPECTS).map(([label, a]) => <option key={label} value={`${a.width}x${a.height}`}>{label}</option>)}
        </select>
      </header>
      {s.error && <div className="error" role="alert" onClick={() => useEditor.setState({ error: null })}>{s.error}</div>}
      </div>
      <main className="workspace">
        <Preview />
        <Inspector />
      </main>
      <div className="transport">
        <button onClick={() => setPlayhead(0)}>⏮</button>
        <button data-testid="play" onClick={() => { if (!playing && playhead >= projectDuration(project)) setPlayhead(0); setPlaying(!playing); }}>
          {playing ? "Pause" : "Play"}
        </button>
        <code data-testid="timecode">{formatTimecode(playhead, project.fps)}</code>
      </div>
      <Timeline />
    </div>
  );
}
