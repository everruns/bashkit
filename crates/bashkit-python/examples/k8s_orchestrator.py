#!/usr/bin/env python3
"""Kubernetes API orchestrator using Bashkit ScriptedTool.

Demonstrates composing 12 fake k8s API tools into a single ScriptedTool that
an LLM agent can call with bash scripts. Each tool becomes a bash builtin;
the agent writes one script to orchestrate them all.

Run directly:
    cd crates/bashkit-python && maturin develop && python examples/k8s_orchestrator.py

With LangChain (optional):
    pip install 'bashkit[langchain]'
    ANTHROPIC_API_KEY=... python examples/k8s_orchestrator.py --langchain
"""

from __future__ import annotations

import json
import sys

from bashkit import ScriptedTool

# =============================================================================
# Fake k8s data
# =============================================================================

NODES = [
    {"name": "node-1", "status": "Ready", "cpu": "4", "memory": "16Gi", "pods": 23},
    {"name": "node-2", "status": "Ready", "cpu": "8", "memory": "32Gi", "pods": 41},
    {"name": "node-3", "status": "NotReady", "cpu": "4", "memory": "16Gi", "pods": 0},
]

NAMESPACES = [
    {"name": "default", "status": "Active"},
    {"name": "kube-system", "status": "Active"},
    {"name": "monitoring", "status": "Active"},
    {"name": "production", "status": "Active"},
]

PODS = {
    "default": [
        {"name": "web-abc12", "status": "Running", "restarts": 0, "node": "node-1", "image": "nginx:1.25"},
        {"name": "api-def34", "status": "Running", "restarts": 2, "node": "node-2", "image": "api:v2.1"},
        {
            "name": "worker-ghi56",
            "status": "CrashLoopBackOff",
            "restarts": 15,
            "node": "node-2",
            "image": "worker:v1.0",
        },
    ],
    "kube-system": [
        {"name": "coredns-aaa11", "status": "Running", "restarts": 0, "node": "node-1", "image": "coredns:1.11"},
        {"name": "etcd-bbb22", "status": "Running", "restarts": 0, "node": "node-1", "image": "etcd:3.5"},
    ],
    "monitoring": [
        {"name": "prometheus-ccc33", "status": "Running", "restarts": 0, "node": "node-2", "image": "prom:2.48"},
        {"name": "grafana-ddd44", "status": "Running", "restarts": 1, "node": "node-2", "image": "grafana:10.2"},
    ],
    "production": [
        {"name": "app-eee55", "status": "Running", "restarts": 0, "node": "node-1", "image": "app:v3.2"},
        {"name": "app-fff66", "status": "Running", "restarts": 0, "node": "node-2", "image": "app:v3.2"},
        {"name": "db-ggg77", "status": "Pending", "restarts": 0, "node": "", "image": "postgres:16"},
    ],
}

DEPLOYMENTS = {
    "default": [
        {"name": "web", "replicas": 1, "available": 1, "image": "nginx:1.25"},
        {"name": "api", "replicas": 2, "available": 2, "image": "api:v2.1"},
        {"name": "worker", "replicas": 1, "available": 0, "image": "worker:v1.0"},
    ],
    "production": [
        {"name": "app", "replicas": 2, "available": 2, "image": "app:v3.2"},
        {"name": "db", "replicas": 1, "available": 0, "image": "postgres:16"},
    ],
}

SERVICES = {
    "default": [
        {"name": "web-svc", "type": "LoadBalancer", "clusterIP": "10.0.0.10", "ports": "80/TCP"},
        {"name": "api-svc", "type": "ClusterIP", "clusterIP": "10.0.0.20", "ports": "8080/TCP"},
    ],
    "production": [
        {"name": "app-svc", "type": "LoadBalancer", "clusterIP": "10.0.1.10", "ports": "443/TCP"},
    ],
}

