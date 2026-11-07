//! Linkwise radio-control service and future portable modem boundary.
//!
//! The current executable provides an MCP stdio adapter over the
//! [`radio::RadioControlPort`] contract, with simulated and Hamlib/`rigctld`
//! backends. The audio, platform, and modem modules remain implementation
//! boundaries; protocol and DSP behavior are intentionally undefined until
//! supported by reference observations and repeatable tests.
//!
//! Contributor architecture and invariants are documented in
//! `docs/developer-guide.md`; MCP client workflows are in
//! `docs/mcp-usage.md`.

pub mod api;
pub mod audio;
pub mod mcp;
pub mod modem;
pub mod platform;
pub mod radio;
