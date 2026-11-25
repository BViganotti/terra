//! Loopback HTTP/SSE server and launcher.

use std::io;

/// Starts the Studio and returns when the server shuts down.
///
/// The full server is introduced with the Studio milestone; the scaffold keeps
/// the launcher addressable so the workspace builds end to end.
pub fn launch() -> io::Result<()> {
    Ok(())
}
