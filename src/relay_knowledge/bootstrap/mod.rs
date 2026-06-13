//! Outermost process and runtime assembly facades.
//!
//! The bootstrap layer is the onion architecture boundary that owns process
//! concerns: argv/stdout/stderr handling, environment loading, runtime
//! configuration assembly, adapter construction, listener startup, worker
//! lifecycle, and graceful shutdown. During the first migration step these
//! facades delegate to the existing interface and application modules so the
//! observable CLI and service behavior remains unchanged while new process
//! entry points move to the outer layer.

pub mod cli;
