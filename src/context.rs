//! Shared runtime context passed to web handlers and [`crate::module::WebModule`] implementations.

use std::sync::{Arc, Mutex};

use dtnpm_core::cla_manager::ClaManager;
use dtnpm_core::config::HamConfig;
use dtnpm_core::iam::PolicyEngine;
use dtnpm_core::internal_storage::InternalStorage;
use dtnpm_core::log_buffer::LogBuffer;
use dtnpm_core::routing::RouteManager;
use dtnpm_core::security::DirectionalKeyStore;
use dtnpm_core::service_tracker::ServiceTracker;
use dtnpm_core::{BpaClient, LocalBpaClient};
use hardy_bpa::bpa::BpaRegistration;
use rusqlite::Connection;

use crate::auth::LoginRateLimiter;
use crate::injector::WebBundleInjector;
use crate::module::{ModuleDescriptor, WebModule};

/// Shared runtime state injected into system routes and [`WebModule`] instances.
#[derive(Clone)]
pub struct ModuleContext {
    pub config: Arc<HamConfig>,
    pub bpa: Arc<dyn BpaClient>,
    pub injector: WebBundleInjector,
    pub service_tracker: ServiceTracker,
    pub internal_storage: InternalStorage,
    pub db: Arc<Mutex<Connection>>,
    pub cla_manager: ClaManager,
    pub route_manager: Arc<RouteManager>,
    pub log_buffer: Arc<LogBuffer>,
    pub keystore: Arc<DirectionalKeyStore>,
    pub policy_engine: Arc<PolicyEngine>,
    pub login_rate_limiter: Arc<LoginRateLimiter>,
    pub capabilities: Arc<Vec<ModuleDescriptor>>,
}

impl ModuleContext {
    /// Creates a `ModuleContext` from an arbitrary [`BpaClient`] (local, gRPC, or mock).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<HamConfig>,
        bpa: Arc<dyn BpaClient>,
        injector: WebBundleInjector,
        service_tracker: ServiceTracker,
        internal_storage: InternalStorage,
        cla_manager: ClaManager,
        route_manager: Arc<RouteManager>,
        log_buffer: Arc<LogBuffer>,
        keystore: Arc<DirectionalKeyStore>,
    ) -> Self {
        let db = internal_storage.connection();
        let policy_engine = match PolicyEngine::load(config.auth.policy_path.as_deref()) {
            Ok(engine) => Arc::new(engine),
            Err(e) => {
                tracing::error!(
                    "Failed to load Rego policy ({}): {e}; failing back to default deny-only policy",
                    config
                        .auth
                        .policy_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "default_policy.rego".to_string())
                );
                Arc::new(
                    PolicyEngine::from_rego_str(
                        "fallback_deny.rego",
                        "package dtn.ham.iam\ndefault allow = false\n",
                    )
                    .unwrap_or_else(|_| {
                        unreachable!("built-in fallback Rego policy must always compile")
                    }),
                )
            }
        };
        let login_rate_limiter = Arc::new(LoginRateLimiter::new());
        Self {
            config,
            bpa,
            injector,
            service_tracker,
            internal_storage,
            db,
            cla_manager,
            route_manager,
            log_buffer,
            keystore,
            policy_engine,
            login_rate_limiter,
            capabilities: Arc::new(Vec::new()),
        }
    }

    /// Convenience constructor wrapping any [`BpaRegistration`] inside a [`LocalBpaClient`].
    #[allow(clippy::too_many_arguments)]
    pub fn with_local_bpa(
        config: Arc<HamConfig>,
        bpa: Arc<dyn BpaRegistration>,
        injector: WebBundleInjector,
        service_tracker: ServiceTracker,
        internal_storage: InternalStorage,
        cla_manager: ClaManager,
        route_manager: Arc<RouteManager>,
        log_buffer: Arc<LogBuffer>,
        keystore: Arc<DirectionalKeyStore>,
    ) -> Self {
        Self::new(
            config,
            Arc::new(LocalBpaClient::new(bpa)),
            injector,
            service_tracker,
            internal_storage,
            cla_manager,
            route_manager,
            log_buffer,
            keystore,
        )
    }

    /// Returns a clone of this context with its advertised capabilities updated.
    #[must_use]
    pub fn with_capabilities(mut self, modules: &[Arc<dyn WebModule>]) -> Self {
        self.capabilities = Arc::new(modules.iter().map(|m| m.descriptor()).collect());
        self
    }

    /// Runs [`WebModule::migrate`] for all modules against `self.db` inside a lock.
    pub fn run_migrations(&self, modules: &[Arc<dyn WebModule>]) -> anyhow::Result<()> {
        let conn = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("Database mutex poisoned"))?;
        for module in modules {
            module.migrate(&conn)?;
        }
        Ok(())
    }

    /// Convenience access to the [`BpaRegistration`] trait object (for registering DTN services).
    #[must_use]
    pub fn bpa_registration(&self) -> Arc<dyn BpaRegistration> {
        self.bpa.clone().as_registration()
    }
}
