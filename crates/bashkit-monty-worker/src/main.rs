// Monty worker: runs in a subprocess, communicates with parent via JSON lines.
// If this process segfaults (e.g., monty parser stack overflow), the parent
// catches the child exit and returns a shell error instead of crashing.
//
// EXPERIMENTAL: Monty is early-stage; this subprocess boundary is the primary
// defense against its known and unknown crash/security bugs.

use bashkit_monty_worker::{
    read_message, write_message, WireExternalResult, WireLimits, WorkerRequest, WorkerResponse,
};
use monty::{
    CollectStringPrint, ExcType, ExternalResult, LimitedTracker, MontyException, MontyRun,
    ResourceLimits, RunProgress,
};
use std::io::{self, BufRead, Write};
use std::time::Duration;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    // Read init message
    let init = match read_message::<WorkerRequest>(&mut reader) {
        Ok(Some(WorkerRequest::Init {
            code,
            filename,
            limits,
        })) => (code, filename, limits),
        Ok(Some(_)) => {
            send_error(&mut writer, "expected Init message", "");
            std::process::exit(1);
        }
        Ok(None) => std::process::exit(0), // EOF, parent closed pipe
        Err(e) => {
            eprintln!("monty-worker: {e}");
            std::process::exit(1);
        }
    };

    let (code, filename, limits) = init;

    if let Err(e) = run(&code, &filename, &limits, &mut reader, &mut writer) {
        eprintln!("monty-worker: {e}");
        std::process::exit(1);
    }
}

fn run(
    code: &str,
    filename: &str,
    limits: &WireLimits,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<(), String> {
    // Strip shebang if present
    let code = if code.starts_with("#!") {
        match code.find('\n') {
            Some(pos) => &code[pos + 1..],
            None => "",
        }
    } else {
        code
    };

    // Parse
    let runner = match MontyRun::new(code.to_owned(), filename, vec![], vec![]) {
        Ok(r) => r,
        Err(e) => {
            send_error(writer, &format!("{e}"), "");
            return Ok(());
        }
    };

    // Set up resource limits
    let rl = ResourceLimits::new()
        .max_allocations(limits.max_allocations)
        .max_duration(Duration::from_secs_f64(limits.max_duration_secs))
        .max_memory(limits.max_memory)
        .max_recursion_depth(Some(limits.max_recursion));

    let tracker = LimitedTracker::new(rl);
    let mut printer = CollectStringPrint::new();

    // Start execution
    let mut progress = match runner.start(vec![], tracker, &mut printer) {
        Ok(p) => p,
        Err(e) => {
            let output = printer.into_output();
            send_error(writer, &format!("{e}"), &output);
            return Ok(());
        }
    };

    // Event loop: handle pauses for OsCalls
    loop {
        match progress {
            RunProgress::OsCall {
                function,
                args,
                kwargs,
                state,
                ..
            } => {
                // Ask parent for VFS operation
                write_message(
                    writer,
                    &WorkerResponse::OsCall {
                        function,
                        args: args.clone(),
                        kwargs: kwargs.clone(),
                    },
                )?;

                // Read parent's response
                let wire_result = match read_message::<WorkerRequest>(reader)? {
                    Some(WorkerRequest::OsResponse { result }) => result,
                    Some(_) => {
                        send_error(writer, "expected OsResponse message", "");
                        return Ok(());
                    }
                    None => return Err("parent closed pipe during OsCall".into()),
                };

                // Convert wire result to monty ExternalResult
                let ext_result = wire_to_external(wire_result);

                match state.run(ext_result, &mut printer) {
                    Ok(next) => progress = next,
                    Err(e) => {
                        let output = printer.into_output();
                        send_error(writer, &format!("{e}"), &output);
                        return Ok(());
                    }
                }
            }
            RunProgress::FunctionCall { state, .. } => {
                // No external functions in virtual mode
                let err = MontyException::new(
                    ExcType::RuntimeError,
                    Some("external function not available in virtual mode".into()),
                );
                match state.run(ExternalResult::Error(err), &mut printer) {
                    Ok(next) => progress = next,
                    Err(e) => {
                        let output = printer.into_output();
                        send_error(writer, &format!("{e}"), &output);
                        return Ok(());
                    }
                }
            }
            RunProgress::ResolveFutures(_) => {
                let output = printer.into_output();
                send_error(
                    writer,
                    "RuntimeError: async operations not supported in virtual mode",
                    &output,
                );
                return Ok(());
            }
            RunProgress::Complete(result) => {
                let output = printer.into_output();
                write_message(writer, &WorkerResponse::Complete { result, output })?;
                return Ok(());
            }
        }
    }
}

fn wire_to_external(wire: WireExternalResult) -> ExternalResult {
    match wire {
        WireExternalResult::Return { value } => ExternalResult::Return(value),
        WireExternalResult::Error { exc_type, message } => {
            ExternalResult::Error(MontyException::new(exc_type, message))
        }
    }
}

fn send_error(writer: &mut impl Write, exception: &str, output: &str) {
    let _ = write_message(
        writer,
        &WorkerResponse::Error {
            exception: format!("{exception}\n"),
            output: output.to_string(),
        },
    );
}
