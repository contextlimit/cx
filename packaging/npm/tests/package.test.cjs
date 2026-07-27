"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const fsp = require("node:fs/promises");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const { createHash } = require("node:crypto");
const { spawnSync } = require("node:child_process");
const { once } = require("node:events");
const test = require("node:test");

const packageRoot = path.resolve(__dirname, "..");
const launcher = path.join(packageRoot, "bin", "cx.js");
const { installBinary } = require("../scripts/install.cjs");
const {
  assetName,
  parseChecksums,
  releaseTarget,
} = require("../lib/release.cjs");

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function temporaryDirectory() {
  return fsp.mkdtemp(path.join(os.tmpdir(), "cx-npm-test-"));
}

async function startFixtureServer(routes) {
  const server = http.createServer((request, response) => {
    const route = routes.get(request.url);
    if (!route) {
      response.writeHead(404);
      response.end("not found");
      return;
    }
    if (route.redirect) {
      response.writeHead(302, { location: route.redirect });
      response.end();
      return;
    }
    response.writeHead(route.status ?? 200, {
      "content-length": Buffer.byteLength(route.body),
      "content-type": route.contentType ?? "application/octet-stream",
    });
    response.end(route.body);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
    }),
  };
}

test("release target mapping is explicit and rejects unsupported systems", () => {
  assert.equal(releaseTarget("darwin", "arm64"), "darwin-arm64");
  assert.equal(releaseTarget("darwin", "x64"), "darwin-x64");
  assert.equal(releaseTarget("linux", "arm64"), "linux-arm64");
  assert.equal(releaseTarget("linux", "x64"), "linux-x64");
  assert.throws(() => releaseTarget("win32", "x64"), /does not publish/u);
  assert.throws(() => releaseTarget("linux", "ppc64"), /does not publish/u);
});

test("checksum parser requires exact unique release rows", () => {
  const hash = "a".repeat(64);
  assert.equal(parseChecksums(`${hash}  cx-v0.1.0-linux-x64\n`).size, 1);
  assert.throws(
    () => parseChecksums(`${hash} cx-v0.1.0-linux-x64\n`),
    /invalid checksum/u,
  );
  assert.throws(
    () => parseChecksums(
      `${hash}  cx-v0.1.0-linux-x64\n${hash}  cx-v0.1.0-linux-x64\n`,
    ),
    /duplicate checksum/u,
  );
});

test("installer follows bounded HTTP redirects and verifies the binary", async (t) => {
  const version = "0.1.0";
  const asset = assetName(version);
  const binary = Buffer.from("#!/bin/sh\nprintf 'fixture cx\\n'\n", "utf8");
  const routes = new Map([
    ["/checksums.txt", { redirect: "/manifest" }],
    ["/manifest", {
      body: `${sha256(binary)}  ${asset}\n`,
      contentType: "text/plain",
    }],
    [`/${asset}`, { redirect: "/binary" }],
    ["/binary", { body: binary }],
  ]);
  const server = await startFixtureServer(routes);
  t.after(server.close);
  const root = await temporaryDirectory();
  t.after(() => fsp.rm(root, { force: true, recursive: true }));
  const vendorDir = path.join(root, "vendor");

  const installed = await installBinary({
    allowHttp: true,
    baseUrl: server.baseUrl,
    vendorDir,
    version,
  });

  assert.equal(installed.asset, asset);
  assert.deepEqual(await fsp.readFile(installed.destination), binary);
  assert.equal((await fsp.stat(installed.destination)).mode & 0o777, 0o755);
  assert.deepEqual(
    (await fsp.readdir(vendorDir)).sort(),
    ["cx"],
    "temporary files must not remain after an atomic install",
  );
});

test("installer removes partial files after checksum mismatch", async (t) => {
  const version = "0.1.0";
  const asset = assetName(version);
  const routes = new Map([
    ["/checksums.txt", {
      body: `${"0".repeat(64)}  ${asset}\n`,
      contentType: "text/plain",
    }],
    [`/${asset}`, { body: Buffer.from("wrong bytes") }],
  ]);
  const server = await startFixtureServer(routes);
  t.after(server.close);
  const root = await temporaryDirectory();
  t.after(() => fsp.rm(root, { force: true, recursive: true }));
  const vendorDir = path.join(root, "vendor");

  await assert.rejects(
    installBinary({
      allowHttp: true,
      baseUrl: server.baseUrl,
      vendorDir,
      version,
    }),
    /checksum mismatch/u,
  );
  assert.deepEqual(await fsp.readdir(vendorDir), []);
});

test("installer enforces the configured binary size limit", async (t) => {
  const version = "0.1.0";
  const asset = assetName(version);
  const binary = Buffer.alloc(256, 7);
  const routes = new Map([
    ["/checksums.txt", {
      body: `${sha256(binary)}  ${asset}\n`,
      contentType: "text/plain",
    }],
    [`/${asset}`, { body: binary }],
  ]);
  const server = await startFixtureServer(routes);
  t.after(server.close);
  const root = await temporaryDirectory();
  t.after(() => fsp.rm(root, { force: true, recursive: true }));
  const vendorDir = path.join(root, "vendor");

  await assert.rejects(
    installBinary({
      allowHttp: true,
      baseUrl: server.baseUrl,
      maxBinaryBytes: 128,
      vendorDir,
      version,
    }),
    /exceeds 128 byte limit/u,
  );
  assert.deepEqual(await fsp.readdir(vendorDir), []);
});

test("launcher preserves argv, stdout, stderr, and exit code", async (t) => {
  if (process.platform === "win32") {
    t.skip("CX does not publish a Windows package");
  }
  const root = await temporaryDirectory();
  t.after(() => fsp.rm(root, { force: true, recursive: true }));
  await fsp.mkdir(path.join(root, "bin"), { recursive: true });
  await fsp.mkdir(path.join(root, "vendor"), { recursive: true });
  await fsp.copyFile(launcher, path.join(root, "bin", "cx.js"));
  const fakeBinary = path.join(root, "vendor", "cx");
  await fsp.writeFile(
    fakeBinary,
    "#!/bin/sh\nprintf 'stdout:%s|%s\\n' \"$1\" \"$2\"\nprintf 'stderr:%s\\n' \"$1\" >&2\nexit 7\n",
  );
  await fsp.chmod(fakeBinary, 0o755);

  const result = spawnSync(
    process.execPath,
    [path.join(root, "bin", "cx.js"), "alpha", "two words"],
    { encoding: "utf8" },
  );

  assert.equal(result.status, 7);
  assert.equal(result.stdout, "stdout:alpha|two words\n");
  assert.equal(result.stderr, "stderr:alpha\n");
});

test("npm package metadata remains public and dependency free", () => {
  const metadata = JSON.parse(fs.readFileSync(
    path.join(packageRoot, "package.json"),
    "utf8",
  ));
  assert.equal(metadata.name, "@contextlimit/cx");
  assert.equal(metadata.publishConfig.access, "public");
  assert.deepEqual(metadata.dependencies, undefined);
  assert.equal(metadata.bin.cx, "bin/cx.js");
});
