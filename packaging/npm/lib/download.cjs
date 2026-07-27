"use strict";

const fs = require("node:fs/promises");
const http = require("node:http");
const https = require("node:https");
const { createHash } = require("node:crypto");

const USER_AGENT = "@contextlimit/cx npm installer";
const DEFAULT_REDIRECT_LIMIT = 5;
const DEFAULT_TIMEOUT_MS = 30_000;

function validateUrl(value, allowHttp) {
  const url = new URL(value);
  if (url.username || url.password) {
    throw new Error("release URLs must not contain credentials");
  }
  if (url.protocol !== "https:" && !(allowHttp && url.protocol === "http:")) {
    throw new Error(`release URL must use HTTPS: ${url}`);
  }
  return url;
}

function openResponse(
  value,
  {
    allowHttp = false,
    redirectLimit = DEFAULT_REDIRECT_LIMIT,
    timeoutMs = DEFAULT_TIMEOUT_MS,
  } = {},
) {
  return new Promise((resolve, reject) => {
    const requestUrl = validateUrl(value, allowHttp);
    const client = requestUrl.protocol === "https:" ? https : http;
    const request = client.get(
      requestUrl,
      {
        headers: {
          accept: "application/octet-stream, text/plain;q=0.9",
          "user-agent": USER_AGENT,
        },
      },
      (response) => {
        const status = response.statusCode ?? 0;
        if (status >= 300 && status < 400) {
          response.resume();
          const location = response.headers.location;
          if (!location) {
            reject(new Error(`redirect from ${requestUrl} has no location`));
            return;
          }
          if (redirectLimit <= 0) {
            reject(new Error(`too many redirects while downloading ${requestUrl}`));
            return;
          }
          const next = new URL(location, requestUrl).toString();
          openResponse(next, {
            allowHttp,
            redirectLimit: redirectLimit - 1,
            timeoutMs,
          }).then(resolve, reject);
          return;
        }
        if (status < 200 || status >= 300) {
          response.resume();
          reject(new Error(`download failed with HTTP ${status}: ${requestUrl}`));
          return;
        }
        resolve({ response, url: requestUrl.toString() });
      },
    );

    request.setTimeout(timeoutMs, () => {
      request.destroy(new Error(`download timed out after ${timeoutMs} ms: ${requestUrl}`));
    });
    request.once("error", reject);
  });
}

function rejectOversizedContentLength(response, maxBytes, url) {
  const value = response.headers["content-length"];
  if (!value) {
    return;
  }
  const contentLength = Number(value);
  if (Number.isFinite(contentLength) && contentLength > maxBytes) {
    response.resume();
    throw new Error(
      `download exceeds ${maxBytes} byte limit (${contentLength} bytes): ${url}`,
    );
  }
}

async function downloadText(url, options = {}) {
  const maxBytes = options.maxBytes ?? 1024 * 1024;
  const opened = await openResponse(url, options);
  rejectOversizedContentLength(opened.response, maxBytes, opened.url);

  const chunks = [];
  let bytes = 0;
  for await (const chunk of opened.response) {
    bytes += chunk.length;
    if (bytes > maxBytes) {
      opened.response.destroy();
      throw new Error(`download exceeds ${maxBytes} byte limit: ${opened.url}`);
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function downloadFile(url, destination, options = {}) {
  const maxBytes = options.maxBytes ?? 128 * 1024 * 1024;
  const opened = await openResponse(url, options);
  rejectOversizedContentLength(opened.response, maxBytes, opened.url);

  const hash = createHash("sha256");
  const file = await fs.open(destination, "wx", 0o700);
  let bytes = 0;
  try {
    for await (const chunk of opened.response) {
      bytes += chunk.length;
      if (bytes > maxBytes) {
        opened.response.destroy();
        throw new Error(`download exceeds ${maxBytes} byte limit: ${opened.url}`);
      }
      hash.update(chunk);
      await file.write(chunk);
    }
  } finally {
    await file.close();
  }

  return {
    bytes,
    sha256: hash.digest("hex"),
    url: opened.url,
  };
}

module.exports = {
  downloadFile,
  downloadText,
  openResponse,
  validateUrl,
};
