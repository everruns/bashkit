#!/usr/bin/env node
/**
 * Kubernetes API orchestrator using bashkit ScriptedTool.
 *
 * Demonstrates composing 12 mock K8s API tools into a single ScriptedTool that
 * an LLM agent can call with bash scripts. Each tool becomes a bash builtin;
 * the agent writes one script to orchestrate them all.
 *
 * Mirrors the Python k8s_orchestrator.py example.
 *
 * Run:
 *   node examples/k8s_orchestrator.mjs
 */

import { ScriptedTool } from "@everruns/bashkit";

// =============================================================================
// Fake k8s data
// =============================================================================

const NODES = [
  { name: "node-1", status: "Ready", cpu: "4", memory: "16Gi", pods: 23 },
  { name: "node-2", status: "Ready", cpu: "8", memory: "32Gi", pods: 41 },
  { name: "node-3", status: "NotReady", cpu: "4", memory: "16Gi", pods: 0 },
];

const NAMESPACES = [
  { name: "default", status: "Active" },
  { name: "kube-system", status: "Active" },
  { name: "monitoring", status: "Active" },
  { name: "production", status: "Active" },
];

const PODS = {
  default: [
    { name: "web-abc12", status: "Running", restarts: 0, node: "node-1", image: "nginx:1.25" },
    { name: "api-def34", status: "Running", restarts: 2, node: "node-2", image: "api:v2.1" },
    { name: "worker-ghi56", status: "CrashLoopBackOff", restarts: 15, node: "node-2", image: "worker:v1.0" },
  ],
  "kube-system": [
    { name: "coredns-aaa11", status: "Running", restarts: 0, node: "node-1", image: "coredns:1.11" },
    { name: "etcd-bbb22", status: "Running", restarts: 0, node: "node-1", image: "etcd:3.5" },
  ],
  monitoring: [
    { name: "prometheus-ccc33", status: "Running", restarts: 0, node: "node-2", image: "prom:2.48" },
    { name: "grafana-ddd44", status: "Running", restarts: 1, node: "node-2", image: "grafana:10.2" },
  ],
  production: [
    { name: "app-eee55", status: "Running", restarts: 0, node: "node-1", image: "app:v3.2" },
    { name: "app-fff66", status: "Running", restarts: 0, node: "node-2", image: "app:v3.2" },
    { name: "db-ggg77", status: "Pending", restarts: 0, node: "", image: "postgres:16" },
  ],
};

const DEPLOYMENTS = {
  default: [
    { name: "web", replicas: 1, available: 1, image: "nginx:1.25" },
    { name: "api", replicas: 2, available: 2, image: "api:v2.1" },
    { name: "worker", replicas: 1, available: 0, image: "worker:v1.0" },
  ],
  production: [
    { name: "app", replicas: 2, available: 2, image: "app:v3.2" },
    { name: "db", replicas: 1, available: 0, image: "postgres:16" },
  ],
};

const SERVICES = {
  default: [
    { name: "web-svc", type: "LoadBalancer", clusterIP: "10.0.0.10", ports: "80/TCP" },
    { name: "api-svc", type: "ClusterIP", clusterIP: "10.0.0.20", ports: "8080/TCP" },
  ],
  production: [
    { name: "app-svc", type: "LoadBalancer", clusterIP: "10.0.1.10", ports: "443/TCP" },
  ],
};

const CONFIGMAPS = {
  default: [{ name: "app-config", data_keys: ["DATABASE_URL", "LOG_LEVEL", "CACHE_TTL"] }],
  production: [{ name: "prod-config", data_keys: ["DATABASE_URL", "REDIS_URL"] }],
};

const EVENTS = [
  { namespace: "default", type: "Warning", reason: "BackOff", object: "pod/worker-ghi56", message: "Back-off restarting failed container" },
  { namespace: "production", type: "Warning", reason: "FailedScheduling", object: "pod/db-ggg77", message: "Insufficient memory on available nodes" },
  { namespace: "default", type: "Normal", reason: "Pulled", object: "pod/api-def34", message: "Successfully pulled image api:v2.1" },
  { namespace: "monitoring", type: "Normal", reason: "Started", object: "pod/prometheus-ccc33", message: "Started container prometheus" },
];

const LOGS = {
  "web-abc12": "2024-01-15T10:00:01Z GET /health 200 1ms\n2024-01-15T10:00:02Z GET /api/users 200 45ms\n",
  "api-def34": "2024-01-15T10:00:01Z INFO  Starting API server on :8080\n2024-01-15T10:00:02Z WARN  High latency detected: 250ms\n",
  "worker-ghi56": "2024-01-15T10:00:01Z ERROR Connection refused: redis://redis:6379\n2024-01-15T10:00:02Z FATAL Exiting due to unrecoverable error\n",
};

// Track mutable state for scale operations
const deploymentState = {};

// =============================================================================
// Tool callbacks — each receives (params, stdin) => string
// =============================================================================

function getNodes() {
  return JSON.stringify({ items: NODES }) + "\n";
}