CONFIGMAPS = {
    "default": [{"name": "app-config", "data_keys": ["DATABASE_URL", "LOG_LEVEL", "CACHE_TTL"]}],
    "production": [{"name": "prod-config", "data_keys": ["DATABASE_URL", "REDIS_URL"]}],
}

EVENTS = [
    {
        "namespace": "default",
        "type": "Warning",
        "reason": "BackOff",
        "object": "pod/worker-ghi56",
        "message": "Back-off restarting failed container",
    },
    {
        "namespace": "production",
        "type": "Warning",
        "reason": "FailedScheduling",
        "object": "pod/db-ggg77",
        "message": "Insufficient memory on available nodes",
    },
    {
        "namespace": "default",
        "type": "Normal",
        "reason": "Pulled",
        "object": "pod/api-def34",
        "message": "Successfully pulled image api:v2.1",
    },
    {
        "namespace": "monitoring",
        "type": "Normal",
        "reason": "Started",
        "object": "pod/prometheus-ccc33",
        "message": "Started container prometheus",
    },
]

LOGS = {
    "web-abc12": ("2024-01-15T10:00:01Z GET /health 200 1ms\n2024-01-15T10:00:02Z GET /api/users 200 45ms\n"),
    "api-def34": (
        "2024-01-15T10:00:01Z INFO  Starting API server on :8080\n"
        "2024-01-15T10:00:02Z WARN  High latency detected: 250ms\n"
    ),
    "worker-ghi56": (
        "2024-01-15T10:00:01Z ERROR Connection refused: redis://redis:6379\n"
        "2024-01-15T10:00:02Z FATAL Exiting due to unrecoverable error\n"
    ),
}

# Track mutable state for scale operations
_deployment_state: dict[str, dict[str, int]] = {}


# =============================================================================
# Tool callbacks — each receives (params: dict, stdin: str | None) -> str
# =============================================================================


def get_nodes(params, stdin=None):
    """Return cluster nodes."""
    return json.dumps({"items": NODES}) + "\n"


def get_namespaces(params, stdin=None):
    """Return namespaces."""
    return json.dumps({"items": NAMESPACES}) + "\n"


def get_pods(params, stdin=None):
    """Return pods in namespace."""
    ns = params.get("namespace", "default")
    pods = PODS.get(ns, [])
    return json.dumps({"items": pods}) + "\n"


def get_deployments(params, stdin=None):
    """Return deployments in namespace."""
    ns = params.get("namespace", "default")
    deps = DEPLOYMENTS.get(ns, [])
    return json.dumps({"items": deps}) + "\n"


def get_services(params, stdin=None):
    """Return services in namespace."""
    ns = params.get("namespace", "default")
    svcs = SERVICES.get(ns, [])
    return json.dumps({"items": svcs}) + "\n"


def describe_pod(params, stdin=None):
    """Describe a specific pod."""
    name = params.get("name", "")
    ns = params.get("namespace", "default")
    for pod in PODS.get(ns, []):
        if pod["name"] == name:
            detail = {**pod, "namespace": ns, "labels": {"app": name.rsplit("-", 1)[0]}}
            return json.dumps(detail) + "\n"
    raise ValueError(f"pod {name} not found in {ns}")


def get_logs(params, stdin=None):
    """Get pod logs."""
    name = params.get("name", "")
    tail = params.get("tail", 50)
    logs = LOGS.get(name, f"No logs available for {name}\n")
    lines = logs.strip().split("\n")
    return "\n".join(lines[-int(tail) :]) + "\n"


def get_configmaps(params, stdin=None):
    """List configmaps in namespace."""
    ns = params.get("namespace", "default")
    cms = CONFIGMAPS.get(ns, [])
    return json.dumps({"items": cms}) + "\n"


def get_secrets(params, stdin=None):
    """List secrets (redacted)."""
    ns = params.get("namespace", "default")
    secrets = [{"name": f"{ns}-tls", "type": "kubernetes.io/tls", "data": "***REDACTED***"}]
    return json.dumps({"items": secrets}) + "\n"


