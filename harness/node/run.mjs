// The browser-engine run of the conformance corpus (Kernel ABI — Sanki
// §Conformance of a build, item 3): the module under V8 — Chrome's WebAssembly
// engine, here through Node — driven through the ABI's arena protocol, with
// the same vector mapping as the `wasmi` run (`tests/common/mod.rs`; *Sanki
// conformance vectors* §Through the module). Every response's digest is
// written under `target/conformance/<category>.node.json`; when the `wasmi`
// records are present, the two are compared and any difference fails the run.
//
//   node harness/node/run.mjs [path/to/module.wasm]

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const modulePath =
  process.argv[2] ??
  process.env.SANKI_MODULE ??
  join(root, "target/wasm32-unknown-unknown/release/sashite_sanki_kernel_wasm.wasm");
const outDir = join(root, "target/conformance");
mkdirSync(outDir, { recursive: true });

const REQUEST_BOUND = 33_554_432;

// ---- the runtime ------------------------------------------------------------

const bytes = readFileSync(modulePath);
const compiled = await WebAssembly.compile(bytes);
if (WebAssembly.Module.imports(compiled).length !== 0) {
  throw new Error("the import section is not empty");
}
const exportNames = WebAssembly.Module.exports(compiled).map((e) => e.name);
for (const name of ["memory", "abi", "alloc", "call"]) {
  if (!exportNames.includes(name)) throw new Error(`missing export ${name}`);
}
const instance = await WebAssembly.instantiate(compiled, {});
const { memory, abi, alloc, call } = instance.exports;
if (memory.buffer.byteLength !== 2048 * 65536) {
  throw new Error(`memory is ${memory.buffer.byteLength} bytes, not 128 MiB`);
}
let grew = true;
try {
  memory.grow(1);
} catch {
  grew = false;
}
if (grew) throw new Error("the memory grew: its maximum is not 2048 pages");

function buffer(address) {
  const view = new DataView(memory.buffer);
  const len = view.getUint32(address, true);
  return new Uint8Array(memory.buffer, address + 4, len).slice();
}

const abiText = new TextDecoder().decode(buffer(abi()));
if (abiText !== "sashite.sanki.kernel-abi/1") throw new Error(`abi() says ${abiText}`);

const encoder = new TextEncoder();

function request(requestBytes) {
  if (requestBytes.length > REQUEST_BOUND) return null;
  const address = alloc(requestBytes.length);
  if (address === 0) return null;
  new Uint8Array(memory.buffer, address, requestBytes.length).set(requestBytes);
  const response = call(address, requestBytes.length);
  if (response === 0) return null;
  return buffer(response);
}

function ask(value) {
  const response = request(encoder.encode(JSON.stringify(value)));
  if (response === null) throw new Error("no answer");
  return response;
}

function digest(u8) {
  return createHash("sha256").update(u8).digest("hex");
}

// ---- the corpus mapping (identical to tests/common/mod.rs) ------------------

function hexId(id) {
  const out = new Uint8Array(32);
  const raw = encoder.encode(id).slice(0, 32);
  out.set(raw);
  return Array.from(out, (b) => b.toString(16).padStart(2, "0")).join("");
}
const seatPubkey = (seat) => (seat === "first" ? "0a" : "14").repeat(32);
const timestamperPubkey = "63".repeat(32);
const sessionId = hexId("session");
const content = (mv) => JSON.stringify(mv);
const timeControl = (vector) => vector.timeControl ?? [[3600, null, null]];

function sessionTerms(vector, attested) {
  return {
    id: sessionId,
    first: seatPubkey("first"),
    second: seatPubkey("second"),
    timestamper: attested ? timestamperPubkey : null,
    time_control: timeControl(vector),
    position: vector.position,
    start: vector.t0,
  };
}

function events(vector, attested) {
  const plies = [];
  const attestations = [];
  for (const ply of vector.plies) {
    plies.push({
      id: hexId(ply.id),
      signer: seatPubkey(ply.seat),
      session: sessionId,
      step: ply.step,
      draw: ply.draw ?? false,
      content: content(ply.move),
      created_at: ply.timedAt,
    });
    if (attested) {
      attestations.push({
        id: hexId(`att-${ply.id}`),
        signer: timestamperPubkey,
        attests: hexId(ply.id),
        created_at: ply.timedAt,
      });
    }
  }
  return { plies, attestations };
}

const corpus = (name) => JSON.parse(readFileSync(join(root, "tests/conformance", name), "utf8"));

// ---- the run ----------------------------------------------------------------

