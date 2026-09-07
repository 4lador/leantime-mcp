//! leantmcp — Model Context Protocol server for [Leantime](https://leantime.io/).
//!
//! Architecture:
//! - [`client`] — JSON-RPC HTTP client with adaptive 429 retry and caches
//! - [`config`] — multi-instance keyring (`~/.config/leantime/instances/`)
//! - [`tools`] — the 41 MCP tools, split by domain
//! - [`backup`] — project backup to timestamped JSON files
//! - [`restore`] — project restore from backup files
//! - [`markdown`] — deterministic Markdown → rich HTML (byte-parity with the
//!   TypeScript edition)
//! - [`harness`] — writes MCP server configs for opencode, Claude, Cursor, Codex
//! - [`doctor`] — keyring/config health check
//!
//! Credentials never appear in harness configs: the binary resolves them at
//! startup from the environment (per-run override) or the keyring.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

/// Project backup — dump to timestamped JSON files.
pub mod backup;
/// JSON-RPC client for the Leantime API (rate-limit aware, cached lookups).
pub mod client;
/// Multi-instance keyring and credential resolution.
pub mod config;
/// Health check for the keyring, configs and live key.
pub mod doctor;
/// Harness (MCP client) config writers.
pub mod harness;
/// Deterministic Markdown → TipTap-compatible HTML converter.
pub mod markdown;
/// Project restore from backup files.
pub mod restore;
/// The 41 MCP tools.
pub mod tools;
