// Emits VERGEN_GIT_SHA so orchestrator.rs can stamp the real git SHA into
// Provenance.engine_sha via env!/option_env!. When the crate is built outside
// a git checkout (e.g. from a packaged crate or shallow tarball) vergen-git2
// will fail to resolve a SHA; in that case we swallow the error and let the
// consumer fall back to CARGO_PKG_VERSION at compile time.
use vergen_git2::{Emitter, Git2Builder};

fn main() {
    let git2 = match Git2Builder::default().sha(false).build() {
        Ok(g) => g,
        Err(_) => return,
    };
    let _ = Emitter::default()
        .fail_on_error()
        .add_instructions(&git2)
        .and_then(|e| e.emit());
}
