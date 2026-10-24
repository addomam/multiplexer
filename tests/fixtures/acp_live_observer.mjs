// Opt-in live acceptance only. Forward ACP bytes unchanged; retain safe model,
// process and request-count evidence, never prompts, answers, credentials or logs.
// Requires Node only for this explicitly configured ignored live test.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { Transform } from 'node:stream';

const MAX_FRAME = 1024 * 1024;
let child;
function fail() {
  process.stderr.write('amux live observer: private configuration or transport failure\n');
  child?.kill('SIGTERM');
  process.exitCode = 1;
  process.stdin.destroy();
}
function privateFile(file) {
  if (!path.isAbsolute(file)) throw new Error('absolute private file required');
  const st = fs.lstatSync(file);
  if (!st.isFile() || (st.mode & 0o777) !== 0o600 || st.uid !== process.getuid()) {
    throw new Error('private file required');
  }
}
function atomicJson(file, value) {
  const temporary = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(value), { mode: 0o600, flag: 'wx' });
  fs.renameSync(temporary, file);
}
try {
  const configPath = process.argv[2];
  privateFile(configPath);
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  if (!['codex', 'grok'].includes(config.id) || !config.command?.length ||
      !path.isAbsolute(config.command[0]) || !path.isAbsolute(config.cwd) ||
      !path.isAbsolute(config.evidence) || fs.existsSync(config.evidence)) {
    throw new Error('fresh pinned launcher required');
  }
  privateFile(config.budget);
  const env = config.environment;
  if (!env || ['OPENAI_API_KEY', 'CODEX_API_KEY', 'XAI_API_KEY', 'ANTHROPIC_API_KEY'].some(k => k in env)) {
    throw new Error('subscription environment required');
  }
  // One launcher owns a budget at a time. A stale lock fails closed; the
  // operator removes the private root only after observing owned process exit.
  const budgetLock = fs.openSync(`${config.budget}.lock`, 'wx', 0o600);
  const requests = new Map();
  const counts = { prompt_requests: 0, form_requests: 0, permission_requests: 0, cancellations: 0, local_commands: 0 };
  let modelId;
  let sessionId;
  let reasoningEffort;
  let questionShape;
  const stopReasons = [];
  const observeModel = value => {
    if (typeof value !== 'string') return;
    const profile = config.id === 'codex' && value.match(/^(.+)\[(minimal|low|medium|high|xhigh)\]$/);
    modelId = profile ? profile[1] : value;
    if (profile) reasoningEffort = profile[2];
  };
  const save = () => {
    if (sessionId && modelId) atomicJson(config.evidence, {
      session_id: sessionId, model_id: modelId, pid: child.pid,
      reasoning_effort: reasoningEffort ?? null, counts,
      prompt_stop_reasons: stopReasons,
      ...(questionShape ? { question_shape: questionShape } : {}),
    });
  };
  const input = message => {
    if (message.method === 'session/prompt') {
      const budget = JSON.parse(fs.readFileSync(config.budget, 'utf8'));
      if (!Number.isSafeInteger(budget.used) || !Number.isSafeInteger(budget.limit) ||
          budget.limit < 1 || budget.limit > 12 || budget.used >= budget.limit) {
        throw new Error('live prompt ceiling');
      }
      if (modelId !== config.expected_model ||
          (config.expected_effort && reasoningEffort !== config.expected_effort)) {
        throw new Error('unexpected model or effort; refusing inference');
      }
      atomicJson(config.budget, { used: budget.used + 1, limit: budget.limit });
      counts.prompt_requests++;
      if (config.id === 'codex' && message.params?.prompt?.length === 1 &&
          message.params.prompt[0].type === 'text' && message.params.prompt[0].text === '/plan') {
        counts.local_commands++;
      }
      save();
    }
    if (message.method === 'session/cancel') { counts.cancellations++; save(); }
    if (message.id !== undefined && message.method) {
      if (requests.size >= 128) throw new Error('outstanding request ceiling');
      requests.set(JSON.stringify(message.id), message.method);
    }
  };
  const output = message => {
    if (message.method === 'elicitation/create' || message.method === '_x.ai/ask_user_question') counts.form_requests++;
    if (message.method === '_x.ai/ask_user_question') {
      const shape = value => Object.fromEntries(Object.entries(value ?? {}).slice(0, 64).map(([key, val]) =>
        [/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(key) ? key : '<non-field-key>', Array.isArray(val) ? 'array' : val === null ? 'null' : typeof val]));
      const params = message.params ?? {};
      questionShape = { params: shape(params), question: shape(params.questions?.[0]),
        option: shape(params.questions?.[0]?.options?.[0]),
        mode: ['default', 'plan'].includes(params.mode) ? params.mode : '<other-or-absent>' };
    }
    if (message.method === 'session/request_permission') counts.permission_requests++;
    if (message.id !== undefined && !message.method) {
      const method = requests.get(JSON.stringify(message.id));
      requests.delete(JSON.stringify(message.id));
      if (method === 'session/prompt' && message.result) {
        if (stopReasons.length >= 8) throw new Error('prompt result ceiling');
        const reason = message.result.stopReason;
        stopReasons.push(['end_turn', 'refusal', 'cancelled', 'max_tokens', 'max_turn_requests'].includes(reason) ? reason : '<unrecognized>');
      }
      if (method === 'session/new' && message.result) {
        sessionId = message.result.sessionId;
        observeModel(message.result.models?.currentModelId);
        for (const option of message.result.configOptions ?? []) {
          if (!modelId && option.category === 'model') observeModel(option.currentValue);
          if (option.id === 'reasoning_effort') reasoningEffort = option.currentValue;
        }
      }
    }
    if (message.method === 'session/update') {
      const update = message.params?.update;
      if (update?.sessionUpdate === 'config_option_update') {
        for (const option of update.configOptions ?? []) {
          if (option.category === 'model') observeModel(option.currentValue);
          if (option.id === 'reasoning_effort') reasoningEffort = option.currentValue;
        }
      }
      if (update?.sessionUpdate === 'current_model_update') observeModel(update.currentModelId);
    }
    save();
  };
  // The observer's parser is bounded independently of the broker. The original
  // bytes, not parsed/reserialized frames, reach both endpoints with backpressure.
  function inspectFrames(observe) {
    let pending = Buffer.alloc(0);
    return new Transform({
      transform(chunk, _, callback) {
        try {
          pending = Buffer.concat([pending, chunk]);
          let end;
          while ((end = pending.indexOf(10)) !== -1) {
            if (end > MAX_FRAME) throw new Error('frame ceiling');
            observe(JSON.parse(pending.subarray(0, end).toString('utf8')));
            pending = pending.subarray(end + 1);
          }
          if (pending.length > MAX_FRAME) throw new Error('frame ceiling');
          callback(null, chunk);
        } catch { callback(new Error('bounded ACP observation failed')); }
      },
      flush(callback) {
        callback(pending.length ? new Error('incomplete ACP frame') : null);
      },
    });
  }
  child = spawn(config.command[0], config.command.slice(1), {
    cwd: config.cwd, env, stdio: ['pipe', 'pipe', 'pipe'],
  });
  const incoming = inspectFrames(input);
  const outgoing = inspectFrames(output);
  for (const stream of [incoming, outgoing, child.stdin, child.stdout, child.stderr]) stream.on('error', fail);
  child.on('error', fail);
  process.on('SIGTERM', () => { child.kill('SIGTERM'); child.stdin.end(); });
  process.stdin.pipe(incoming).pipe(child.stdin);
  child.stdout.pipe(outgoing).pipe(process.stdout);
  child.stderr.pipe(process.stderr);
  child.on('close', (code, signal) => {
    save();
    fs.closeSync(budgetLock);
    fs.unlinkSync(`${config.budget}.lock`);
    process.exitCode = process.exitCode || code || (signal ? 1 : 0);
    process.stdin.destroy();
  });
} catch { fail(); }
