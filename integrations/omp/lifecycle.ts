export type Activity = "idle" | "working" | "needs-input" | "unknown";
export type LifecycleEvent = { type: string; reason?: string; willContinue?: boolean; willRetry?: boolean; toolName?: string; toolCallId?: string; hasUI?: boolean };

/** Notification-only reducer for OMP 18.8.7; a turn or stop-hook pass is not a settle. */
export class Lifecycle {
  activity: Activity = "idle";
  reason = "session-start";
  generation = 0;
  completedGeneration: number | null = null;
  retrying = false;
  shutdown = false;
  private ended = false;
  private compactionWasSettled = false;
  private waiting = new Map<string, string>();

  get settled(): boolean {
    return !this.shutdown && !this.retrying && this.activity === "idle" && this.ended && this.completedGeneration === this.generation;
  }
  apply(event: LifecycleEvent): void {
    switch (event.type) {
      case "session_start": case "session_switch": case "session_branch": case "session_tree":
        this.activity = "idle";
        this.reason = event.type;
        this.generation = 0;
        this.completedGeneration = null;
        this.retrying = false;
        this.shutdown = false;
        this.ended = false;
        this.waiting.clear();
        this.compactionWasSettled = false;
        break;
      case "agent_start":
        this.generation++;
        this.ended = false;
        this.completedGeneration = null;
        this.activity = "working";
        this.reason = "agent-start";
        break;
      case "turn_start": case "session_stop":
        this.activity = "working";
        this.ended = false;
        this.reason = event.type;
        break;
      case "turn_end":
        // Tool rounds, stop hooks, queued messages and retry recovery still follow.
        break;
      case "auto_retry_start":
        this.retrying = true;
        this.ended = false;
        this.completedGeneration = null;
        this.activity = "working";
        this.reason = "retry";
        break;
      case "auto_retry_end":
        this.retrying = false;
        // Retry completion is not an agent_end; retain working until explicit end.
        this.reason = "retry-ended";
        break;
      case "auto_compaction_start":
        this.compactionWasSettled = this.settled && event.reason === "idle";
        this.ended = false;
        this.completedGeneration = null;
        this.activity = "working";
        this.reason = "compaction";
        break;
      case "auto_compaction_end":
        if (this.compactionWasSettled && !event.willRetry) {
          this.ended = true;
          this.completedGeneration = this.generation;
          this.activity = "idle";
          this.reason = "idle-compaction-ended";
        } else {
          this.activity = "working";
          this.reason = event.willRetry ? "compaction-continuation" : "compaction-ended";
        }
        this.compactionWasSettled = false;
        break;
      case "tool_approval_requested":
        this.waiting.set(`${event.toolCallId ?? "approval"}:approval`, "approval");
        this.activity = "needs-input";
        this.reason = "approval";
        break;
      case "tool_execution_start":
        if (event.toolName === "ask" && event.hasUI) {
          this.waiting.set(`${event.toolCallId ?? "ask"}:question`, "question");
          this.activity = "needs-input";
          this.reason = "question";
        }
        break;
      case "tool_approval_resolved": case "tool_execution_end":
        this.waiting.delete(event.type === "tool_approval_resolved" ? `${event.toolCallId ?? "approval"}:approval` : `${event.toolCallId ?? "ask"}:question`);
        this.activity = this.waiting.size ? "needs-input" : "working";
        this.reason = this.waiting.values().next().value ?? "input-resolved";
        break;
      case "agent_end":
        if (event.willContinue || this.retrying || this.waiting.size) {
          this.ended = false;
          this.activity = this.waiting.size ? "needs-input" : "working";
          this.reason = this.waiting.values().next().value ?? "continuation";
        } else {
          this.ended = true;
          this.activity = "idle";
          this.completedGeneration = this.generation;
          this.reason = "agent-end";
        }
        break;
      case "session_shutdown":
        this.shutdown = true;
        this.ended = false;
        this.activity = "unknown";
        this.reason = "shutdown";
        this.waiting.clear();
        break;
    }
  }
  snapshot() {
    return { activity: this.activity, reason: this.reason, generation: this.generation, completed_generation: this.completedGeneration, retrying: this.retrying, settled: this.settled, shutdown: this.shutdown };
  }
}
