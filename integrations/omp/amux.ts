import type { ExtensionAPI, ExtensionContext } from "@oh-my-pi/pi-coding-agent";
import { createConnection } from "node:net";
import { randomUUID } from "node:crypto";
import { Lifecycle, type LifecycleEvent } from "./lifecycle";

/** Pinned to OMP 18.8.7. Reports only supported lifecycle events, never terminal output. */
export default function amux(pi: ExtensionAPI) {
  const socket = process.env.AMUX_SOCKET;
  const terminal = process.env.AMUX_TERMINAL;
  if (!socket || !terminal) return;
  const agents = new Map<string, { instance: string; lifecycle: Lifecycle; context: ExtensionContext }>();
  let reports: Promise<void> = Promise.resolve();
  let heartbeat: NodeJS.Timeout | undefined;

  const report = (ctx: ExtensionContext, lifecycle: Lifecycle, instance: string, lifecycleEvent = "heartbeat"): Promise<void> => {
    // These supported observations are not exposed by every pinned readonly
    // context type. Never force a save or read transcript entries to infer one.
    const sessions = ctx.sessionManager as ExtensionContext["sessionManager"] & {
      isSessionConfirmedOnDisk?: () => boolean;
      isSessionOnDisk?: () => boolean;
    };
    const persisted = sessions.isSessionConfirmedOnDisk?.() ?? sessions.isSessionOnDisk?.() ?? null;
    const request = JSON.stringify({ version: 1, op: "report", terminal, event: {
      kind: "agent", instance_id: `${instance}:${ctx.agent.id}`,
      agent_id: ctx.agent.id, agent_kind: ctx.agent.kind, parent_id: ctx.agent.parentId ?? null,
      agent_family: "omp", conversation_id: sessions.getSessionId(),
      session_file: sessions.getSessionFile() ?? null, cwd: ctx.cwd,
      session_persisted: persisted, pid: process.pid,
      lifecycle_event: lifecycleEvent,
      omp_version: "18.8.7", ...lifecycle.snapshot(),
    } }) + "\n";
    reports = reports.then(() => new Promise<void>((resolve) => {
      const connection = createConnection({ path: socket });
      let received = "";
      const finish = () => { connection.destroy(); resolve(); };
      connection.setTimeout(1_000, finish);
      connection.on("error", finish);
      connection.on("end", finish);
      connection.on("connect", () => connection.write(request));
      connection.on("data", data => {
        received += data.toString();
        if (received.includes("\n") || received.length > 65_536) finish();
      });
    }));
    return reports;
  };
  const observe = async (event: LifecycleEvent, ctx: ExtensionContext) => {
    let agent = agents.get(ctx.agent.id);
    if (!agent) {
      // A registration is one root/agent lifetime, not one conversation. Keep
      // its identity across session switches, but never reuse it after shutdown
      // even if OMP reuses an agent ID in this process or the OS reuses its PID.
      agent = { instance: randomUUID(), lifecycle: new Lifecycle(), context: ctx };
      agents.set(ctx.agent.id, agent);
    }
    agent.context = ctx;
    agent.lifecycle.apply({ ...event, hasUI: ctx.hasUI });
    const pending = report(ctx, agent.lifecycle, agent.instance, event.type);
    if (!heartbeat && !agent.lifecycle.shutdown) {
      heartbeat = setInterval(() => {
        for (const current of agents.values()) {
          if (!current.lifecycle.shutdown) void report(current.context, current.lifecycle, current.instance);
        }
      }, 2_000);
      heartbeat.unref?.();
    }
    if (agent.lifecycle.shutdown) agents.delete(ctx.agent.id);
    if (agents.size === 0) {
      clearInterval(heartbeat);
      heartbeat = undefined;
    }
    await pending;
  };
  pi.on("session_start", observe);
  pi.on("session_switch", observe);
  pi.on("session_branch", observe);
  pi.on("session_tree", observe);
  pi.on("session_shutdown", observe);
  pi.on("agent_start", observe);
  pi.on("agent_end", observe);
  pi.on("turn_start", observe);
  pi.on("turn_end", observe);
  pi.on("session_stop", observe);
  pi.on("auto_retry_start", observe);
  pi.on("auto_retry_end", observe);
  pi.on("auto_compaction_start", observe);
  pi.on("auto_compaction_end", observe);
  pi.on("tool_approval_requested", observe);
  pi.on("tool_approval_resolved", observe);
  pi.on("tool_execution_start", observe);
  pi.on("tool_execution_end", observe);
}
