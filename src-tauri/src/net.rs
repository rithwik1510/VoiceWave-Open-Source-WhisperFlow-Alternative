//! The app's HTTPS client setup, shared by model downloads and usage
//! analytics: rustls trusting both the bundled Mozilla roots and the Windows
//! certificate store (antivirus HTTPS scanning and corporate TLS inspection
//! install their own root there). Each caller builds its own agent from
//! `agent_builder` with the timeouts it needs.

use std::sync::{Arc, OnceLock};

/// A ureq agent builder with the shared TLS config and user agent. Proxy
/// variables from the environment are deliberately ignored: ureq skips
/// `NO_PROXY` and has no SOCKS support here, so a dev's `ALL_PROXY` would turn
/// a working direct connection into a confusing "couldn't reach" failure.
pub fn agent_builder() -> ureq::AgentBuilder {
    let builder =
        ureq::AgentBuilder::new().user_agent(concat!("voicewave/", env!("CARGO_PKG_VERSION")));
    match tls_config() {
        Some(config) => builder.tls_config(config),
        None => builder,
    }
}

/// Built once: reading the Windows certificate store is slow, and every
/// agent can share the result.
fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    static CONFIG: OnceLock<Option<Arc<rustls::ClientConfig>>> = OnceLock::new();
    CONFIG.get_or_init(build_tls_config).clone()
}

fn build_tls_config() -> Option<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let bundled = roots.len();
    // The loader unwraps a couple of Win32 results; a panic there must not
    // take every HTTPS request down with it.
    match std::panic::catch_unwind(rustls_native_certs::load_native_certs) {
        Ok(native) => {
            let (added, ignored) = roots.add_parsable_certificates(native.certs);
            for err in &native.errors {
                tracing::warn!("couldn't read Windows certificate store: {err}");
            }
            tracing::info!(
                "HTTPS client trusts {bundled} bundled + {added} Windows roots ({ignored} unparsable)"
            );
        }
        Err(_) => tracing::warn!("Windows certificate store load panicked; using bundled roots"),
    }
    // `builder_with_provider`, not `builder()`: no reliance on a process-wide
    // crypto provider (same choice ureq makes for its own default).
    let config = rustls::ClientConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|err| tracing::warn!("custom TLS config unavailable, using ureq defaults: {err}"))
    .ok()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Some(Arc::new(config))
}
