#!/usr/bin/env node

"use strict";

const fs = require("node:fs/promises");
const path = require("node:path");
const { randomBytes } = require("node:crypto");

const packageJson = require("../package.json");
const { downloadFile, downloadText } = require("../lib/download.cjs");
const { assetName, parseChecksums } = require("../lib/release.cjs");

const DEFAULT_BINARY_LIMIT = 128 * 1024 * 1024;
const DEFAULT_MANIFEST_LIMIT = 1024 * 1024;

async function installBinary({
  version = packageJson.version,
  platform = process.platform,
  arch = process.arch,
  baseUrl = process.env.CX_NPM_RELEASE_BASE_URL ||
    `https://github.com/contextlimit/cx/releases/download/v${version}`,
  vendorDir = path.resolve(__dirname, "..", "vendor"),
  allowHttp = process.env.CX_NPM_ALLOW_HTTP === "1",
  maxBinaryBytes = DEFAULT_BINARY_LIMIT,
  maxManifestBytes = DEFAULT_MANIFEST_LIMIT,
} = {}) {
  const name = assetName(version, platform, arch);
  const normalizedBaseUrl = baseUrl.replace(/\/+$/u, "");
  const manifestUrl = `${normalizedBaseUrl}/checksums.txt`;
  const assetUrl = `${normalizedBaseUrl}/${name}`;

  const manifest = await downloadText(manifestUrl, {
    allowHttp,
    maxBytes: maxManifestBytes,
  });
  const expectedSha256 = parseChecksums(manifest).get(name);
  if (!expectedSha256) {
    throw new Error(`checksums.txt does not contain ${name}`);
  }

  await fs.mkdir(vendorDir, { recursive: true });
  const destination = path.join(vendorDir, "cx");
  const temporary = path.join(
    vendorDir,
    `.cx-${process.pid}-${randomBytes(8).toString("hex")}.tmp`,
  );

  try {
    const downloaded = await downloadFile(assetUrl, temporary, {
      allowHttp,
      maxBytes: maxBinaryBytes,
    });
    if (downloaded.sha256 !== expectedSha256) {
      throw new Error(
        `checksum mismatch for ${name}: expected ${expectedSha256}, ` +
          `received ${downloaded.sha256}`,
      );
    }
    await fs.chmod(temporary, 0o755);
    await fs.rename(temporary, destination);
  } catch (error) {
    await fs.rm(temporary, { force: true });
    throw error;
  }

  return {
    asset: name,
    destination,
    sha256: expectedSha256,
    version,
  };
}

async function main() {
  const installed = await installBinary();
  console.log(`installed ${installed.asset}`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(`@contextlimit/cx install failed: ${error.message}`);
    process.exit(1);
  });
}

module.exports = {
  DEFAULT_BINARY_LIMIT,
  DEFAULT_MANIFEST_LIMIT,
  installBinary,
};