function getNamespaces() {
  return JSON.stringify({ items: NAMESPACES }) + "\n";
}

function getPods(params) {
  const ns = params.namespace || "default";
  return JSON.stringify({ items: PODS[ns] || [] }) + "\n";
}

function getDeployments(params) {
  const ns = params.namespace || "default";
  return JSON.stringify({ items: DEPLOYMENTS[ns] || [] }) + "\n";
}

function getServices(params) {
  const ns = params.namespace || "default";
  return JSON.stringify({ items: SERVICES[ns] || [] }) + "\n";
}

function describePod(params) {
  const name = params.name || "";
  const ns = params.namespace || "default";
  for (const pod of PODS[ns] || []) {
    if (pod.name === name) {
      const detail = { ...pod, namespace: ns, labels: { app: name.split("-").slice(0, -1).join("-") } };
      return JSON.stringify(detail) + "\n";
    }
  }
  throw new Error(`pod ${name} not found in ${ns}`);
}

function getLogs(params) {
  const name = params.name || "";
  const tail = params.tail || 50;
  const logs = LOGS[name] || `No logs available for ${name}\n`;
  const lines = logs.trim().split("\n");
  return lines.slice(-tail).join("\n") + "\n";
}

function getConfigmaps(params) {
  const ns = params.namespace || "default";
  return JSON.stringify({ items: CONFIGMAPS[ns] || [] }) + "\n";
}

function getSecrets(params) {
  const ns = params.namespace || "default";
  const secrets = [{ name: `${ns}-tls`, type: "kubernetes.io/tls", data: "***REDACTED***" }];
  return JSON.stringify({ items: secrets }) + "\n";
}

function getEvents(params) {
  const ns = params.namespace;
  const items = ns ? EVENTS.filter((e) => e.namespace === ns) : EVENTS;
  return JSON.stringify({ items }) + "\n";
}

function scaleDeployment(params) {
  const name = params.name || "";
  const ns = params.namespace || "default";
  const replicas = Number(params.replicas) || 1;
  deploymentState[`${ns}/${name}`] = { replicas };
  return JSON.stringify({ deployment: name, namespace: ns, replicas, status: "scaling" }) + "\n";
}

function rolloutStatus(params) {
  const name = params.name || "";
  const ns = params.namespace || "default";
  const key = `${ns}/${name}`;
  if (deploymentState[key]) {
    const r = deploymentState[key].replicas;
    return JSON.stringify({ deployment: name, status: "progressing", replicas: r, updated: r }) + "\n";
  }
  for (const dep of DEPLOYMENTS[ns] || []) {
    if (dep.name === name) {
      const status = dep.available === dep.replicas ? "available" : "progressing";
      return JSON.stringify({ deployment: name, status, ...dep }) + "\n";
    }
  }
  throw new Error(`deployment ${name} not found in ${ns}`);
}

// =============================================================================
// Build the ScriptedTool with all 12 k8s commands
// =============================================================================

function buildK8sTool() {
  const tool = new ScriptedTool({ name: "kubectl", shortDescription: "Kubernetes cluster management API" });

  tool.addTool("get_nodes", "List cluster nodes", getNodes);

  tool.addTool("get_namespaces", "List namespaces", getNamespaces);

  tool.addTool("get_pods", "List pods in a namespace", getPods, {
    type: "object",
    properties: { namespace: { type: "string", description: "Namespace" } },
  });

  tool.addTool("get_deployments", "List deployments in a namespace", getDeployments, {
    type: "object",
    properties: { namespace: { type: "string", description: "Namespace" } },
  });

  tool.addTool("get_services", "List services in a namespace", getServices, {
    type: "object",
    properties: { namespace: { type: "string", description: "Namespace" } },
  });

  tool.addTool("describe_pod", "Describe a specific pod", describePod, {
    type: "object",
    properties: {
      name: { type: "string", description: "Pod name" },
      namespace: { type: "string", description: "Namespace" },
    },
    required: ["name"],
  });

  tool.addTool("get_logs", "Get pod logs", getLogs, {
    type: "object",
    properties: {
      name: { type: "string", description: "Pod name" },
      tail: { type: "integer", description: "Number of lines" },
    },
    required: ["name"],
  });

  tool.addTool("get_configmaps", "List configmaps in a namespace", getConfigmaps, {
    type: "object",
    properties: { namespace: { type: "string", description: "Namespace" } },
  });

  tool.addTool("get_secrets", "List secrets in a namespace (values redacted)", getSecrets, {
    type: "object",
    properties: { namespace: { type: "string", description: "Namespace" } },
  });

  tool.addTool("get_events", "Get cluster events", getEvents, {
    type: "object",
    properties: { namespace: { type: "string", description: "Filter namespace" } },
  });

  tool.addTool("scale_deployment", "Scale a deployment to N replicas", scaleDeployment, {
    type: "object",
    properties: {
      name: { type: "string", description: "Deployment name" },
      namespace: { type: "string", description: "Namespace" },
      replicas: { type: "integer", description: "Target replica count" },
    },
    required: ["name", "replicas"],
  });

  tool.addTool("rollout_status", "Check deployment rollout status", rolloutStatus, {
    type: "object",
    properties: {
      name: { type: "string", description: "Deployment name" },
      namespace: { type: "string", description: "Namespace" },
    },
    required: ["name"],
  });

  tool.env("KUBECONFIG", "/etc/kubernetes/admin.conf");
  return tool;
}

