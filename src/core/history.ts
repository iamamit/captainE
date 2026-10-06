import { apply, type Command } from "./commands";
import type { Project } from "./model";

/**
 * Undo/redo over immutable snapshots (cheap thanks to structural sharing) plus an
 * operation log of every command applied — the log is what the AI tool and
 * (later) branching/diffing work from.
 */
export class History {
  private past: Project[] = [];
  private future: Project[] = [];
  readonly log: Command[] = [];

  constructor(public present: Project) {}

  get canUndo() { return this.past.length > 0; }
  get canRedo() { return this.future.length > 0; }

  run(cmd: Command): Project {
    const next = apply(this.present, cmd); // throws before any state changes
    this.past.push(this.present);
    this.future = [];
    this.present = next;
    this.log.push(cmd);
    return next;
  }

  /** Apply several commands as one undo step; all-or-nothing. */
  batch(cmds: Command[]): Project {
    let next = this.present;
    for (const c of cmds) next = apply(next, c);
    this.past.push(this.present);
    this.future = [];
    this.present = next;
    this.log.push(...cmds);
    return next;
  }

  undo(): Project {
    const prev = this.past.pop();
    if (prev) { this.future.push(this.present); this.present = prev; }
    return this.present;
  }

  redo(): Project {
    const next = this.future.pop();
    if (next) { this.past.push(this.present); this.present = next; }
    return this.present;
  }
}
