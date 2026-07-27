#!/usr/bin/env node

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const binary = path.resolve(__dirname, "..", "vendor", "cx");

try {
  fs.accessSync(binary, fs.constants.X_OK);
} catch {
  console.error(
    `cx npm launcher: missing executable at ${binary}; reinstall @contextlimit/cx`,
  );
  process.exit(127);
}

const child = spawn(binary, process.argv.slice(2), {
  env: process.env,
  shell: false,
  stdio: "inherit",
});

const forwardedSignals = ["SIGINT", "SIGTERM", "SIGHUP"];
const handlers = new Map();

for (const signal of forwardedSignals) {
  const handler = () => {
    if (!child.killed) {
      child.kill(signal);
    }
  };
  handlers.set(signal, handler);
  process.on(signal, handler);
}

function removeSignalHandlers() {
  for (const [signal, handler] of handlers) {
    process.removeListener(signal, handler);
  }
}

child.once("error", (error) => {
  removeSignalHandlers();
  console.error(`cx npm launcher: ${error.message}`);
  process.exit(error.code === "ENOENT" ? 127 : 1);
});

child.once("exit", (code, signal) => {
  removeSignalHandlers();
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