def get_events(params, stdin=None):
    """Get cluster events, optionally filtered by namespace."""
    ns = params.get("namespace")
    items = EVENTS if not ns else [e for e in EVENTS if e["namespace"] == ns]
    return json.dumps({"items": items}) + "\n"


def scale_deployment(params, stdin=None):
    """Scale a deployment."""
    name = params.get("name", "")
    ns = params.get("namespace", "default")
    replicas = params.get("replicas", 1)
    key = f"{ns}/{name}"
    _deployment_state[key] = {"replicas": int(replicas)}
    return json.dumps({"deployment": name, "namespace": ns, "replicas": int(replicas), "status": "scaling"}) + "\n"


def rollout_status(params, stdin=None):
    """Check rollout status of a deployment."""
    name = params.get("name", "")
    ns = params.get("namespace", "default")
    key = f"{ns}/{name}"
    if key in _deployment_state:
        r = _deployment_state[key]["replicas"]
        return json.dumps({"deployment": name, "status": "progressing", "replicas": r, "updated": r}) + "\n"
    for dep in DEPLOYMENTS.get(ns, []):
        if dep["name"] == name:
            status = "available" if dep["available"] == dep["replicas"] else "progressing"
            return json.dumps({"deployment": name, "status": status, **dep}) + "\n"
    raise ValueError(f"deployment {name} not found in {ns}")


# =============================================================================
# Build the ScriptedTool with all 12 k8s commands
# =============================================================================


def build_k8s_tool() -> ScriptedTool:
    tool = ScriptedTool("kubectl", short_description="Kubernetes cluster management API")

    tool.add_tool("get_nodes", "List cluster nodes", callback=get_nodes)

    tool.add_tool("get_namespaces", "List namespaces", callback=get_namespaces)

    tool.add_tool(
        "get_pods",
        "List pods in a namespace",
        callback=get_pods,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Namespace"}}},
    )

    tool.add_tool(
        "get_deployments",
        "List deployments in a namespace",
        callback=get_deployments,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Namespace"}}},
    )

    tool.add_tool(
        "get_services",
        "List services in a namespace",
        callback=get_services,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Namespace"}}},
    )

    tool.add_tool(
        "describe_pod",
        "Describe a specific pod",
        callback=describe_pod,
        schema={
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Pod name"},
                "namespace": {"type": "string", "description": "Namespace"},
            },
            "required": ["name"],
        },
    )

    tool.add_tool(
        "get_logs",
        "Get pod logs",
        callback=get_logs,
        schema={
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Pod name"},
                "tail": {"type": "integer", "description": "Number of lines"},
            },
            "required": ["name"],
        },
    )

    tool.add_tool(
        "get_configmaps",
        "List configmaps in a namespace",
        callback=get_configmaps,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Namespace"}}},
    )

    tool.add_tool(
        "get_secrets",
        "List secrets in a namespace (values redacted)",
        callback=get_secrets,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Namespace"}}},
    )

    tool.add_tool(
        "get_events",
        "Get cluster events",
        callback=get_events,
        schema={"type": "object", "properties": {"namespace": {"type": "string", "description": "Filter namespace"}}},
    )

    tool.add_tool(
        "scale_deployment",
        "Scale a deployment to N replicas",
        callback=scale_deployment,
        schema={
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Deployment name"},
                "namespace": {"type": "string", "description": "Namespace"},
                "replicas": {"type": "integer", "description": "Target replica count"},
            },
            "required": ["name", "replicas"],
        },
    )

    tool.add_tool(
        "rollout_status",
        "Check deployment rollout status",
        callback=rollout_status,
        schema={
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Deployment name"},
                "namespace": {"type": "string", "description": "Namespace"},
            },
            "required": ["name"],
        },
    )

    tool.env("KUBECONFIG", "/etc/kubernetes/admin.conf")
    return tool


# =============================================================================
# Demo scripts — what an LLM agent would generate
# =============================================================================


