#!/usr/bin/env node
import dgram from "node:dgram";
import { performance } from "node:perf_hooks";

const defaults = {
  target: "127.0.0.1:15353",
  domain: "example.com",
  count: 1000,
  concurrency: 50,
  timeout: 2000,
  randomPrefix: false
};

function parseArgs(argv) {
  const args = { ...defaults };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const readValue = () => {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`missing value for ${arg}`);
      }
      index += 1;
      return value;
    };
    if (arg === "--target") {
      args.target = readValue();
    } else if (arg === "--domain") {
      args.domain = readValue();
    } else if (arg === "--count") {
      args.count = parsePositiveInteger(readValue(), "count");
    } else if (arg === "--concurrency") {
      args.concurrency = parsePositiveInteger(readValue(), "concurrency");
      if (args.concurrency > 65535) {
        throw new Error("concurrency must be 65535 or lower");
      }
    } else if (arg === "--timeout") {
      args.timeout = parsePositiveInteger(readValue(), "timeout");
    } else if (arg === "--random-prefix") {
      args.randomPrefix = true;
    } else if (arg === "--help" || arg === "-h") {
      printHelp();
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

function parsePositiveInteger(value, name) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive integer`);
  }
  return parsed;
}

function parseTarget(target) {
  const splitAt = target.lastIndexOf(":");
  if (splitAt <= 0 || splitAt === target.length - 1) {
    throw new Error("target must use host:port format");
  }
  return {
    host: target.slice(0, splitAt),
    port: parsePositiveInteger(target.slice(splitAt + 1), "port")
  };
}

function domainForRequest(options, sequence) {
  if (!options.randomPrefix) {
    return options.domain;
  }
  return `${sequence.toString(36)}-${Math.random().toString(36).slice(2, 10)}.${options.domain}`;
}

function buildQuery(id, domain) {
  const labels = domain === "." ? [] : domain.replace(/\.$/, "").split(".");
  const questionLength = labels.reduce((total, label) => total + 1 + Buffer.byteLength(label), 1);
  const packet = Buffer.alloc(12 + questionLength + 4);
  packet.writeUInt16BE(id, 0);
  packet.writeUInt16BE(0x0100, 2);
  packet.writeUInt16BE(1, 4);
  let offset = 12;
  for (const label of labels) {
    const length = Buffer.byteLength(label);
    if (length === 0 || length > 63) {
      throw new Error(`invalid DNS label: ${label}`);
    }
    packet[offset] = length;
    offset += 1;
    packet.write(label, offset, "ascii");
    offset += length;
  }
  packet[offset] = 0;
  offset += 1;
  packet.writeUInt16BE(1, offset);
  offset += 2;
  packet.writeUInt16BE(1, offset);
  return packet;
}

async function runBench(options) {
  const target = parseTarget(options.target);
  const socket = dgram.createSocket(target.host.includes(":") ? "udp6" : "udp4");
  const latencies = [];
  const pending = new Map();
  let sent = 0;
  let completed = 0;
  let succeeded = 0;
  let failed = 0;
  let nextId = Math.floor(Math.random() * 0xffff);
  const startedAt = performance.now();

  await new Promise((resolve) => {
    socket.once("listening", resolve);
    socket.bind(0);
  });

  return new Promise((resolve, reject) => {
    socket.on("error", reject);
    function finish() {
      socket.close();
      const elapsedMs = performance.now() - startedAt;
      resolve({ elapsedMs, succeeded, failed, latencies });
    }

    function failRequest(id) {
      if (!pending.has(id)) {
        return;
      }
      pending.delete(id);
      completed += 1;
      failed += 1;
      schedule();
    }

    function sendOne() {
      let id = nextId;
      while (pending.has(id)) {
        id = (id + 1) & 0xffff;
      }
      nextId = (id + 1) & 0xffff;
      const packet = buildQuery(id, domainForRequest(options, sent));
      sent += 1;
      const timer = setTimeout(() => failRequest(id), options.timeout);
      pending.set(id, { startedAt: performance.now(), timer });
      socket.send(packet, target.port, target.host, (err) => {
        if (err) {
          clearTimeout(timer);
          failRequest(id);
        }
      });
    }

    function schedule() {
      while (sent < options.count && pending.size < options.concurrency) {
        sendOne();
      }
      if (completed >= options.count) {
        finish();
      }
    }

    socket.on("message", (message) => {
      if (message.length < 2) {
        return;
      }
      const id = message.readUInt16BE(0);
      const request = pending.get(id);
      if (!request) {
        return;
      }
      pending.delete(id);
      clearTimeout(request.timer);
      completed += 1;
      succeeded += 1;
      latencies.push(performance.now() - request.startedAt);
      schedule();
    });

    schedule();
  });
}

function percentile(values, p) {
  if (!values.length) {
    return 0;
  }
  const index = Math.min(values.length - 1, Math.ceil((p / 100) * values.length) - 1);
  return values[index];
}

function printHelp() {
  console.log(`Usage: node scripts/dns-bench.mjs [options]

Options:
  --target <host:port>     DNS server address (default: ${defaults.target})
  --domain <domain>        Query domain (default: ${defaults.domain})
  --count <number>         Total queries (default: ${defaults.count})
  --concurrency <number>   In-flight queries (default: ${defaults.concurrency})
  --timeout <ms>           Per-query timeout (default: ${defaults.timeout})
  --random-prefix          Query a unique random subdomain each time
`);
}

function printResult(options, result) {
  const sorted = [...result.latencies].sort((a, b) => a - b);
  const total = result.succeeded + result.failed;
  const average = sorted.length
    ? sorted.reduce((sum, item) => sum + item, 0) / sorted.length
    : 0;
  const qps = total / (result.elapsedMs / 1000);
  console.log(`target: ${options.target}`);
  console.log(`domain: ${options.domain}`);
  console.log(`random_prefix: ${options.randomPrefix ? "yes" : "no"}`);
  console.log(`total: ${total}`);
  console.log(`success: ${result.succeeded}`);
  console.log(`failed: ${result.failed}`);
  console.log(`elapsed_ms: ${result.elapsedMs.toFixed(1)}`);
  console.log(`qps: ${qps.toFixed(1)}`);
  console.log(`avg_ms: ${average.toFixed(2)}`);
  console.log(`p50_ms: ${percentile(sorted, 50).toFixed(2)}`);
  console.log(`p95_ms: ${percentile(sorted, 95).toFixed(2)}`);
  console.log(`p99_ms: ${percentile(sorted, 99).toFixed(2)}`);
}

try {
  const options = parseArgs(process.argv.slice(2));
  const result = await runBench(options);
  printResult(options, result);
  process.exit(result.failed > 0 ? 1 : 0);
} catch (err) {
  console.error(err instanceof Error ? err.message : String(err));
  process.exit(1);
}
