"use strict";

const TARGETS = new Map([
  ["darwin:arm64", "darwin-arm64"],
  ["darwin:x64", "darwin-x64"],
  ["linux:arm64", "linux-arm64"],
  ["linux:x64", "linux-x64"],
]);

function releaseTarget(platform = process.platform, arch = process.arch) {
  const key = `${platform}:${arch}`;
  const target = TARGETS.get(key);
  if (!target) {
    throw new Error(
      `CX does not publish a native binary for ${platform}/${arch}; ` +
        "supported platforms are macOS or Linux on arm64 or x64",
    );
  }
  return target;
}

function assetName(version, platform = process.platform, arch = process.arch) {
  return `cx-v${version}-${releaseTarget(platform, arch)}`;
}

function parseChecksums(text) {
  const checksums = new Map();
  for (const rawLine of text.split(/\r?\n/u)) {
    if (!rawLine) {
      continue;
    }
    const match = rawLine.match(/^([0-9a-f]{64}) {2}(\S+)$/u);
    if (!match) {
      throw new Error(`invalid checksum manifest line: ${rawLine}`);
    }
    const [, sha256, name] = match;
    if (checksums.has(name)) {
      throw new Error(`duplicate checksum manifest entry: ${name}`);
    }
    checksums.set(name, sha256);
  }
  return checksums;
}

module.exports = {
  TARGETS,
  assetName,
  parseChecksums,
  releaseTarget,
};
