// Decision: keep the Astro site static and do content negotiation at the
// Cloudflare Worker edge. HTML stays the default; Markdown is served only when
// the client explicitly prefers `text/markdown`.
// Old doc URLs that were split/merged into the per-target quickstarts. Keep
// them as permanent redirects so external links and bookmarks don't 404.
const REDIRECTS = new Map([
  ["/docs/embedding", "/docs/start-rust"],
  ["/docs/targets", "/docs/start"],
]);

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    const redirectTarget = REDIRECTS.get(url.pathname.replace(/\/$/, ""));
    if (redirectTarget) {
      return Response.redirect(new URL(redirectTarget, url).toString(), 301);
    }

    const assetPath = markdownAssetPath(url.pathname);

    if (
      assetPath &&
      (request.method === "GET" || request.method === "HEAD") &&
      prefersMarkdown(request.headers.get("Accept") ?? "")
    ) {
      const markdownUrl = new URL(assetPath, url);
      const response = await env.ASSETS.fetch(new Request(markdownUrl, request));
      if (response.ok) {
        return withVary(response, "Accept", "text/markdown; charset=utf-8");
      }
    }

    const response = await env.ASSETS.fetch(request);
    if (assetPath) {
      return withVary(response, "Accept");
    }
    return response;
  },
};

export function markdownAssetPath(pathname) {
  if (pathname === "/") {
    return "/index.md";
  }

  if (pathname === "/docs" || pathname === "/docs/") {
    return "/docs.md";
  }

  const match = /^\/docs\/([^/.]+)\/?$/.exec(pathname);
  if (!match) {
    return null;
  }

  return `/docs/${match[1]}.md`;
}

export function prefersMarkdown(acceptHeader) {
  const markdownQ = qualityFor(acceptHeader, "text/markdown", {
    includeWildcards: false,
  });
  if (markdownQ <= 0) {
    return false;
  }

  const htmlQ = qualityFor(acceptHeader, "text/html");
  return markdownQ >= htmlQ;
}

function qualityFor(acceptHeader, mediaType, options = {}) {
  const includeWildcards = options.includeWildcards ?? true;
  let best = 0;

  for (const part of acceptHeader.split(",")) {
    const [rawType, ...params] = part.trim().split(";");
    const type = rawType.toLowerCase();
    if (!type || !matchesMediaType(type, mediaType, includeWildcards)) {
      continue;
    }

    const qParam = params.find((param) => param.trim().toLowerCase().startsWith("q="));
    const q = qParam ? Number.parseFloat(qParam.split("=")[1]) : 1;
    if (Number.isFinite(q)) {
      best = Math.max(best, Math.min(Math.max(q, 0), 1));
    }
  }

  return best;
}

function matchesMediaType(candidate, mediaType, includeWildcards) {
  if (candidate === mediaType) {
    return true;
  }

  if (!includeWildcards) {
    return false;
  }

  const [candidateType, candidateSubtype] = candidate.split("/");
  const [mediaTypeType, mediaTypeSubtype] = mediaType.split("/");
  return (
    (candidateType === "*" || candidateType === mediaTypeType) &&
    (candidateSubtype === "*" || candidateSubtype === mediaTypeSubtype)
  );
}

function withVary(response, headerName, contentType) {
  const headers = new Headers(response.headers);
  appendVary(headers, headerName);
  if (contentType) {
    headers.set("Content-Type", contentType);
  }

  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function appendVary(headers, headerName) {
  const vary = headers.get("Vary");
  if (!vary) {
    headers.set("Vary", headerName);
    return;
  }

  const values = vary.split(",").map((value) => value.trim().toLowerCase());
  if (!values.includes(headerName.toLowerCase())) {
    headers.set("Vary", `${vary}, ${headerName}`);
  }
}
