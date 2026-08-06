// One ToolDef-backed registry owns dispatch for shell, Python, and TypeScript.
// Surface adapters only translate arguments/results; policy, validation,
// deadlines, sanitization, callback identity, and tracing stay here.

use crate::builtins::Context;
#[cfg(not(target_family = "wasm"))]
use crate::builtins::ExecutionDeadline;
use crate::scripted_tool::{
    CallbackKind, RegisteredTool, ScriptedCommandKind, ToolDefInvocationTrace,
};
use crate::{ToolArgs, ToolDef, ToolImpl};
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

/// Runtime surface that initiated a registry call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallSurface {
    Shell,
    Python,
    TypeScript,
}

/// Result of the registry's approval/policy hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallDecision {
    Allow,
    Deny,
}

/// Immutable data supplied to the approval/policy hook.
pub struct ToolCall<'a> {
    name: &'a str,
    params: &'a Value,
    tenant_id: Option<&'a str>,
    surface: ToolCallSurface,
}

impl ToolCall<'_> {
    pub fn name(&self) -> &str {
        self.name
    }

    pub fn params(&self) -> &Value {
        self.params
    }

    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id
    }

    pub fn surface(&self) -> ToolCallSurface {
        self.surface
    }
}

type PolicyHook = Arc<dyn Fn(&ToolCall<'_>) -> ToolCallDecision + Send + Sync>;

/// Per-`exec*` registry context carried through [`crate::ExecutionExtensions`].
///
/// Each request owns a trace sink, preventing cross-tenant trace mixing even
/// when tenants share one immutable registry and callback set.
#[derive(Clone)]
// THREAT[TM-ISO-026]: tenant identity and trace are request-owned, never registry state.
pub struct ToolCallRequest {
    tenant_id: String,
    trace: ToolDefInvocationTrace,
}

impl ToolCallRequest {
    pub fn new(tenant_id: impl Into<String>, trace: ToolDefInvocationTrace) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            trace,
        }
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn trace(&self) -> ToolDefInvocationTrace {
        self.trace.clone()
    }
}

#[derive(Clone)]
pub(crate) struct ToolCallScope {
    request: Option<crate::ExecutionCapability<ToolCallRequest>>,
    budget: Option<crate::limits::ExecutionBudget>,
    lease: Option<crate::ExecutionCapability<()>>,
    #[cfg(not(target_family = "wasm"))]
    deadline: Option<crate::time_compat::Instant>,
}

impl ToolCallScope {
    pub(crate) fn from_context(ctx: &Context<'_>) -> Self {
        Self {
            request: ctx.execution_extension::<ToolCallRequest>(),
            budget: ctx
                .execution_budget()
                .and_then(|budget| budget.try_with(Clone::clone).ok()),
            lease: ctx.execution_capability(()),
            #[cfg(not(target_family = "wasm"))]
            deadline: ctx
                .execution_extension::<ExecutionDeadline>()
                .and_then(|deadline| {
                    deadline
                        .try_with(ExecutionDeadline::remaining)
                        .ok()
                        .and_then(|remaining| {
                            crate::time_compat::Instant::now().checked_add(remaining)
                        })
                }),
        }
    }

    fn tenant_id(&self) -> Result<Option<String>, crate::ExecutionCapabilityError> {
        self.request
            .as_ref()
            .map(|request| request.try_with(|request| request.tenant_id().to_string()))
            .transpose()
    }

    fn trace(&self) -> Result<Option<ToolDefInvocationTrace>, crate::ExecutionCapabilityError> {
        self.request
            .as_ref()
            .map(|request| request.try_with(ToolCallRequest::trace))
            .transpose()
    }

    #[cfg(not(target_family = "wasm"))]
    fn remaining(&self) -> Option<std::time::Duration> {
        self.deadline.map(|deadline| {
            deadline
                .checked_duration_since(crate::time_compat::Instant::now())
                .unwrap_or_default()
        })
    }
}

#[cfg(any(feature = "python", feature = "typescript"))]
tokio::task_local! {
    static RUNTIME_TOOL_SCOPE: ToolCallScope;
}

#[cfg(any(feature = "python", feature = "typescript"))]
pub(crate) async fn scope_runtime_call<F: Future>(scope: ToolCallScope, future: F) -> F::Output {
    RUNTIME_TOOL_SCOPE.scope(scope, future).await
}