def run_demos(tool: ScriptedTool) -> None:
    print("=" * 70)
    print("Kubernetes Orchestrator - 12 tools via ScriptedTool")
    print("=" * 70)

    # -- Demo 1: Simple listing --
    print("\n--- Demo 1: List all nodes ---")
    r = tool.execute_sync("get_nodes | jq -r '.items[] | \"\\(.name)  \\(.status)  cpu=\\(.cpu)  mem=\\(.memory)\"'")
    print(r.stdout)

    # -- Demo 2: Unhealthy pods across all namespaces --
    print("--- Demo 2: Find unhealthy pods across namespaces ---")
    r = tool.execute_sync("""
        get_namespaces | jq -r '.items[].name' | while read ns; do
            get_pods --namespace "$ns" \
                | jq -r '.items[] | select(.status != "Running") | .name + " " + .status' \
                | while read line; do echo "  $ns/$line"; done
        done
    """)
    print(r.stdout)

    # -- Demo 3: Cluster health report --
    print("--- Demo 3: Full cluster health report ---")
    r = tool.execute_sync("""
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
    """)
    print(r.stdout)

    # -- Demo 4: Diagnose CrashLoopBackOff --
    print("--- Demo 4: Diagnose crashing pod ---")
    r = tool.execute_sync("""
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
    """)
    print(r.stdout)

    # -- Demo 5: Scale + rollout --
    print("--- Demo 5: Scale deployment and check rollout ---")
    r = tool.execute_sync("""
        echo "Scaling 'app' in production to 5 replicas..."
        scale_deployment --name app --namespace production --replicas 5 | jq '.'
        echo ""
        echo "Rollout status:"
        rollout_status --name app --namespace production | jq '.'
    """)
    print(r.stdout)

    # -- Demo 6: Service + configmap inventory --
    print("--- Demo 6: Namespace inventory ---")
    r = tool.execute_sync("""
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
    """)
    print(r.stdout)


# =============================================================================
# LangChain integration (optional)
# =============================================================================


def run_langchain_demo(tool: ScriptedTool) -> None:
    """Demo: wrap ScriptedTool as LangChain tool for a ReAct agent."""
    try:
        from langchain_anthropic import ChatAnthropic
        from langgraph.prebuilt import create_react_agent

        from bashkit.langchain import create_scripted_tool
    except ImportError as e:
        print(f"\nSkipping LangChain demo (missing dependency: {e})")
        print("Install with: pip install 'bashkit[langchain]' langgraph")
        return

    print("\n" + "=" * 70)
    print("LangChain ReAct Agent Demo")
    print("=" * 70)

    # Wrap our k8s ScriptedTool as a LangChain tool
    lc_tool = create_scripted_tool(tool)
    print(f"\nLangChain tool: name={lc_tool.name!r}, tools={tool.tool_count()}")

    # Create agent with Claude
    model = ChatAnthropic(model="claude-sonnet-4-20250514", temperature=0)
    agent = create_react_agent(model, [lc_tool])

    # Ask the agent to investigate the cluster
    query = "Check the cluster health. Find any pods that are not running and diagnose why they're failing."
    print(f"\nUser: {query}\n")

    result = agent.invoke({"messages": [{"role": "user", "content": query}]})
    # Print the final assistant message
    for msg in result["messages"]:
        if hasattr(msg, "content") and msg.type == "ai" and msg.content:
            print(f"Agent: {msg.content}")


# =============================================================================
# Main
# =============================================================================


def main():
    tool = build_k8s_tool()

    # Show what the LLM sees
    print(f"Tool: {tool.name} ({tool.tool_count()} commands)\n")
    print("--- System prompt (sent to LLM) ---")
    print(tool.system_prompt())

    # Run direct demos
    run_demos(tool)

    # LangChain demo if requested
    if "--langchain" in sys.argv:
        run_langchain_demo(tool)

    print("=" * 70)
    print("Done.")


if __name__ == "__main__":
    main()
