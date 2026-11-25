//! HTML, CSS and JavaScript embedded into the binary.
//!
//! The assets are ordinary source files under `assets/` included at compile
//! time. Nothing is loaded from disk or the network at runtime.

/// The Studio single page application.
pub const INDEX_HTML: &str = include_str!("../../../assets/index.html");
/// The Studio stylesheet.
pub const APP_CSS: &str = include_str!("../../../assets/app.css");
/// The Studio client script.
pub const APP_JS: &str = include_str!("../../../assets/app.js");
/// Small vector icon used as the favicon; original artwork.
pub const FAVICON_SVG: &str = include_str!("../../../assets/favicon.svg");
