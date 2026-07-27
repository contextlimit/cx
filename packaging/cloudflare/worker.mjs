import installerBase64 from "./generated/installer.mjs";

const INSTALL_PATHS = new Set(["/", "/install", "/install.sh"]);
const INSTALL_HEADERS = Object.freeze({
  "cache-control": "public, max-age=300, stale-while-revalidate=86400",
  "content-disposition": 'inline; filename="install.sh"',
  "content-type": "text/x-shellscript; charset=utf-8",
  "x-content-type-options": "nosniff",
});
const installer = new TextDecoder().decode(
  Uint8Array.from(atob(installerBase64), (character) =>
    character.charCodeAt(0),
  ),
);

export default {
  fetch(request) {
    const url = new URL(request.url);
    if (!INSTALL_PATHS.has(url.pathname)) {
      return new Response("not found\n", {
        status: 404,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("method not allowed\n", {
        status: 405,
        headers: {
          allow: "GET, HEAD",
          "content-type": "text/plain; charset=utf-8",
        },
      });
    }
    return new Response(request.method === "HEAD" ? null : installer, {
      headers: INSTALL_HEADERS,
    });
  },
};
