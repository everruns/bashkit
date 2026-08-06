//! TM-ISO-027: host extension facilities are capabilities of one execution.

use bashkit::{
    Bash, Builtin, BuiltinContext, BuiltinRegistry, CommandResolver, ExecOptions, ExecResult,
    ExecutionCapability, ExecutionCapabilityError, ExecutionExtensions, FileSystem,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct RetainFs {
    retained: Arc<Mutex<Vec<Arc<dyn FileSystem>>>>,
}

#[bashkit::async_trait]
impl Builtin for RetainFs {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let content = ctx.fs.read_file(Path::new("/request.txt")).await?;
        self.retained.lock().unwrap().push(ctx.fs.clone());
        Ok(ExecResult::ok(content))
    }
}

#[tokio::test]
async fn scoped_vfs_works_during_execution_and_fails_after_completion() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let mut bash = Bash::builder()
        .builtin(
            "retain-fs",
            Box::new(RetainFs {
                retained: retained.clone(),
            }),
        )
        .build();
    bash.fs()
        .write_file(Path::new("/request.txt"), b"request-a")
        .await
        .unwrap();

    let result = bash.exec("retain-fs").await.unwrap();
    assert_eq!(result.stdout, "request-a");
    assert!(result.capability_cleanup.revoked);
    assert_eq!(result.capability_cleanup.failures, 0);

    let fs = retained.lock().unwrap().pop().unwrap();
    let error = fs.read_file(Path::new("/request.txt")).await.unwrap_err();
    assert_eq!(error.to_string(), "io error: execution capability revoked");
}

#[tokio::test]
async fn old_vfs_handle_cannot_cross_into_the_next_request() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let mut bash = Bash::builder()
        .builtin(
            "retain-fs",
            Box::new(RetainFs {
                retained: retained.clone(),
            }),
        )
        .build();
    bash.fs()
        .write_file(Path::new("/request.txt"), b"shared")
        .await
        .unwrap();

    bash.exec("retain-fs").await.unwrap();
    bash.exec("retain-fs").await.unwrap();
    let handles = retained.lock().unwrap().clone();

    for handle in handles {
        let error = handle
            .read_file(Path::new("/request.txt"))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "io error: execution capability revoked");
    }
}

struct PendingFs {
    retained: Arc<Mutex<Option<Arc<dyn FileSystem>>>>,
    entered: Arc<tokio::sync::Notify>,
}