#[cfg(any(feature = "python", feature = "typescript"))]
fn runtime_scope() -> ToolCallScope {
    RUNTIME_TOOL_SCOPE
        .try_with(Clone::clone)
        .unwrap_or(ToolCallScope {
            request: None,
            budget: None,
            lease: None,
            #[cfg(not(target_family = "wasm"))]
            deadline: None,
        })
}

#[derive(Clone)]
pub struct ToolRegistry {
    inner: Arc<ToolRegistryInner>,
}

struct ToolRegistryInner {
    tools: Vec<RegisteredTool>,
    policy: PolicyHook,
    sanitize_errors: bool,
    default_trace: ToolDefInvocationTrace,
}

pub struct ToolRegistryBuilder {
    tools: Vec<RegisteredTool>,
    policy: PolicyHook,
    sanitize_errors: bool,
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            policy: Arc::new(|_| ToolCallDecision::Allow),
            sanitize_errors: true,
        }
    }
}

impl ToolRegistryBuilder {
    pub fn tool(mut self, tool: ToolImpl) -> Self {
        self.tools.push(RegisteredTool::from_tool_impl(tool));
        self
    }

    pub fn tool_fn(
        mut self,
        def: ToolDef,
        exec: impl Fn(&ToolArgs) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Sync(Arc::new(exec)),
            dry_run: None,
        });
        self
    }

    pub fn async_tool_fn<F, Fut>(mut self, def: ToolDef, exec: F) -> Self
    where
        F: Fn(ToolArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, String>> + Send + 'static,
    {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Async(Arc::new(move |args| Box::pin(exec(args)))),
            dry_run: None,
        });
        self
    }

    pub fn policy(
        mut self,
        policy: impl Fn(&ToolCall<'_>) -> ToolCallDecision + Send + Sync + 'static,
    ) -> Self {
        self.policy = Arc::new(policy);
        self
    }

    pub fn sanitize_errors(mut self, sanitize: bool) -> Self {
        self.sanitize_errors = sanitize;
        self
    }

    pub fn build(&self) -> ToolRegistry {
        for tool in &self.tools {
            assert!(
                valid_runtime_name(&tool.def.name),
                "ToolDef name must contain dot-separated identifier segments"
            );
        }
        ToolRegistry {
            inner: Arc::new(ToolRegistryInner {
                tools: self.tools.clone(),
                policy: self.policy.clone(),
                sanitize_errors: self.sanitize_errors,
                default_trace: ToolDefInvocationTrace::new(),
            }),
        }
    }
}

pub(crate) struct RegistryOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) exit_code: i32,
}

