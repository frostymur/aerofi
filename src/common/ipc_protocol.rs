//! IPC protocol types, reserved for v0.2.
//!
//! v0.1 exposes no socket and no `aerofi-ask` CLI; these are placeholders so
//! the protocol shape is documented in one place. Socket path, stale-socket
//! handling and the JSON request/response exchange are specified in
//! `ARCHITECTURE.md` ("IPC protocol and socket policy").
//!
//! TODO(v0.2): define the real request/response fields and the socket server.

/// A request to prompt a script (reserved for v0.2).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PromptReq;

/// A response to a prompt (reserved for v0.2).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct IpcResponse;
