//! `dtnpm-web-core`: Modular Axum web dashboard shell, system APIs, [`WebModule`] plugin trait,
//! capabilities discovery, and in-memory test server harness for `dtn-postmaster`.

pub mod auth;
pub mod context;
pub mod injector;
pub mod module;
pub mod routes;
pub mod server;
pub mod test_harness;

pub use context::ModuleContext;
pub use injector::WebBundleInjector;
pub use module::{ModuleDescriptor, WebModule};
pub use server::{
    AppState, CapabilitiesResponse, create_router, register_modules, start_web_server,
};
pub use test_harness::{TestServer, TestServerBuilder};
