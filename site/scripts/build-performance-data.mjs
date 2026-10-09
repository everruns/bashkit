import { access, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Decision: publish only aggregated history. Raw eval traces and per-iteration
// benchmark samples are useful locally, but too large for the static site.
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(siteDir, "..");

const outputPath = path.join(siteDir, "src/data/performance-timeline.json");
const benchDir = path.join(repoRoot, "crates/bashkit-bench/results");
const criterionDir = path.join(repoRoot, "crates/bashkit/benches/results");
const evalDir = path.join(repoRoot, "crates/bashkit-eval/results");
const gapDir = path.join(evalDir, "gaps");
const oilsSpecDir = path.join(repoRoot, "scripts/oils-spec/results");

const benchmarkCategoryDescriptions = {
  arithmetic: "Integer math, substitutions, and expression-heavy shell snippets.",
  arrays: "Indexed array reads, writes, expansion, and iteration.",
  complex: "Mixed shell workflows that combine multiple language features.",
  control: "Conditionals, loops, case statements, and branching scripts.",
  io: "File reads, writes, redirects, and filesystem-facing commands.",
  large: "Bigger scripts and higher-volume data paths.",
  pipes: "Pipeline construction, streaming, and command chaining.",
  startup: "Small commands where interpreter startup dominates runtime.",
  strings: "String expansion, pattern handling, and text manipulation.",
  subshell: "Command substitution and nested shell execution paths.",
  tools: "Builtin and external-tool style command workloads.",
  variables: "Variable assignment, lookup, expansion, and environment handling.",
};

function round(value, digits = 2) {
  if (!Number.isFinite(value)) return null;
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
}

function percentile(values, p) {
  const sorted = values.filter(Number.isFinite).toSorted((a, b) => a - b);
  if (sorted.length === 0) return null;
  const index = (sorted.length - 1) * p;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  if (lower === upper) return sorted[lower];
  return sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower);
}

function unixSecondsToIso(seconds) {
  const n = Number(seconds);
  if (!Number.isFinite(n) || n <= 0) return null;
  return new Date(n * 1000).toISOString();
}

function dateLabel(iso) {
  if (!iso) return "unknown";
  return iso.slice(0, 10);
}

function parseJsonFileTimestamp(fileName) {
  const isoMatch = fileName.match(/(\d{4}-\d{2}-\d{2})-(\d{6})/);
  if (!isoMatch) return null;
  const [, date, time] = isoMatch;
  return `${date}T${time.slice(0, 2)}:${time.slice(2, 4)}:${time.slice(4, 6)}Z`;
}

function parseCriterionTimestamp(fileName, content) {
  const contentMatch = content.match(/\*\*Timestamp\*\*:\s*([0-9]+)/);
  if (contentMatch) return unixSecondsToIso(contentMatch[1]);
  const fileMatch = fileName.match(/-([0-9]+)\.md$/);
  return fileMatch ? unixSecondsToIso(fileMatch[1]) : null;
}

function parseTimeToUs(raw) {
  if (typeof raw !== "string") return null;
  const match = raw
    .replaceAll("`", "")
    .match(/([0-9]+(?:\.[0-9]+)?)\s*(ns|us|µs|ms|s)\b/i);
  if (!match) return null;
  const value = Number(match[1]);
  const unit = match[2].toLowerCase();
  if (unit === "ns") return value / 1000;
  if (unit === "us" || unit === "µs") return value;
  if (unit === "ms") return value * 1000;
  if (unit === "s") return value * 1_000_000;
  return null;
}

function parseMarkdownTables(content) {
  const lines = content.split(/\r?\n/);
  const rows = [];
  let headers = null;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (!line.startsWith("|") || !line.endsWith("|")) {
      headers = null;
      continue;
    }

    const cells = line
      .slice(1, -1)
      .split("|")
      .map((cell) => cell.trim());

    const next = lines[i + 1]?.trim() ?? "";
    if (next.startsWith("|") && /^[-:|\s]+$/.test(next.slice(1, -1))) {
      headers = cells.map((cell) => cell.toLowerCase());
      i += 1;
      continue;
    }

    if (headers && cells.length === headers.length) {
      rows.push(Object.fromEntries(headers.map((header, index) => [header, cells[index]])));
    }
  }

  return rows;
}