impl ToolRegistry {
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::default()
    }

    pub(crate) fn from_registered(
        tools: Vec<RegisteredTool>,
        sanitize_errors: bool,
        trace: ToolDefInvocationTrace,
    ) -> Self {
        Self {
            inner: Arc::new(ToolRegistryInner {
                tools,
                policy: Arc::new(|_| ToolCallDecision::Allow),
                sanitize_errors,
                default_trace: trace,
            }),
        }
    }

    pub(crate) fn with_fresh_default_trace(&self) -> Self {
        Self {
            inner: Arc::new(ToolRegistryInner {
                tools: self.inner.tools.clone(),
                policy: self.inner.policy.clone(),
                sanitize_errors: self.inner.sanitize_errors,
                default_trace: ToolDefInvocationTrace::new(),
            }),
        }
    }

    /// Create a fresh trace sink suitable for one request/tenant.
    pub fn trace(&self) -> ToolDefInvocationTrace {
        ToolDefInvocationTrace::new()
    }

    pub(crate) fn tools(&self) -> &[RegisteredTool] {
        &self.inner.tools
    }

    pub(crate) fn sanitize_errors(&self) -> bool {
        self.inner.sanitize_errors
    }

    pub(crate) fn default_trace(&self) -> ToolDefInvocationTrace {
        self.inner.default_trace.clone()
    }

    pub(crate) async fn invoke_from_context(
        &self,
        name: &str,
        params: Value,
        stdin: Option<String>,
        surface: ToolCallSurface,
        ctx: &Context<'_>,
    ) -> RegistryOutput {
        self.invoke(
            name,
            params,
            stdin,
            surface,
            ToolCallScope::from_context(ctx),
            ctx.args.to_vec(),
        )
        .await
    }

    #[cfg(any(feature = "python", feature = "typescript"))]
    pub(crate) async fn invoke_runtime(
        &self,
        name: &str,
        params: Value,
        surface: ToolCallSurface,
    ) -> RegistryOutput {
        self.invoke(name, params, None, surface, runtime_scope(), Vec::new())
            .await
    }

    async fn invoke(
        &self,
        name: &str,
        params: Value,
        stdin: Option<String>,
        surface: ToolCallSurface,
        scope: ToolCallScope,
        trace_args: Vec<String>,
    ) -> RegistryOutput {
        let Some(tool) = self.inner.tools.iter().find(|tool| tool.def.name == name) else {
            return RegistryOutput::error(format!("{name}: unknown tool\n"), 127);
        };
        let Some(lease) = scope.lease.as_ref() else {
            return RegistryOutput::error(
                "tool callback: execution capability revoked\n".into(),
                1,
            );
        };
        let trace = match scope.trace() {
            Ok(Some(trace)) => trace,
            Ok(None) => self.default_trace(),
            Err(_) => {
                return RegistryOutput::error(
                    "tool callback: execution capability revoked\n".into(),
                    1,
                );
            }
        };
        let tenant_id = match scope.tenant_id() {
            Ok(tenant_id) => tenant_id,
            Err(_) => {
                return RegistryOutput::error(
                    "tool callback: execution capability revoked\n".into(),
                    1,
                );
            }
        };

        if let Err(error) = validate_schema(&params, &tool.def.input_schema, "input") {
            trace.push(name, ScriptedCommandKind::Tool, trace_args, 2);
            return RegistryOutput::error(format!("{name}: schema error: {error}\n"), 2);
        }

        let call = ToolCall {
            name,
            params: &params,
            tenant_id: tenant_id.as_deref(),
            surface,
        };
        if (self.inner.policy)(&call) == ToolCallDecision::Deny {
            trace.push(name, ScriptedCommandKind::Tool, trace_args, 1);
            return RegistryOutput::error(format!("{name}: denied by policy\n"), 1);
        }

        let context = crate::tool_def::ToolArgsContextData::new(tenant_id, surface);
        let Some(context) = lease.derive(context) else {
            return RegistryOutput::error(
                "tool callback: execution capability revoked\n".into(),
                1,
            );
        };
        let args = ToolArgs::with_context(params, stdin, context);
        let callback = async {
            match &tool.callback {
                CallbackKind::Sync(callback) => callback(&args),
                CallbackKind::Async(callback) => callback(args).await,
            }
        };
        let callback = async {
            match &scope.budget {
                Some(budget) => budget
                    .run(callback)
                    .await
                    .map_err(|_| InvokeError::Lifecycle)?
                    .map_err(InvokeError::Callback),
                None => callback.await.map_err(InvokeError::Callback),
            }
        };
        #[cfg(not(target_family = "wasm"))]
        let result = if let Some(remaining) = scope.remaining() {
            match tokio::time::timeout(remaining, callback).await {
                Ok(result) => result,
                Err(_) => Err(InvokeError::Timeout),
            }
        } else {
            callback.await
        };
        #[cfg(target_family = "wasm")]
        let result = callback.await;

        let output = match result {
            Ok(stdout) => RegistryOutput {
                stdout,
                stderr: String::new(),
                exit_code: 0,
            },
            #[cfg(not(target_family = "wasm"))]
            Err(InvokeError::Timeout) => {
                RegistryOutput::error(format!("{name}: callback timeout\n"), 1)
            }
            Err(InvokeError::Callback(_error)) if self.inner.sanitize_errors => {
                // THREAT[TM-INF-030]: callback diagnostics can contain secrets.
                #[cfg(feature = "tracing")]
                tracing::debug!(tool = name, error = %_error, "registry callback error (sanitized)");
                RegistryOutput::error(format!("{name}: callback failed\n"), 1)
            }
            Err(InvokeError::Callback(error)) => RegistryOutput::error(error, 1),
            Err(InvokeError::Lifecycle) => {
                RegistryOutput::error(format!("{name}: request cancelled\n"), 1)
            }
        };
        trace.push(
            name,
            ScriptedCommandKind::Tool,
            trace_args,
            output.exit_code,
        );
        output
    }

    #[cfg(any(feature = "python", feature = "typescript"))]
    pub(crate) fn discover(&self, params: &Value) -> Value {
        let category = params.get("category").and_then(Value::as_str);
        let tag = params.get("tag").and_then(Value::as_str);
        let search = params
            .get("search")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        Value::Array(
            self.inner
                .tools
                .iter()
                .filter(|tool| category.is_none_or(|v| tool.def.category.as_deref() == Some(v)))
                .filter(|tool| tag.is_none_or(|v| tool.def.tags.iter().any(|tag| tag == v)))
                .filter(|tool| {
                    search.as_ref().is_none_or(|search| {
                        tool.def.name.to_lowercase().contains(search)
                            || tool.def.description.to_lowercase().contains(search)
                    })
                })
                .map(|tool| {
                    serde_json::json!({
                        "name": tool.def.name,
                        "description": tool.def.description,
                        "input_schema": tool.def.input_schema,
                        "tags": tool.def.tags,
                        "category": tool.def.category,
                    })
                })
                .collect(),
        )
    }

    #[cfg(any(feature = "python", feature = "typescript"))]
    fn discover_runtime(&self, params: &Value) -> Value {
        let scope = runtime_scope();
        let trace = match scope.trace() {
            Ok(Some(trace)) => trace,
            Ok(None) => self.default_trace(),
            Err(_) => return Value::Array(Vec::new()),
        };
        trace.push("discover", ScriptedCommandKind::Discover, Vec::new(), 0);
        self.discover(params)
    }

    #[cfg(feature = "typescript")]
    pub(crate) fn typescript_handler(&self) -> crate::TypeScriptExternalFnHandler {
        let registry = self.clone();
        Arc::new(move |external_name, args| {
            let registry = registry.clone();
            Box::pin(async move {
                let params = match args.first() {
                    Some(zapcode_core::Value::String(json)) => serde_json::from_str(json)
                        .map_err(|_| "tool input must be valid JSON".to_string())?,
                    Some(value) => zapcode_to_json(value)?,
                    None => serde_json::json!({}),
                };
                if external_name == "__bashkit_discover" {
                    return Ok(json_to_zapcode(registry.discover_runtime(&params)));
                }
                let Some(tool) = registry
                    .inner
                    .tools
                    .iter()
                    .find(|tool| typescript_function_name(&tool.def.name) == external_name)
                else {
                    return Err("unknown registry tool".to_string());
                };
                let output = registry
                    .invoke_runtime(&tool.def.name, params, ToolCallSurface::TypeScript)
                    .await;
                if output.exit_code != 0 {
                    return Err(output.stderr.trim().to_string());
                }
                let value =
                    serde_json::from_str(&output.stdout).unwrap_or(Value::String(output.stdout));
                Ok(json_to_zapcode(value))
            })
        })
    }

    #[cfg(feature = "python")]
    pub(crate) fn python_handler(&self) -> crate::PythonExternalFnHandler {
        use monty_types::{ExcType, ExtFunctionResult, MontyException, MontyObject};
        let registry = self.clone();
        Arc::new(move |_external_name, args, _kwargs| {
            let registry = registry.clone();
            Box::pin(async move {
                let name = args.first().and_then(|value| match value {
                    MontyObject::String(name) => Some(name.clone()),
                    _ => None,
                });
                let params = args
                    .get(1)
                    .map(monty_to_json)
                    .transpose()
                    .unwrap_or(None)
                    .unwrap_or_else(|| serde_json::json!({}));
                if name.as_deref() == Some("__discover") {
                    return ExtFunctionResult::Return(json_to_monty(
                        registry.discover_runtime(&params),
                    ));
                }
                let Some(name) = name else {
                    return ExtFunctionResult::Error(MontyException::new(
                        ExcType::RuntimeError,
                        Some("invalid tool call".into()),
                    ));
                };
                let output = registry
                    .invoke_runtime(&name, params, ToolCallSurface::Python)
                    .await;
                if output.exit_code == 0 {
                    let value = serde_json::from_str(&output.stdout)
                        .unwrap_or(Value::String(output.stdout));
                    ExtFunctionResult::Return(json_to_monty(value))
                } else {
                    ExtFunctionResult::Error(MontyException::new(
                        ExcType::RuntimeError,
                        Some(output.stderr.trim().to_string()),
                    ))
                }
            })
        })
    }

    #[cfg(feature = "typescript")]
    pub(crate) fn typescript_prelude(&self) -> String {
        String::new()
    }

    #[cfg(feature = "typescript")]
    pub(crate) fn typescript_rewrites(&self) -> Vec<(String, String)> {
        let mut rewrites = self
            .inner
            .tools
            .iter()
            .map(|tool| {
                (
                    format!("tools.{}", tool.def.name),
                    typescript_function_name(&tool.def.name),
                )
            })
            .collect::<Vec<_>>();
        rewrites.push(("tools.discover".into(), "__bashkit_discover".into()));
        rewrites
    }

    #[cfg(feature = "typescript")]
    pub(crate) fn typescript_external_names(&self) -> Vec<String> {
        let mut names = self
            .inner
            .tools
            .iter()
            .map(|tool| typescript_function_name(&tool.def.name))
            .collect::<Vec<_>>();
        names.push("__bashkit_discover".to_string());
        names
    }

    #[cfg(feature = "python")]
    pub(crate) fn python_prelude(&self) -> String {
        let mut root = serde_json::Map::new();
        for tool in &self.inner.tools {
            insert_runtime_tree(&mut root, &tool.def.name);
        }
        let mut classes = String::new();
        let root_class = python_tree(&root, "Tools", &mut classes);
        format!("{classes}\ntools = {root_class}()\n")
    }
}