#[bashkit::async_trait]
impl Builtin for PendingFs {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        *self.retained.lock().unwrap() = Some(ctx.fs.clone());
        self.entered.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn dropping_an_execution_future_revokes_retained_vfs_handles() {
    let retained = Arc::new(Mutex::new(None));
    let entered = Arc::new(tokio::sync::Notify::new());
    let bash = Bash::builder()
        .builtin(
            "pending-fs",
            Box::new(PendingFs {
                retained: retained.clone(),
                entered: entered.clone(),
            }),
        )
        .build();
    let task = tokio::spawn(async move {
        let mut bash = bash;
        bash.exec("pending-fs").await
    });
    entered.notified().await;
    task.abort();
    let _ = task.await;

    let fs = retained.lock().unwrap().clone().unwrap();
    let error = fs.read_file(Path::new("/request.txt")).await.unwrap_err();
    assert_eq!(error.to_string(), "io error: execution capability revoked");
}

#[derive(Debug)]
struct RequestSecret(&'static str);

struct RetainExtension {
    retained: Arc<Mutex<Option<ExecutionCapability<RequestSecret>>>>,
}

#[bashkit::async_trait]
impl Builtin for RetainExtension {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let capability = ctx
            .execution_extension::<RequestSecret>()
            .expect("request extension");
        let value = capability.try_with(|secret| secret.0.to_string()).unwrap();
        *self.retained.lock().unwrap() = Some(capability);
        Ok(ExecResult::ok(value))
    }
}

#[tokio::test]
async fn retained_typed_extension_fails_after_completion() {
    let retained = Arc::new(Mutex::new(None));
    let mut bash = Bash::builder()
        .builtin(
            "retain-extension",
            Box::new(RetainExtension {
                retained: retained.clone(),
            }),
        )
        .build();
    let result = bash
        .exec_with_options(
            "retain-extension",
            ExecOptions::new().extensions(ExecutionExtensions::new().with(RequestSecret("secret"))),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "secret");

    let capability = retained.lock().unwrap().clone().unwrap();
    assert_eq!(
        capability.try_with(|secret| secret.0),
        Err(ExecutionCapabilityError::Revoked)
    );
    assert_eq!(capability.cleanup_report().unwrap().failures, 0);
}

struct DropProbe(Arc<AtomicBool>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct RetainDropProbe {
    retained: Arc<Mutex<Option<ExecutionCapability<DropProbe>>>>,
}

#[bashkit::async_trait]
impl Builtin for RetainDropProbe {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        *self.retained.lock().unwrap() = ctx.execution_extension::<DropProbe>();
        Ok(ExecResult::ok("retained"))
    }
}

#[tokio::test]
async fn revocation_drops_extension_resources_even_when_the_handle_is_retained() {
    let dropped = Arc::new(AtomicBool::new(false));
    let retained = Arc::new(Mutex::new(None));
    let mut bash = Bash::builder()
        .builtin(
            "retain-drop-probe",
            Box::new(RetainDropProbe {
                retained: retained.clone(),
            }),
        )
        .build();

    bash.exec_with_options(
        "retain-drop-probe",
        ExecOptions::new().extensions(ExecutionExtensions::new().with(DropProbe(dropped.clone()))),
    )
    .await
    .unwrap();

    assert!(dropped.load(Ordering::SeqCst));
    let capability = retained.lock().unwrap().clone().unwrap();
    assert_eq!(
        capability.try_with(|_| ()),
        Err(ExecutionCapabilityError::Revoked)
    );
}

#[tokio::test]
async fn trusted_host_registry_access_is_explicitly_unscoped() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let registry = BuiltinRegistry::new();
    registry.insert_trusted(
        "trusted-retain-fs",
        Arc::new(RetainFs {
            retained: retained.clone(),
        }),
    );
    let mut bash = Bash::builder().builtin_registry(registry).build();
    bash.fs()
        .write_file(Path::new("/request.txt"), b"trusted")
        .await
        .unwrap();

    bash.exec("trusted-retain-fs").await.unwrap();
    let fs = retained.lock().unwrap().pop().unwrap();
    assert_eq!(
        fs.read_file(Path::new("/request.txt")).await.unwrap(),
        b"trusted"
    );
}

#[tokio::test]
async fn ordinary_host_registry_access_is_execution_scoped() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let registry = BuiltinRegistry::new();
    registry.insert(
        "retain-fs",
        Arc::new(RetainFs {
            retained: retained.clone(),
        }),
    );
    let mut bash = Bash::builder().builtin_registry(registry).build();
    bash.fs()
        .write_file(Path::new("/request.txt"), b"scoped")
        .await
        .unwrap();

    bash.exec("retain-fs").await.unwrap();
    let fs = retained.lock().unwrap().pop().unwrap();
    let error = fs.read_file(Path::new("/request.txt")).await.unwrap_err();
    assert_eq!(error.to_string(), "io error: execution capability revoked");
}

struct RetainFsResolver {
    retained: Arc<Mutex<Vec<Arc<dyn FileSystem>>>>,
}

impl CommandResolver for RetainFsResolver {
    fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>> {
        (name == "resolved-retain-fs").then(|| {
            Arc::new(RetainFs {
                retained: self.retained.clone(),
            }) as Arc<dyn Builtin>
        })
    }
}

#[tokio::test]
async fn command_resolver_builtins_receive_execution_scoped_access() {
    let retained = Arc::new(Mutex::new(Vec::new()));
    let mut bash = Bash::builder()
        .command_resolver(Arc::new(RetainFsResolver {
            retained: retained.clone(),
        }))
        .build();
    bash.fs()
        .write_file(Path::new("/request.txt"), b"resolved")
        .await
        .unwrap();

    bash.exec("resolved-retain-fs").await.unwrap();
    let fs = retained.lock().unwrap().pop().unwrap();
    let error = fs.read_file(Path::new("/request.txt")).await.unwrap_err();
    assert_eq!(error.to_string(), "io error: execution capability revoked");
}