function parsePercent(raw) {
  const match = raw?.match(/-?[0-9]+(?:\.[0-9]+)?/);
  return match ? Number(match[0]) : null;
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

async function existingMarkdownReport(relativeSource) {
  if (relativeSource.endsWith(".md")) return relativeSource;

  const reportSource = relativeSource.replace(/\.[^.]+$/, ".md");
  try {
    await access(path.join(repoRoot, reportSource));
    return reportSource;
  } catch {
    return null;
  }
}

async function listFiles(dir, extension) {
  return (await readdir(dir))
    .filter((file) => file.endsWith(extension))
    .toSorted((a, b) => a.localeCompare(b));
}

async function buildBenchRuns() {
  const files = await listFiles(benchDir, ".json");
  const runs = [];

  for (const file of files) {
    const sourcePath = path.join(benchDir, file);
    const data = await readJson(sourcePath);
    const timestamp = unixSecondsToIso(data.timestamp) ?? parseJsonFileTimestamp(file);
    const stats = data.summary?.runner_stats ?? {};
    const bashkit = stats.bashkit;
    const bash = stats.bash;
    if (!bashkit || !bash) continue;

    const categoryPairs = new Map();
    for (const row of data.results ?? []) {
      if (!row.category || !row.runner || !Number.isFinite(row.mean_ns)) continue;
      const key = `${row.category}:${row.case_name}`;
      const existing = categoryPairs.get(key) ?? { category: row.category };
      existing[row.runner] = row.mean_ns / 1_000_000;
      categoryPairs.set(key, existing);
    }

    const byCategory = new Map();
    for (const row of categoryPairs.values()) {
      if (!Number.isFinite(row.bashkit) || !Number.isFinite(row.bash) || row.bashkit <= 0) {
        continue;
      }
      const bucket = byCategory.get(row.category) ?? {
        bashkitMs: [],
        bashMs: [],
        ratios: [],
        cases: 0,
      };
      bucket.bashkitMs.push(row.bashkit);
      bucket.bashMs.push(row.bash);
      bucket.ratios.push(row.bash / row.bashkit);
      bucket.cases += 1;
      byCategory.set(row.category, bucket);
    }

    const categories = [...byCategory.entries()]
      .map(([category, bucket]) => ({
        category,
        description: benchmarkCategoryDescriptions[category] ?? "Benchmarks grouped by harness category.",
        cases: bucket.cases,
        bashkitMedianMs: round(percentile(bucket.bashkitMs, 0.5), 3),
        bashMedianMs: round(percentile(bucket.bashMs, 0.5), 3),
        speedup: round(percentile(bucket.ratios, 0.5), 1),
      }))
      .sort((a, b) => a.bashkitMedianMs - b.bashkitMedianMs);

    const speedup = bashkit.total_time_ms > 0 ? bash.total_time_ms / bashkit.total_time_ms : null;
    const source = `crates/bashkit-bench/results/${file}`;
    runs.push({
      id: file.replace(/\.json$/, ""),
      kind: "bashkit-bench",
      label: data.moniker ?? data.system?.moniker ?? file,
      date: dateLabel(timestamp),
      timestamp,
      source,
      reportSource: await existingMarkdownReport(source),
      cases: data.summary?.total_cases ?? categories.reduce((sum, item) => sum + item.cases, 0),
      speedup: round(speedup, 1),
      bashkitMs: round(bashkit.total_time_ms, 2),
      bashMs: round(bash.total_time_ms, 2),
      errorRate: round(bashkit.error_rate * 100, 2),
      matchRate: round(bashkit.output_match_rate * 100, 2),
      categories,
    });
  }

  return runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
}

function criterionFamily(fileName) {
  const base = fileName.replace(/^criterion-/, "").replace(/-[0-9]+\.md$/, "");
  if (base.startsWith("hotpath")) return "hotpath";
  if (base.startsWith("file_ops")) return "file-ops";
  if (base.startsWith("parallel")) return "parallel";
  if (base.startsWith("sqlite")) return "sqlite";
  return base.split("-")[0] || "criterion";
}

async function buildCriterionRuns() {
  const files = await listFiles(criterionDir, ".md");
  const runs = [];

  for (const file of files) {
    if (file === "README.md") continue;
    const sourcePath = path.join(criterionDir, file);
    const content = await readFile(sourcePath, "utf8");
    const timestamp = parseCriterionTimestamp(file, content);
    const title = content.match(/^#\s+(.+)$/m)?.[1] ?? file;
    const rows = parseMarkdownTables(content);

    const changes = rows
      .map((row) => parsePercent(row.change))
      .filter((value) => Number.isFinite(value));
    const timesUs = rows
      .map((row) => parseTimeToUs(row["time (median)"] ?? row.time ?? row.after ?? row["after (µs)"]))
      .filter((value) => Number.isFinite(value));

    const fastestRow = rows
      .map((row) => ({
        name: row.benchmark ?? row.case ?? row["group / case"] ?? row.bench ?? "case",
        us: parseTimeToUs(row["time (median)"] ?? row.time ?? row.after ?? row["after (µs)"]),
      }))
      .filter((row) => Number.isFinite(row.us))
      .toSorted((a, b) => a.us - b.us)[0];

    const bestChangeRow = rows
      .map((row) => ({
        name: row.bench ?? row.case ?? row.benchmark ?? "case",
        change: parsePercent(row.change),
      }))
      .filter((row) => Number.isFinite(row.change))
      .toSorted((a, b) => a.change - b.change)[0];

    const summaryMedianMatch = content.match(/median change:\s*\*\*(-?[0-9.]+)%\*\*/i);
    const summaryMeanMatch = content.match(/mean change:\s*\*\*(-?[0-9.]+)%\*\*/i);

    const source = `crates/bashkit/benches/results/${file}`;
    runs.push({
      id: file.replace(/\.md$/, ""),
      kind: "criterion",
      family: criterionFamily(file),
      label: title,
      date: dateLabel(timestamp),
      timestamp,
      source,
      reportSource: source,
      cases: Math.max(changes.length, timesUs.length),
      medianUs: round(percentile(timesUs, 0.5), 2),
      p95Us: round(percentile(timesUs, 0.95), 2),
      medianChangePct: round(
        summaryMedianMatch ? Number(summaryMedianMatch[1]) : percentile(changes, 0.5),
        1,
      ),
      meanChangePct: round(
        summaryMeanMatch ? Number(summaryMeanMatch[1]) : changes.reduce((sum, n) => sum + n, 0) / changes.length,
        1,
      ),
      bestChangePct: round(bestChangeRow?.change, 1),
      fastestCase: fastestRow ? { name: fastestRow.name, us: round(fastestRow.us, 2) } : null,
      bestImprovement: bestChangeRow
        ? { name: bestChangeRow.name, changePct: round(bestChangeRow.change, 1) }
        : null,
    });
  }

  return runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
}

// Python startup series: one point per `criterion-python-*` report. The raw
// start table (fresh process) and warm per-call rows are what users feel.
async function buildPythonStartup() {
  const files = (await listFiles(criterionDir, ".md")).filter((file) => file.startsWith("criterion-python-"));
  const runs = [];

  for (const file of files) {
    const content = await readFile(path.join(criterionDir, file), "utf8");
    const rows = parseMarkdownTables(content);
    const startMs = (runtime, measure) => {
      const row = rows.find((r) => r.runtime === runtime && r.measure?.startsWith(measure));
      const us = parseTimeToUs(row?.median);
      return Number.isFinite(us) ? round(us / 1000, 3) : null;
    };
    const benchMs = (name) => {
      const row = rows.find((r) => r.benchmark === name);
      const us = parseTimeToUs(row?.time);
      return Number.isFinite(us) ? round(us / 1000, 3) : null;
    };
    const runtime = (name) => ({
      firstCallMs: startMs(name, "first call"),
      secondCallMs: startMs(name, "second call"),
      warmPrintMs: benchMs(`python_call/${name}/print`),
    });
    const timestamp = parseCriterionTimestamp(file, content);
    const source = `crates/bashkit/benches/results/${file}`;
    runs.push({
      id: file.replace(/\.md$/, ""),
      date: dateLabel(timestamp),
      timestamp,
      source,
      monty: runtime("monty"),
      cpython: {
        ...runtime("cpython"),
        importHttpClientMs: benchMs("python_import/cpython/http_client"),
      },
    });
  }

  return runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
}

// Gap telemetry: one point per `bashkit-replay` report (recorded agent tool
// calls replayed on the current bashkit). The latest report's top gaps pick
// the order of coverage work; history shows gap calls shrinking.
async function buildGapTelemetry() {
  let files;
  try {
    files = (await listFiles(gapDir, ".json")).filter((file) => file.startsWith("gaps-"));
  } catch {
    return { runs: [], latest: null };
  }
  const runs = [];
  for (const file of files) {
    const report = await readJson(path.join(gapDir, file));
    const source = `crates/bashkit-eval/results/gaps/${file}`;
    runs.push({
      id: file.replace(/\.json$/, ""),
      date: dateLabel(report.generated_at),
      timestamp: report.generated_at,
      source,
      reportSource: await existingMarkdownReport(source),
      version: report.bashkit_version,
      sessions: report.sessions,
      calls: report.calls,
      gapCallsNow: report.now.gap_calls,
      gapCallsRecorded: report.recorded.gap_calls,
      failedCallsNow: report.now.failed_calls,
      failedCallsRecorded: report.recorded.failed_calls,
      top: report.top,
      fixed: report.fixed.slice(0, 10),
    });
  }
  const sorted = runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
  const latestRun = sorted.at(-1) ?? null;
  return {
    runs: sorted.map(({ top, fixed, ...point }) => point),
    latest: latestRun,
  };
}

// Oils spec pass rate: one point per `scripts/oils-spec/run.py --save` report
// (upstream Oils spec suite, bash column). Headline = bashkit passes among the
// cases real bash passes; the latest report's worst spec files are listed.
async function buildOilsSpec() {
  let files;
  try {
    files = (await listFiles(oilsSpecDir, ".json")).filter((file) => file.startsWith("oils-spec-"));
  } catch {
    return { runs: [], latest: null };
  }
  const runs = [];
  for (const file of files) {
    const report = await readJson(path.join(oilsSpecDir, file));
    const source = `scripts/oils-spec/results/${file}`;
    const total = report.total;
    runs.push({
      id: file.replace(/\.json$/, ""),
      date: dateLabel(report.timestamp),
      timestamp: report.timestamp,
      source,
      reportSource: await existingMarkdownReport(source),
      oilsRev: report.oils_rev.slice(0, 12),
      bashkitCommit: report.bashkit_commit,
      bashVersion: report.bash_version,
      cases: total.cases,
      bashPass: total.bash_pass ?? null,
      bashkitPass: total.bashkit_pass,
      bashkitPassOfBash: total.bashkit_pass_of_bash ?? null,
      passRate: total.pass_rate,
      topFiles: Object.entries(report.files)
        .slice(0, 12)
        .map(([name, entry]) => ({
          name,
          cases: entry.summary.cases,
          bashPass: entry.summary.bash_pass ?? entry.summary.cases,
          bashkitPass: entry.summary.bashkit_pass_of_bash ?? entry.summary.bashkit_pass,
          passRate: entry.summary.pass_rate,
        })),
    });
  }
  const sorted = runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
  return {
    runs: sorted.map(({ topFiles, ...point }) => point),
    latest: sorted.at(-1) ?? null,
  };
}

async function buildEvalRuns() {
  const files = await listFiles(evalDir, ".json");
  const runs = [];

  for (const file of files) {
    const data = await readJson(path.join(evalDir, file));
    const summary = data.summary;
    if (!summary?.total_tasks || !Number.isFinite(summary.overall_rate)) continue;
    const timestamp = data.timestamp ?? parseJsonFileTimestamp(file);
    const categories = Object.entries(summary.by_category ?? {})
      .map(([category, row]) => ({
        category,
        tasks: row.tasks,
        passed: row.passed,
        rate: round(row.rate * 100, 1),
      }))
      .sort((a, b) => a.rate - b.rate || b.tasks - a.tasks);

    const source = `crates/bashkit-eval/results/${file}`;
    runs.push({
      id: file.replace(/\.json$/, ""),
      kind: file.startsWith("scripting-eval") ? "scripting-eval" : "llm-eval",
      provider: data.provider ?? "unknown",
      model: data.model ?? "unknown",
      baseline: data.baseline ?? null,
      label: `${data.provider ?? "unknown"}/${data.model ?? "unknown"}`,
      date: dateLabel(timestamp),
      timestamp,
      source,
      reportSource: await existingMarkdownReport(source),
      tasks: summary.total_tasks,
      passed: summary.total_passed,
      scorePct: round(summary.overall_rate * 100, 1),
      toolSuccessPct: round(summary.tool_call_success_rate * 100, 1),
      avgTurns: round(summary.avg_turns_per_task, 2),
      avgToolCalls: round(summary.avg_tool_calls_per_task, 2),
      avgDurationMs: round(summary.avg_duration_ms, 0),
      inputTokens: summary.total_input_tokens ?? null,
      outputTokens: summary.total_output_tokens ?? null,
      categories,
    });
  }

  return runs.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
}

// mira runs (results/mira/<run_id>/report.json) hold every target of one
// `mira run`; split them into one eval run per target so they line up with
// the pre-mira per-model JSON files. Only the full `bashkit_bash` eval feeds
// the site. Score = tasks passed / tasks scored (a case passes iff every check
// passes), matching the README tables. Infra errors (no scores) are excluded.
async function buildMiraEvalRuns() {
  const miraDir = path.join(evalDir, "mira");
  let runIds = [];
  try {
    runIds = (await readdir(miraDir)).toSorted();
  } catch {
    return [];
  }
  const categoryById = new Map();
  const tasksFile = await readFile(path.join(repoRoot, "crates/bashkit-eval/data/eval-tasks.jsonl"), "utf8");
  for (const line of tasksFile.split("\n")) {
    if (!line.trim()) continue;
    const task = JSON.parse(line);
    categoryById.set(task.id, task.category);
  }

  const runs = [];
  for (const runId of runIds) {
    let report;
    let meta;
    try {
      report = await readJson(path.join(miraDir, runId, "report.json"));
      meta = await readJson(path.join(miraDir, runId, "meta.json"));
    } catch {
      continue;
    }
    const timestamp = meta.started_unix ? new Date(meta.started_unix * 1000).toISOString() : null;
    const byTarget = new Map();
    for (const item of report.cases ?? []) {
      if (item.eval !== "bashkit_bash" || item.skipped || !item.scores?.length) continue;
      const bucket = byTarget.get(item.target) ?? [];
      bucket.push(item);
      byTarget.set(item.target, bucket);
    }

    for (const [target, cases] of byTarget) {
      const slash = target.indexOf("/");
      const provider = target.slice(0, slash);
      // OpenRouter ids carry a vendor prefix (openai/gpt-5.6-sol); the model
      // name alone keys trends across direct and routed runs.
      const model = target.slice(slash + 1).split("/").at(-1);
      const sum = (pick) => cases.reduce((acc, item) => acc + (pick(item) ?? 0), 0);
      const passed = cases.filter((item) => item.passed).length;
      const toolCalls = sum((item) => item.transcript?.metrics?.tool_calls);
      const toolOk = sum((item) => item.transcript?.metrics?.tool_calls_ok);
      const byCategory = new Map();
      for (const item of cases) {
        const category = categoryById.get(item.sample) ?? "unknown";
        const row = byCategory.get(category) ?? { tasks: 0, passed: 0 };
        row.tasks += 1;
        row.passed += item.passed ? 1 : 0;
        byCategory.set(category, row);
      }
      const categories = [...byCategory.entries()]
        .map(([category, row]) => ({ ...row, category, rate: round((row.passed / row.tasks) * 100, 1) }))
        .sort((a, b) => a.rate - b.rate || b.tasks - a.tasks);
      const source = `crates/bashkit-eval/results/mira/${runId}/report.json`;
      runs.push({
        id: `mira-${runId}-${target.replaceAll("/", "-")}`,
        kind: "llm-eval",
        provider,
        model,
        baseline: null,
        label: target,
        date: dateLabel(timestamp),
        timestamp,
        source,
        reportSource: `crates/bashkit-eval/results/mira/${runId}/report.html`,
        tasks: cases.length,
        passed,
        scorePct: round((passed / cases.length) * 100, 1),
        toolSuccessPct: toolCalls ? round((toolOk / toolCalls) * 100, 1) : null,
        avgTurns: round(sum((item) => item.transcript?.metrics?.turns) / cases.length, 2),
        avgToolCalls: round(toolCalls / cases.length, 2),
        avgDurationMs: round(sum((item) => item.transcript?.timing?.duration_ms) / cases.length, 0),
        inputTokens: sum((item) => item.transcript?.usage?.input_tokens),
        outputTokens: sum((item) => item.transcript?.usage?.output_tokens),
        categories,
      });
    }
  }
  return runs;
}

function bestBy(items, score) {
  return items.reduce((best, item) => {
    if (!best) return item;
    return score(item) > score(best) ? item : best;
  }, null);
}

function latest(items) {
  return items.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp)).at(-1) ?? null;
}