impl RegistryOutput {
    fn error(stderr: String, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr,
            exit_code,
        }
    }
}

enum InvokeError {
    Callback(String),
    Lifecycle,
    #[cfg(not(target_family = "wasm"))]
    Timeout,
}

fn valid_runtime_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
                && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
        })
}

fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if schema.as_object().is_none_or(serde_json::Map::is_empty) {
        return Ok(());
    }
    if let Some(expected) = schema.get("type") {
        let types = expected
            .as_str()
            .into_iter()
            .chain(
                expected
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
            .collect::<Vec<_>>();
        if !types.is_empty() && !types.iter().any(|expected| value_has_type(value, expected)) {
            return Err(format!("{path} must be {}", types.join(" or ")));
        }
    }
    for keyword in ["oneOf", "anyOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array)
            && !branches
                .iter()
                .any(|branch| validate_schema(value, branch, path).is_ok())
        {
            return Err(format!("{path} does not match any {keyword} branch"));
        }
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            validate_schema(value, branch, path)?;
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        return Err(format!("{path} is not an allowed value"));
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("required property '{key}' is missing"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in object.keys() {
                if properties.is_none_or(|properties| !properties.contains_key(key)) {
                    return Err(format!("unknown property '{key}'"));
                }
            }
        }
        if let Some(properties) = properties {
            for (key, child) in object {
                if let Some(child_schema) = properties.get(key) {
                    validate_schema(child, child_schema, &format!("{path}.{key}"))?;
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, child) in values.iter().enumerate() {
            validate_schema(child, items, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn value_has_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

#[cfg(feature = "python")]
fn insert_runtime_tree(root: &mut serde_json::Map<String, Value>, name: &str) {
    let mut current = root;
    let mut segments = name.split('.').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current.insert(segment.to_string(), Value::String(name.to_string()));
            return;
        }
        current = current
            .entry(segment)
            .or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
            .expect("tool namespace collision");
    }
}

#[cfg(feature = "typescript")]
fn typescript_function_name(name: &str) -> String {
    format!("__bashkit_{}", name.replace('.', "_"))
}

#[cfg(feature = "python")]
fn python_tree(
    tree: &serde_json::Map<String, Value>,
    class_name: &str,
    classes: &mut String,
) -> String {
    let mut nested = Vec::new();
    for (name, value) in tree {
        if let Value::Object(children) = value {
            let child_name = format!("{class_name}_{name}");
            python_tree(children, &child_name, classes);
            nested.push((name, child_name));
        }
    }
    classes.push_str(&format!("class {class_name}:\n"));
    if tree.is_empty() {
        classes.push_str("    pass\n");
    }
    for (name, child_name) in nested {
        classes.push_str(&format!("    {name} = {child_name}()\n"));
    }
    for (name, value) in tree {
        if let Value::String(full_name) = value {
            classes.push_str(&format!(
                "    def {name}(self, args={{}}):\n        return __bashkit_tool_call('{full_name}', args)\n"
            ));
        }
    }
    if class_name == "Tools" {
        classes.push_str(
            "    def discover(self, args={}):\n        return __bashkit_tool_call('__discover', args)\n",
        );
    }
    class_name.to_string()
}

#[cfg(feature = "typescript")]
fn zapcode_to_json(value: &zapcode_core::Value) -> Result<Value, String> {
    Ok(match value {
        zapcode_core::Value::Undefined | zapcode_core::Value::Null => Value::Null,
        zapcode_core::Value::Bool(value) => Value::Bool(*value),
        zapcode_core::Value::Int(value) => Value::from(*value),
        zapcode_core::Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| "non-finite number is not valid tool input".to_string())?,
        zapcode_core::Value::String(value) => Value::String(value.to_string()),
        zapcode_core::Value::Array(values) => Value::Array(
            values
                .iter()
                .map(zapcode_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        zapcode_core::Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.to_string(), zapcode_to_json(value)?)))
                .collect::<Result<serde_json::Map<_, _>, String>>()?,
        ),
        _ => return Err("functions are not valid tool input".to_string()),
    })
}

#[cfg(feature = "typescript")]
fn json_to_zapcode(value: Value) -> zapcode_core::Value {
    match value {
        Value::Null => zapcode_core::Value::Null,
        Value::Bool(value) => zapcode_core::Value::Bool(value),
        Value::Number(value) => value
            .as_i64()
            .map(zapcode_core::Value::Int)
            .or_else(|| value.as_f64().map(zapcode_core::Value::Float))
            .unwrap_or(zapcode_core::Value::Null),
        Value::String(value) => zapcode_core::Value::String(Arc::from(value)),
        Value::Array(values) => {
            zapcode_core::Value::Array(values.into_iter().map(json_to_zapcode).collect())
        }
        Value::Object(values) => zapcode_core::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (Arc::from(key), json_to_zapcode(value)))
                .collect(),
        ),
    }
}

