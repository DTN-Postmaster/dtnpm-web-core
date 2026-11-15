//! Pluggable WebUI & DTN service module contract.

use crate::context::ModuleContext;
use axum::Router;
use hardy_async::async_trait;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Public metadata describing an installed [`WebModule`], returned by
/// `GET /api/capabilities` and `GET /api/v1/capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDescriptor {
    /// Short URL-safe identifier (e.g. `"chat"`, `"files"`, `"senml"`).
    /// Used as mount prefix `/api/{id}/v1`.
    pub id: String,
    /// Human-readable display name for UI navigation tabs.
    pub name: String,
    /// Semantic version of the module crate.
    pub version: String,
    /// HTTP base path where the module routes are mounted (e.g. `"/api/chat/v1"`).
    pub base_path: String,
    /// DTN BPv7 service names registered by this module (e.g. `["chat"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
}

/// Extension trait implemented by modular DTN web applications (chat, files, senml, ...).
///
/// Modules own their SQLite migrations, BPv7 endpoint registration, and Axum routes
/// (mounted under `/api/{id}/v1` by [`crate::server::create_router`]).
#[async_trait]
pub trait WebModule: Send + Sync {
    /// Unique lowercase identifier used as `/api/{id}/v1` URL namespace.
    fn id(&self) -> &'static str;

    /// Display name exposed via capabilities discovery.
    fn name(&self) -> &'static str;

    /// Module version string.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// DTN service endpoint names owned by this module (e.g. `["chat"]`).
    fn services(&self) -> Vec<String> {
        vec![self.id().to_string()]
    }

    /// Returns the public [`ModuleDescriptor`] advertised by `/api/capabilities`.
    fn descriptor(&self) -> ModuleDescriptor {
        let id = self.id();
        ModuleDescriptor {
            id: id.to_string(),
            name: self.name().to_string(),
            version: self.version().to_string(),
            base_path: format!("/api/{id}/v1"),
            services: self.services(),
        }
    }

    /// Runs idempotent SQLite migrations against the station database connection.
    fn migrate(&self, _conn: &Connection) -> anyhow::Result<()> {
        Ok(())
    }

    /// Returns Hardy BPA [`FilterPack`](hardy_bpa::filter::pack::FilterPack) instances to register
    /// on [`BpaBuilder`](hardy_bpa::builder::BpaBuilder) prior to BPA startup.
    fn filter_packs(&self) -> Vec<hardy_bpa::filter::pack::FilterPack> {
        Vec::new()
    }

    /// Registers DTN BPv7 endpoints/services/applications against the BPA via `ctx`.
    async fn register_bpa(&self, _ctx: &ModuleContext) -> anyhow::Result<()> {
        Ok(())
    }

    /// Returns the Axum [`Router`] handling requests relative to `/api/{id}/v1`.
    fn api_routes(&self, ctx: &ModuleContext) -> Router<()>;
}
