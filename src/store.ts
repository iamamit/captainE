import { create } from "zustand";
import type { Command } from "./core/commands";
import { History } from "./core/history";
import { newProject, type Id, type Project } from "./core/model";
import type { Time } from "./core/time";

const history = new History(newProject("My video"));

interface EditorState {
  project: Project;
  playhead: Time;
  playing: boolean;
  selectedClipId: Id | null;
  error: string | null;
  canUndo: boolean;
  canRedo: boolean;
  run: (cmd: Command) => boolean;
  undo: () => void;
  redo: () => void;
  setPlayhead: (t: Time) => void;
  setPlaying: (p: boolean) => void;
  select: (id: Id | null) => void;
}

export const useEditor = create<EditorState>((set) => {
  const sync = () => ({ project: history.present, canUndo: history.canUndo, canRedo: history.canRedo });
  return {
    ...sync(),
    playhead: 0,
    playing: false,
    selectedClipId: null,
    error: null,
    run: (cmd) => {
      try {
        history.run(cmd);
        set({ ...sync(), error: null });
        return true;
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) });
        return false;
      }
    },
    undo: () => { history.undo(); set({ ...sync(), error: null }); },
    redo: () => { history.redo(); set({ ...sync(), error: null }); },
    setPlayhead: (t) => set({ playhead: Math.max(0, t) }),
    setPlaying: (playing) => set({ playing }),
    select: (selectedClipId) => set({ selectedClipId }),
  };
});