#[cfg(feature = "python")]
fn monty_to_json(value: &monty_types::MontyObject) -> Result<Value, String> {
    use monty_types::MontyObject;
    Ok(match value {
        MontyObject::None => Value::Null,
        MontyObject::Bool(value) => Value::Bool(*value),
        MontyObject::Int(value) => Value::from(*value),
        MontyObject::Float(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| "non-finite number is not valid tool input".to_string())?,
        MontyObject::String(value) | MontyObject::Path(value) => Value::String(value.clone()),
        MontyObject::List(values) | MontyObject::Tuple(values) => Value::Array(
            values
                .iter()
                .map(monty_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        MontyObject::Dict(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let MontyObject::String(key) = key else {
                        return Err("tool input object keys must be strings".to_string());
                    };
                    Ok((key.clone(), monty_to_json(value)?))
                })
                .collect::<Result<serde_json::Map<_, _>, String>>()?,
        ),
        _ => return Err("unsupported Python value in tool input".to_string()),
    })
}

#[cfg(feature = "python")]
fn json_to_monty(value: Value) -> monty_types::MontyObject {
    use monty_types::MontyObject;
    match value {
        Value::Null => MontyObject::None,
        Value::Bool(value) => MontyObject::Bool(value),
        Value::Number(value) => value
            .as_i64()
            .map(MontyObject::Int)
            .or_else(|| value.as_f64().map(MontyObject::Float))
            .unwrap_or(MontyObject::None),
        Value::String(value) => MontyObject::String(value),
        Value::Array(values) => MontyObject::List(values.into_iter().map(json_to_monty).collect()),
        Value::Object(values) => MontyObject::Dict(
            values
                .into_iter()
                .map(|(key, value)| (MontyObject::String(key), json_to_monty(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
    }
}