function buildMilestones({ benchRuns, criterionRuns, evalRuns }) {
  const points = [];

  for (const run of benchRuns) {
    points.push({
      date: run.date,
      timestamp: run.timestamp,
      kind: "Benchmark",
      title: `${run.speedup}x faster than bash`,
      detail: `${run.cases} parity/perf cases on ${run.label}; output match ${run.matchRate}%.`,
      metric: run.speedup,
      source: run.source,
    });
  }

  for (const run of criterionRuns) {
    const improvement = run.bestImprovement
      ? `${Math.abs(run.bestImprovement.changePct)}% faster in ${run.bestImprovement.name}`
      : run.fastestCase
        ? `${run.fastestCase.name} at ${run.fastestCase.us} us median`
        : `${run.cases} criterion cases`;
    points.push({
      date: run.date,
      timestamp: run.timestamp,
      kind: "Criterion",
      title: run.family,
      detail: improvement,
      metric: run.medianChangePct ?? run.medianUs,
      source: run.source,
    });
  }

  for (const run of evalRuns) {
    if (run.tasks < 10 && !run.kind.includes("scripting")) continue;
    const weakest = run.categories[0];
    points.push({
      date: run.date,
      timestamp: run.timestamp,
      kind: "Eval",
      title: `${run.model}: ${run.scorePct}%`,
      detail: `${run.passed}/${run.tasks} tasks passed. Weakest category: ${weakest?.category ?? "n/a"} (${weakest?.rate ?? "n/a"}%).`,
      metric: run.scorePct,
      source: run.source,
    });
  }

  return points
    .filter((point) => point.timestamp)
    .toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp));
}

