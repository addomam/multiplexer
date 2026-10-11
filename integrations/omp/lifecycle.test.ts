import { describe, expect, test } from "bun:test";
import { Lifecycle } from "./lifecycle";

describe("OMP 18.8.7 explicit lifecycle", () => {
  test("turn_end and session_stop cannot satisfy agent-settled", () => {
    const state = new Lifecycle();
    state.apply({ type: "session_start" });
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_start" });
    state.apply({ type: "turn_end" });
    expect(state.activity).toBe("working");
    expect(state.settled).toBe(false);
    state.apply({ type: "session_stop" });
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_end", willContinue: false });
    expect(state.settled).toBe(true);
    expect(state.completedGeneration).toBe(1);
  });
  test("retry and hidden continuation need a new explicit terminal end", () => {
    const state = new Lifecycle();
    state.apply({ type: "agent_start" });
    state.apply({ type: "auto_retry_start" });
    state.apply({ type: "agent_end", willContinue: true });
    expect(state.settled).toBe(false);
    state.apply({ type: "auto_retry_end" });
    expect(state.activity).toBe("working");
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_start" });
    state.apply({ type: "agent_end", willContinue: true });
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_start" });
    state.apply({ type: "agent_end", willContinue: false });
    expect(state.settled).toBe(true);
    expect(state.completedGeneration).toBe(3);
  });
  test("parallel approvals and ask dialogs retain needs-input until all resolve", () => {
    const state = new Lifecycle();
    state.apply({ type: "agent_start" });
    state.apply({ type: "tool_approval_requested", toolCallId: "approval-a" });
    state.apply({ type: "tool_execution_start", toolName: "ask", toolCallId: "question-b", hasUI: true });
    expect(state.activity).toBe("needs-input");
    state.apply({ type: "tool_approval_resolved", toolCallId: "approval-a" });
    expect(state.activity).toBe("needs-input");
    expect(state.reason).toBe("question");
    state.apply({ type: "tool_execution_end", toolName: "ask", toolCallId: "question-b" });
    expect(state.activity).toBe("working");
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_end" });
    expect(state.settled).toBe(true);
  });
  test("approval of the ask tool does not erase its question wait", () => {
    const state = new Lifecycle();
    state.apply({ type: "agent_start" });
    state.apply({ type: "tool_execution_start", toolName: "ask", toolCallId: "same-tool", hasUI: true });
    state.apply({ type: "tool_approval_requested", toolCallId: "same-tool" });
    state.apply({ type: "tool_approval_resolved", toolCallId: "same-tool" });
    expect(state.activity).toBe("needs-input");
    expect(state.reason).toBe("question");
    state.apply({ type: "tool_execution_end", toolName: "ask", toolCallId: "same-tool" });
    expect(state.activity).toBe("working");
  });
  test("headless ask does not claim interactive waiting", () => {
    const state = new Lifecycle();
    state.apply({ type: "agent_start" });
    state.apply({ type: "tool_execution_start", toolName: "ask", hasUI: false });
    expect(state.activity).toBe("working");
  });
  test("session transition invalidates prior completion and shutdown is unknown", () => {
    for (const type of ["session_switch", "session_branch", "session_tree"]) {
      const state = new Lifecycle();
      state.apply({ type: "agent_start" });
      state.apply({ type: "agent_end" });
      expect(state.settled).toBe(true);
      state.apply({ type });
      expect(state.activity).toBe("idle");
      expect(state.completedGeneration).toBeNull();
      expect(state.settled).toBe(false);
      state.apply({ type: "session_shutdown" });
      expect(state.activity).toBe("unknown");
      expect(state.settled).toBe(false);
    }
  });
  test("nested reducer lifetime never settles the root", () => {
    const root = new Lifecycle();
    const child = new Lifecycle();
    root.apply({ type: "agent_start" });
    child.apply({ type: "agent_start" });
    child.apply({ type: "agent_end" });
    expect(child.settled).toBe(true);
    expect(root.activity).toBe("working");
    expect(root.settled).toBe(false);
    child.apply({ type: "session_shutdown" });
    expect(root.activity).toBe("working");
  });
  test("active compaction continuation stays working; idle maintenance preserves completed generation", () => {
    const state = new Lifecycle();
    state.apply({ type: "agent_start" });
    state.apply({ type: "auto_compaction_start", reason: "overflow" });
    state.apply({ type: "auto_compaction_end", willRetry: true });
    expect(state.activity).toBe("working");
    expect(state.settled).toBe(false);
    state.apply({ type: "agent_end" });
    state.apply({ type: "auto_compaction_start", reason: "idle" });
    expect(state.settled).toBe(false);
    state.apply({ type: "auto_compaction_end", willRetry: false });
    expect(state.settled).toBe(true);
    expect(state.completedGeneration).toBe(1);
  });
});
