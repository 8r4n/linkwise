//! Linkwise radio-control service and portable modem research components.
//!
//! The current executable provides an MCP stdio adapter over the
//! [`radio::RadioControlPort`] contract, with simulated and Hamlib/`rigctld`
//! backends. Native modem codecs, audio capture and host stream transports
//! are separate components; they do not yet form a qualified live modem.
//! See `docs/modem.md` for implemented behavior and remaining integration.
//!
//! Contributor architecture and invariants are documented in
//! `docs/developer-guide.md`; MCP client workflows are in
//! `docs/mcp-usage.md`.

pub mod api;
pub mod audio;
#[cfg(feature = "demo")]
pub mod demo;
pub mod host;
pub mod mcp;
pub mod modem;
pub mod platform;
pub mod radio;
