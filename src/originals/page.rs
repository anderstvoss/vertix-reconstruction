//! The page shell, served with the smallest change that lets it run here.
//!
//! The archived page loads jQuery and the Socket.IO client from their CDNs.
//! Those copies are in the archive too, so the two script URLs are pointed
//! at `/cdn/<host>/<path>`, where the server answers with the archived,
//! hash-checked bytes. The menu's version link is relabelled with this
//! server's own version (a decided deviation: this is not the original
//! V3.0 game) and points at our changelog. Nothing else in the page
//! changes. Third-party
//! scripts (ads, analytics, social widgets) stay in the markup and are
//! stopped by the Content-Security-Policy the server sends with the page,
//! so the page never contacts anything but this server.
//!
//! Each rewrite must match exactly once, so a different page shell fails
//! loudly instead of being half-patched.

use super::Error;

/// `(original, replacement)` pairs applied to the page shell.
pub const REWRITES: &[(&str, &str)] = &[
    (
        "\"http://code.jquery.com/jquery-2.1.4.min.js\"",
        "\"/cdn/code.jquery.com/jquery-2.1.4.min.js\"",
    ),
    (
        "\"http://cdn.socket.io/socket.io-1.4.5.js\"",
        "\"/cdn/cdn.socket.io/socket.io-1.4.5.js\"",
    ),
];

/// The menu's version link in the 2016-08-07 page shell.
const VERSION_LINK: &str = "<a target=\"_blank\" href=\"./versions.txt\">V3.0 (CHANGELOG)</a>";

/// Where our version label links to.
const CHANGELOG_URL: &str =
    "https://github.com/anderstvoss/vertix-reconstruction/blob/main/CHANGELOG.md";

/// The label shown instead of `V3.0`.
#[must_use]
pub fn version_label() -> String {
    format!("RECON {} (CHANGELOG)", env!("CARGO_PKG_VERSION"))
}

fn version_link() -> String {
    format!(
        "<a target=\"_blank\" href=\"{CHANGELOG_URL}\">{}</a>",
        version_label()
    )
}

/// The Content-Security-Policy sent with the page: only this origin, plus
/// the `blob:` URLs the client makes for sprites unpacked from `res.zip`.
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
media-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
worker-src 'self' blob:; frame-src 'none'; object-src 'none'";

/// Applies [`REWRITES`] and the version relabel to the page bytes.
///
/// # Errors
/// Fails if the page is not UTF-8 or a rewrite does not match exactly once.
pub fn rewrite(page: &[u8]) -> Result<Vec<u8>, Error> {
    let mut text = std::str::from_utf8(page)
        .map_err(|e| Error::Page(format!("page shell is not UTF-8: {e}")))?
        .to_owned();
    let version = version_link();
    let all = REWRITES
        .iter()
        .copied()
        .chain(std::iter::once((VERSION_LINK, version.as_str())));
    for (from, to) in all {
        let n = text.matches(from).count();
        if n != 1 {
            return Err(Error::Page(format!(
                "page rewrite expected one match of {from}, found {n}"
            )));
        }
        text = text.replacen(from, to, 1);
    }
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &[u8] = br#"<script src="http://code.jquery.com/jquery-2.1.4.min.js"></script>
<script src="http://cdn.socket.io/socket.io-1.4.5.js"></script>
<a target="_blank" href="./versions.txt">V3.0 (CHANGELOG)</a>"#;

    #[test]
    fn rewrites_both_cdn_scripts_once() {
        let out = String::from_utf8(rewrite(PAGE).unwrap()).unwrap();
        assert!(out.contains("\"/cdn/code.jquery.com/jquery-2.1.4.min.js\""));
        assert!(out.contains("\"/cdn/cdn.socket.io/socket.io-1.4.5.js\""));
        assert!(!out.contains("http://"));
    }

    #[test]
    fn relabels_the_version() {
        let out = String::from_utf8(rewrite(PAGE).unwrap()).unwrap();
        assert!(!out.contains("V3.0"));
        assert!(out.contains(&format!(">{}</a>", version_label())));
    }

    #[test]
    fn refuses_a_page_that_does_not_match() {
        assert!(rewrite(b"<html></html>").is_err());
        let twice = br#""http://code.jquery.com/jquery-2.1.4.min.js" "http://code.jquery.com/jquery-2.1.4.min.js" "http://cdn.socket.io/socket.io-1.4.5.js""#;
        assert!(rewrite(twice).is_err());
    }
}
