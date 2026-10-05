// Work that takes a while or changes things (sending, converting, adding,
// deleting): what's running, shown in the status bar, and what finished this
// session, in its history. The history lives only in memory: what the jobs
// did is in the library already; this is the account of it.

import { createSignal } from 'solid-js';

import { toast } from './toast';

export type Job = {
  id: number;
  /** What it is, e.g. "Send to Kobo". */
  title: string;
  /** Progress while running, then the one-line outcome. */
  status: string;
  state: 'running' | 'done' | 'failed';
  started: Date;
  finished: Date | null;
  /** Per-book lines worth reading afterwards: failures, skips, warnings. */
  details: string[];
};

/** Kept, newest first; older ones drop off. */
const LIMIT = 50;

const [jobs, setJobs] = createSignal<Job[]>([]);
/** Failed jobs not looked at since. */
const [unseenFailures, setUnseenFailures] = createSignal(0);
export { jobs, unseenFailures };

let nextId = 1;

/** The job running now, if any (the most recent, if several). */
export const runningJob = () => jobs().find((j) => j.state === 'running');

function update(id: number, change: Partial<Job>): void {
  setJobs((list) => list.map((j) => (j.id === id ? { ...j, ...change } : j)));
}

export type JobHandle = {
  /** Say where it's got to (in the status bar, and a toast that stays up). */
  progress(status: string): void;
  /**
   * Record the outcome and show it in a toast. `error` marks it failed (it
   * stays flagged in the status bar until the history is opened); `details`
   * are the lines behind the summary.
   */
  finish(summary: string, options?: { error?: boolean; details?: string[]; duration?: number }): void;
};

function add(title: string, status: string): number {
  const id = nextId++;
  const job: Job = { id, title, status, state: 'running', started: new Date(), finished: null, details: [] };
  setJobs((list) => [job, ...list].slice(0, LIMIT));
  return id;
}

function finisher(id: number): JobHandle['finish'] {
  return (summary, { error = false, details = [], duration } = {}) => {
    update(id, { status: summary, state: error ? 'failed' : 'done', finished: new Date(), details });
    if (error) setUnseenFailures((n) => n + 1);
    for (const line of details) console.info(line);
    toast(summary, { error, duration: duration ?? (error ? 7000 : 3500) });
  };
}

/** Start a job, showing `status` until it reports progress. */
export function startJob(title: string, status: string): JobHandle {
  const id = add(title, status);
  toast(status, { duration: Infinity });
  return {
    progress(s) {
      update(id, { status: s });
      toast(s, { duration: Infinity });
    },
    finish: finisher(id),
  };
}

/** Record something that finished at once (a delete, a merge). */
export function recordJob(
  title: string,
  summary: string,
  options?: { error?: boolean; details?: string[]; duration?: number },
): void {
  finisher(add(title, summary))(summary, options);
}

/** The history was looked at. */
export function markJobsSeen(): void {
  setUnseenFailures(0);
}

/** Forget finished jobs (running ones stay). */
export function clearJobs(): void {
  setJobs((list) => list.filter((j) => j.state === 'running'));
  setUnseenFailures(0);
}
