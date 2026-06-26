// bashkit-eval: mira eval study host.
//
// This binary advertises bashkit's evals to the `mira` CLI over stdio. Run it
// through the host:
//
//     mira --bin bashkit-eval list
//     ANTHROPIC_API_KEY=... mira --bin bashkit-eval run --tag smoke
//     mira --bin bashkit-eval run --targets 'anthropic/*' --format html --out report.html
//
// The `#[eval]` wrappers live here (in the bin crate) so their inventory
// registrations are guaranteed to link into this binary; the heavy lifting is
// in `bashkit_eval::mira_study`. See specs/eval.md.

use bashkit_eval::mira_study;
use mira::{Eval, eval};

#[eval]
fn bashkit_bash() -> Eval {
    mira_study::bash_eval()
}

#[eval]
fn bashkit_smoke() -> Eval {
    mira_study::smoke_eval()
}

#[eval]
fn bashkit_scripting() -> Eval {
    mira_study::scripting_eval()
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Our providers use reqwest with rustls' `rustls-no-provider` feature, so a
    // crypto provider must be installed before any HTTPS request. Subjects also
    // ensure this idempotently; doing it here covers the common path.
    let _ = rustls::crypto::ring::default_provider().install_default();

    mira::Study::registered().serve().await
}