const records = {};
const record = (category, id, response) => {
  (records[category] ??= {})[id] = digest(response);
};
let failures = 0;
const check = (ok, message) => {
  if (!ok) {
    failures += 1;
    console.error(`FAIL ${message}`);
  }
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

for (const vector of corpus("legality.json").vectors) {
  const response = ask({ op: "apply", position: vector.position, move: content(vector.move) });
  record("legality", vector.id, response);
  const parsed = JSON.parse(new TextDecoder().decode(response));
  if (vector.legal) {
    check(parsed.position === vector.result, `${vector.id}: position`);
    check(parsed.status === vector.status, `${vector.id}: status`);
  } else {
    check(parsed.error?.code === "illegal", `${vector.id}: expected illegal`);
  }
}

for (const vector of corpus("selection.json").vectors) {
  const candidates = vector.candidates.map((c) => ({ id: c.id, created_at: c.createdAt, legal: c.legal }));
  const response = ask({ op: "select", boundary: vector.boundary, cap: vector.cap, candidates });
  record("selection", vector.id, response);
  const parsed = JSON.parse(new TextDecoder().decode(response));
  check(parsed.result === vector.expected.result && same(parsed.selected, vector.expected.selected), vector.id);
}

for (const vector of corpus("time.json").vectors) {
  if (vector.timings !== undefined) {
    const response = ask({ op: "charges", start: vector.t0, timings: vector.timings });
    record("time", vector.id, response);
    const parsed = JSON.parse(new TextDecoder().decode(response));
    check(
      same(parsed.first, vector.expected.first) &&
        same(parsed.second, vector.expected.second) &&
        parsed.anchor === vector.expected.anchor &&
        parsed.on_move === vector.expected.onMove,
      vector.id,
    );
  } else {
    const response = ask({ op: "elapsed", anchor: vector.anchor, timing: vector.timing });
    record("time", vector.id, response);
    const parsed = JSON.parse(new TextDecoder().decode(response));
    check(parsed.elapsed === vector.expectedElapsed, vector.id);
  }
}

for (const vector of corpus("clock.json").vectors) {
  const response = ask({
    op: "clock",
    time_control: vector.timeControl,
    clock: {
      period: vector.clock.period,
      plies_in_period: vector.clock.pliesInPeriod,
      remaining: vector.clock.remaining,
    },
    elapsed: vector.elapsed,
  });
  record("clock", vector.id, response);
  const parsed = JSON.parse(new TextDecoder().decode(response));
  check(parsed.kind === vector.expected.kind, `${vector.id}: kind`);
  if (vector.expected.kind === "continued") {
    const after = vector.expected.clock;
    check(
      parsed.clock.remaining === after.remaining &&
        parsed.clock.period === after.period &&
        parsed.clock.plies_in_period === after.pliesInPeriod,
      `${vector.id}: clock`,
    );
  }
}

function unhex(hex) {
  const raw = [];
  for (let i = 0; i < 64; i += 2) {
    const b = parseInt(hex.slice(i, i + 2), 16);
    if (b === 0) break;
    raw.push(b);
  }
  return new TextDecoder().decode(new Uint8Array(raw));
}

for (const vector of corpus("scenarios.json").vectors) {
  for (const attested of [false, true]) {
    const mode = attested ? "attested" : "self-timed";
    const { plies, attestations } = events(vector, attested);
    const response = ask({
      op: "natural_state",
      session: sessionTerms(vector, attested),
      plies,
      attestations,
      cutoff: vector.cutoff,
    });
    record("scenarios", `${vector.id}#${mode}#natural_state`, response);
    const parsed = JSON.parse(new TextDecoder().decode(response));
    const chain = (parsed.chain ?? []).map((entry) => unhex(entry.id));
    check(same(chain, vector.expectedChain), `${vector.id} (${mode}): chain`);
    if (vector.expectedTermination === null) {
      check(parsed.end?.ongoing !== undefined, `${vector.id} (${mode}): ongoing`);
    } else {
      check(
        parsed.end?.terminal?.verdict?.status === vector.expectedTermination.status,
        `${vector.id} (${mode}): termination`,
      );
    }
    if (vector.invoker !== undefined) {
      const again = events(vector, attested);
      const verdict = ask({
        op: "verdict_at",
        session: sessionTerms(vector, attested),
        plies: again.plies,
        attestations: again.attestations,
        invoker: vector.invoker,
        cutoff: vector.cutoff,
      });
      record("scenarios", `${vector.id}#${mode}#verdict_at`, verdict);
      const parsedVerdict = JSON.parse(new TextDecoder().decode(verdict));
      const expected = vector.expectedVerdict;
      check(
        parsedVerdict.verdict?.status === expected.status &&
          parsedVerdict.verdict?.result?.first === expected.result.first &&
          parsedVerdict.verdict?.result?.second === expected.result.second,
        `${vector.id} (${mode}): verdict`,
      );
    }
  }
}

record("rule-system", "describe", ask({ op: "describe" }));
for (const variant of ["chess", "ogi", "xiongqi"]) {
  record("rule-system", `ggn/${variant}`, ask({ op: "ggn", variant }));
}

// The extreme session (§Conformance of a build, item 5), when the wasmi run
// left its request behind.
const extremePath = join(outDir, "extreme.request.json");
if (existsSync(extremePath)) {
  const extreme = readFileSync(extremePath);
  const started = performance.now();
  const response = request(new Uint8Array(extreme));
  const elapsed = performance.now() - started;
  check(response !== null, "the extreme session answers");
  if (response !== null) {
    record("extreme", "extreme", response);
    const parsed = JSON.parse(new TextDecoder().decode(response));
    check(parsed.verdict !== undefined, "the extreme session yields a verdict");
    console.log(`extreme session: ${extreme.length} bytes answered in ${elapsed.toFixed(0)} ms under V8`);
  }
}

// ---- the records and the comparison ------------------------------------------

let compared = 0;
let differing = 0;
for (const [category, entries] of Object.entries(records)) {
  writeFileSync(join(outDir, `${category}.node.json`), JSON.stringify(entries, null, 2) + "\n");
  const other = join(outDir, `${category}.wasmi.json`);
  if (!existsSync(other)) continue;
  const wasmi = JSON.parse(readFileSync(other, "utf8"));
  const ids = new Set([...Object.keys(entries), ...Object.keys(wasmi)]);
  for (const id of ids) {
    compared += 1;
    if (entries[id] !== wasmi[id]) {
      differing += 1;
      console.error(`DIFF ${category}/${id}: node ${entries[id]} vs wasmi ${wasmi[id]}`);
    }
  }
}

const total = Object.values(records).reduce((n, e) => n + Object.keys(e).length, 0);
console.log(
  `node: ${total} responses recorded, ${failures} expectation failures; ` +
    (compared > 0 ? `${compared} compared with wasmi, ${differing} differing` : "no wasmi records to compare"),
);
if (failures > 0 || differing > 0) process.exit(1);