// =============================================================================
// Demo scripts — what an LLM agent would generate
// =============================================================================

async function runDemos(tool) {
  console.log("=".repeat(70));
  console.log("Kubernetes Orchestrator - 12 tools via ScriptedTool");
  console.log("=".repeat(70));

  // -- Demo 1: Simple listing --
  console.log("\n--- Demo 1: List all nodes ---");
  let r = await tool.execute(
    "get_nodes | jq -r '.items[] | \"\\(.name)  \\(.status)  cpu=\\(.cpu)  mem=\\(.memory)\"'"
  );
  console.log(r.stdout);

  // -- Demo 2: Unhealthy pods across all namespaces --
  console.log("--- Demo 2: Find unhealthy pods across namespaces ---");
  r = await tool.execute(`
    get_namespaces | jq -r '.items[].name' | while read ns; do
      get_pods --namespace "$ns" \
        | jq -r '.items[] | select(.status != "Running") | .name + " " + .status' \
        | while read line; do echo "  $ns/$line"; done
    done
  `);
  console.log(r.stdout);

  // -- Demo 3: Cluster health report --
  console.log("--- Demo 3: Full cluster health report ---");
  r = await tool.execute(`
    echo "=== Cluster Health Report ==="

    # Node status
    echo ""
    echo "-- Nodes --"
    nodes=$(get_nodes)
    total=$(echo "$nodes" | jq '.items | length')
    ready=$(echo "$nodes" | jq '[.items[] | select(.status == "Ready")] | length')
    echo "Nodes: $ready/$total ready"

    # Pod status per namespace
    echo ""
    echo "-- Pods --"
    get_namespaces | jq -r '.items[].name' | while read ns; do
      pods=$(get_pods --namespace "$ns")
      total=$(echo "$pods" | jq '.items | length')
      running=$(echo "$pods" | jq '[.items[] | select(.status == "Running")] | length')
      echo "  $ns: $running/$total running"
    done

    # Warnings
    echo ""
    echo "-- Recent warnings --"
    get_events | jq -r '.items[] | select(.type == "Warning") | "  [\\(.reason)] \\(.object): \\(.message)"'
  `);
  console.log(r.stdout);

  // -- Demo 4: Diagnose CrashLoopBackOff --
  console.log("--- Demo 4: Diagnose crashing pod ---");
  r = await tool.execute(`
    # Find pods in CrashLoopBackOff
    crash_pods=$(get_pods --namespace default | jq -r '.items[] | select(.status == "CrashLoopBackOff") | .name')

    for pod in $crash_pods; do
      echo "=== Diagnosing: $pod ==="
      describe_pod --name "$pod" --namespace default | jq '{name, status, restarts, image, node}'
      echo ""
      echo "Recent logs:"
      get_logs --name "$pod" --tail 5
      echo "Related events:"
      get_events --namespace default | jq -r '.items[] | "  [" + .type + "] " + .reason + ": " + .message'
      echo ""
    done
  `);
  console.log(r.stdout);

  // -- Demo 5: Scale + rollout --
  console.log("--- Demo 5: Scale deployment and check rollout ---");
  r = await tool.execute(`
    echo "Scaling 'app' in production to 5 replicas..."
    scale_deployment --name app --namespace production --replicas 5 | jq '.'
    echo ""
    echo "Rollout status:"
    rollout_status --name app --namespace production | jq '.'
  `);
  console.log(r.stdout);

  // -- Demo 6: Service + configmap inventory --
  console.log("--- Demo 6: Namespace inventory ---");
  r = await tool.execute(`
    for ns in default production; do
      echo "=== Namespace: $ns ==="
      echo "Services:"
      get_services --namespace "$ns" | jq -r '.items[] | "  \\(.name) (\\(.type)) -> \\(.ports)"'
      echo "ConfigMaps:"
      get_configmaps --namespace "$ns" | jq -r '.items[] | "  \\(.name): \\(.data_keys | join(", "))"'
      echo "Secrets:"
      get_secrets --namespace "$ns" | jq -r '.items[] | "  \\(.name) (\\(.type))"'
      echo ""
    done
  `);
  console.log(r.stdout);
}

// =============================================================================
// Main
// =============================================================================

async function main() {
  const tool = buildK8sTool();

  // Show what the LLM sees
  console.log(`Tool: ${tool.name} (${tool.toolCount()} commands)\n`);
  console.log("--- System prompt (sent to LLM) ---");
  console.log(tool.systemPrompt());

  // Run demos
  await runDemos(tool);

  console.log("=".repeat(70));
  console.log("Done.");
}

main().then(() => process.exit(0));
