import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { execFileSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const testDirectory = path.dirname(fileURLToPath(import.meta.url));
const cloudflareDirectory = path.resolve(testDirectory, "..");
const repoRoot = path.resolve(cloudflareDirectory, "../..");
const builder = path.join(cloudflareDirectory, "build-installer.mjs");
const generated = path.join(
  cloudflareDirectory,
  "generated",
  "installer.mjs",
);

execFileSync(process.execPath, [builder], { stdio: "pipe" });

const workerModule = await import(
  `${pathToFileURL(path.join(cloudflareDirectory, "worker.mjs")).href}?test=${Date.now()}`
);
const worker = workerModule.default;
const installer = await fs.readFile(path.join(repoRoot, "install.sh"), "utf8");

test("serves the exact reviewed installer at canonical and compatibility paths", async () => {
  for (const pathname of ["/", "/install", "/install.sh"]) {
    const response = await worker.fetch(
      new Request(`https://install-cx.asi.sh${pathname}`),
    );
    assert.equal(response.status, 200);
    assert.equal(await response.text(), installer);
    assert.equal(
      response.headers.get("content-type"),
      "text/x-shellscript; charset=utf-8",
    );
    assert.equal(response.headers.get("x-content-type-options"), "nosniff");
  }
});

test("supports HEAD without returning the installer body", async () => {
  const response = await worker.fetch(
    new Request("https://install-cx.asi.sh/", { method: "HEAD" }),
  );
  assert.equal(response.status, 200);
  assert.equal(await response.text(), "");
  assert.equal(
    response.headers.get("content-disposition"),
    'inline; filename="install.sh"',
  );
});

test("rejects unknown paths and mutating methods", async () => {
  const missing = await worker.fetch(
    new Request("https://install-cx.asi.sh/unknown"),
  );
  assert.equal(missing.status, 404);

  const post = await worker.fetch(
    new Request("https://install-cx.asi.sh/", { method: "POST" }),
  );
  assert.equal(post.status, 405);
  assert.equal(post.headers.get("allow"), "GET, HEAD");
});

test("builder output decodes byte-for-byte to the root installer", async () => {
  const generatedUrl = `${pathToFileURL(generated).href}?test=${Date.now()}`;
  const encoded = (await import(generatedUrl)).default;
  assert.equal(Buffer.from(encoded, "base64").toString("utf8"), installer);
});
