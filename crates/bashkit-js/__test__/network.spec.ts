import test from "ava";
import { Bash, BashTool } from "../wrapper.js";

// ----------------------------------------------------------------------------
// Network configuration (http_client) — allowlist + credential injection.
// Mirrors the Python binding's `network=` kwarg. No test performs a real
// network request: allowlist denial happens before any connection.
// ----------------------------------------------------------------------------

test("curl without network config reports network not configured", async (t) => {
  const bash = new Bash();
  const result = await bash.execute("curl https://example.com");
  t.not(result.exitCode, 0);
  t.regex(result.stderr, /network access not configured/);
});

test("curl to a URL outside the allowlist is denied", async (t) => {
  const bash = new Bash({
    network: { allow: ["https://api.example.com/**"] },
  });
  const result = await bash.execute("curl https://blocked.example.org/x");
  t.not(result.exitCode, 0);
  t.regex(result.stderr, /not in allowlist/);
});

test("BashTool accepts network config", async (t) => {
  const tool = new BashTool({
    network: { allow: ["https://api.example.com/**"] },
  });
  const result = await tool.execute("curl https://blocked.example.org/x");
  t.not(result.exitCode, 0);
  t.regex(result.stderr, /not in allowlist/);
});

test("network config survives reset()", async (t) => {
  const bash = new Bash({
    network: { allow: ["https://api.example.com/**"] },
  });
  bash.reset();
  const result = await bash.execute("curl https://blocked.example.org/x");
  t.not(result.exitCode, 0);
  t.regex(result.stderr, /not in allowlist/);
});

test("allowAll mode passes allowlist check for any URL", async (t) => {
  const bash = new Bash({
    network: { allowAll: true, blockPrivateIps: true },
  });
  // The URL passes the allowlist; the failure (if any) is a connection
  // error, not an allowlist denial.
  const result = await bash.execute(
    "curl --max-time 1 https://nonexistent.invalid/x",
  );
  t.notRegex(result.stderr, /not in allowlist/);
  t.notRegex(result.stderr, /network access not configured/);
});

// ----------------------------------------------------------------------------
// Credential placeholders — scripts see an opaque placeholder, never the
// real secret.
// ----------------------------------------------------------------------------

test("credential placeholder env var is set to an opaque value", async (t) => {
  const secret = "super-secret-123";
  const bash = new Bash({
    network: {
      allow: ["https://api.example.com/**"],
      credentialPlaceholders: [
        {
          env: "API_TOKEN",
          pattern: "https://api.example.com/**",
          kind: "bearer",
          token: secret,
        },
      ],
    },
  });
  const result = await bash.execute('echo "token=$API_TOKEN"');
  t.is(result.exitCode, 0);
  t.regex(result.stdout, /token=bk_placeholder_[0-9a-f]+/);
  t.false(result.stdout.includes(secret));
});

test("direct credentials never appear in the shell environment", async (t) => {
  const secret = "direct-secret-456";
  const bash = new Bash({
    network: {
      allow: ["https://api.example.com/**"],
      credentials: [
        {
          pattern: "https://api.example.com/**",
          kind: "header",
          name: "X-Api-Key",
          value: secret,
        },
      ],
    },
  });
  const result = await bash.execute("env");
  t.is(result.exitCode, 0);
  t.false(result.stdout.includes(secret));
});

// ----------------------------------------------------------------------------
// Constructor validation — mirrors the Python binding's rules.
// ----------------------------------------------------------------------------

test("network without allow or allowAll throws", (t) => {
  t.throws(() => new Bash({ network: {} }), {
    message: /must provide 'allow'.*or 'allowAll: true'/,
  });
});

test("allow and allowAll are mutually exclusive", (t) => {
  t.throws(() => new Bash({ network: { allow: ["x"], allowAll: true } }), {
    message: /mutually exclusive/,
  });
});

test("unknown credential kind throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentials: [{ pattern: "x", kind: "bogus" }],
        },
      }),
    { message: /unknown kind 'bogus'/ },
  );
});

test("bearer credential without token throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentials: [{ pattern: "x", kind: "bearer" }],
        },
      }),
    { message: /kind 'bearer' requires 'token'/ },
  );
});

test("header credential without name/value throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentials: [{ pattern: "x", kind: "header", name: "X-K" }],
        },
      }),
    { message: /kind 'header' requires 'name' and 'value'/ },
  );
});

test("headers credential with empty list throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentials: [{ pattern: "x", kind: "headers", headers: [] }],
        },
      }),
    { message: /must contain at least one entry/ },
  );
});

test("headers credential with an empty header name throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentials: [
            {
              pattern: "x",
              kind: "headers",
              headers: [{ name: "", value: "v" }],
            },
          ],
        },
      }),
    { message: /name must be a non-empty header name/ },
  );
});

test("placeholder with empty env name throws", (t) => {
  t.throws(
    () =>
      new Bash({
        network: {
          allow: ["x"],
          credentialPlaceholders: [
            { env: "", pattern: "x", kind: "bearer", token: "t" },
          ],
        },
      }),
    { message: /'env' must be a non-empty environment variable name/ },
  );
});