function buildModelTrends(evalRuns) {
  const byModel = new Map();
  for (const run of evalRuns.filter((item) => item.tasks >= 10)) {
    const key = run.model;
    const bucket = byModel.get(key) ?? [];
    bucket.push({ date: run.date, timestamp: run.timestamp, scorePct: run.scorePct, passed: run.passed, tasks: run.tasks });
    byModel.set(key, bucket);
  }

  return [...byModel.entries()]
    .map(([model, points]) => ({
      model,
      points: points.toSorted((a, b) => new Date(a.timestamp) - new Date(b.timestamp)),
    }))
    .sort((a, b) => a.model.localeCompare(b.model));
}

const benchRuns = await buildBenchRuns();
const criterionRuns = await buildCriterionRuns();
const evalRuns = [...(await buildEvalRuns()), ...(await buildMiraEvalRuns())].toSorted(
  (a, b) => new Date(a.timestamp) - new Date(b.timestamp),
);
const pythonStartup = await buildPythonStartup();
const gapTelemetry = await buildGapTelemetry();
const oilsSpec = await buildOilsSpec();
const newestSourceTimestamp = latest([...benchRuns, ...criterionRuns, ...evalRuns])?.timestamp ?? null;

const payload = {
  generatedAt: newestSourceTimestamp,
  sources: {
    bench: "crates/bashkit-bench/results/*.json",
    criterion: "crates/bashkit/benches/results/*.md",
    evals: "crates/bashkit-eval/results/*.json + results/mira/*/report.json",
    gaps: "crates/bashkit-eval/results/gaps/gaps-*.json",
    oilsSpec: "scripts/oils-spec/results/oils-spec-*.json",
  },
  summary: {
    benchRuns: benchRuns.length,
    criterionRuns: criterionRuns.length,
    evalRuns: evalRuns.length,
    latestBench: latest(benchRuns),
    latestEval: latest(evalRuns.filter((run) => run.tasks >= 10)),
    bestEval: bestBy(evalRuns.filter((run) => run.tasks >= 10), (run) => run.scorePct),
    bestBenchmark: bestBy(benchRuns, (run) => run.speedup ?? 0),
    bestCriterionImprovement: bestBy(
      criterionRuns.filter((run) => Number.isFinite(run.bestChangePct)),
      (run) => Math.abs(run.bestChangePct),
    ),
  },
  benchRuns,
  criterionRuns,
  evalRuns,
  pythonStartup,
  gapTelemetry,
  oilsSpec,
  modelTrends: buildModelTrends(evalRuns),
  milestones: buildMilestones({ benchRuns, criterionRuns, evalRuns }),
};

await mkdir(path.dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(payload, null, 2)}\n`);

console.log(
  `Wrote ${path.relative(repoRoot, outputPath)}: ${benchRuns.length} benchmark runs, ${criterionRuns.length} criterion runs, ${evalRuns.length} eval runs.`,
);
