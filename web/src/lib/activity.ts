import type { Snapshot, StageStatus } from "@/api/schema";
import { orderStages } from "@/lib/levels";
import type { ActivityEntry } from "@/state/atoms";

export const MAX_ACTIVITY_ENTRIES = 20;

function transitionMessage(
  id: string,
  status: StageStatus,
  previous: StageStatus | undefined,
): string | null {
  switch (status) {
    case "executing":
      return `${id} started`;
    case "completed":
      return `${id} completed`;
    case "blocked":
      return `${id} blocked`;
    case "queued":
      // An Accept or NeedsMoreEvidence verdict requeues the stage for a fresh
      // session; a bare "ready" would read as an unexplained restart.
      return previous === "needs-adjudication" ? `${id} verdict applied, requeued` : `${id} ready`;
    case "needs-handoff":
      return `${id} needs handoff`;
    case "needs-adjudication":
      return `${id} entered adjudication`;
    case "needs-human-review":
      return `${id} needs human review`;
    default:
      return null;
  }
}

export function statusesById(snapshot: Snapshot | null): Map<string, StageStatus> {
  const statuses = new Map<string, StageStatus>();
  for (const stage of snapshot?.status.stages ?? []) {
    if (!statuses.has(stage.id)) {
      statuses.set(stage.id, stage.status);
    }
  }
  return statuses;
}

/**
 * Append meaningful stage transitions while retaining the newest twenty
 * entries. The first frame is a baseline, not a burst of transitions: the
 * wire carries no timestamps for what happened before the page opened, so
 * stamping every stage with the load time would show the same wrong age on
 * every row. Only changes the page itself observes are logged.
 */
export function appendTransitions(
  log: readonly ActivityEntry[],
  previous: Snapshot | null,
  next: Snapshot,
  now: number,
): ActivityEntry[] {
  if (previous === null) {
    return [...log];
  }
  const entries = [...log];
  const previousStatuses = statusesById(previous);

  // Order like the TUI (level, then id) so simultaneous transitions log in a
  // stable sequence instead of raw, filesystem-dependent wire order.
  // orderStages already dedupes by id (keeping the first), which is why the
  // dedupe set that used to live here was removed.
  for (const { stage } of orderStages(next.status.stages)) {
    const previousStatus = previousStatuses.get(stage.id);
    if (previousStatus === stage.status) {
      continue;
    }
    const message = transitionMessage(stage.id, stage.status, previousStatus);
    if (message) {
      entries.push({ at: now, stageId: stage.id, status: stage.status, message });
    }
  }
  return entries.slice(-MAX_ACTIVITY_ENTRIES);
}
