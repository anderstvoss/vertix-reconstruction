//! The page shell, served with the smallest change that lets it run here.
//!
//! The archived page loads jQuery and the Socket.IO client from their CDNs.
//! Those copies are in the archive too, so the two script URLs are pointed
//! at `/cdn/<host>/<path>`, where the server answers with the archived,
//! hash-checked bytes. Nothing else in the page changes. Third-party
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

/// The Content-Security-Policy sent with the page: only this origin, plus
/// the `blob:` URLs the client makes for sprites unpacked from `res.zip`.
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
media-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
worker-src 'self' blob:; frame-src 'none'; object-src 'none'";

/// Applies [`REWRITES`] to the page bytes.
///
/// # Errors
/// Fails if the page is not UTF-8 or a rewrite does not match exactly once.
pub fn rewrite(page: &[u8]) -> Result<Vec<u8>, Error> {
    let mut text = std::str::from_utf8(page)
        .map_err(|e| Error::Page(format!("page shell is not UTF-8: {e}")))?
        .to_owned();
    for (from, to) in REWRITES {
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

    #[test]
    fn rewrites_both_cdn_scripts_once() {
        let page = br#"<script src="http://code.jquery.com/jquery-2.1.4.min.js"></script>
<script src="http://cdn.socket.io/socket.io-1.4.5.js"></script>"#;
        let out = String::from_utf8(rewrite(page).unwrap()).unwrap();
        assert!(out.contains("\"/cdn/code.jquery.com/jquery-2.1.4.min.js\""));
        assert!(out.contains("\"/cdn/cdn.socket.io/socket.io-1.4.5.js\""));
        assert!(!out.contains("http://"));
    }

    #[test]
    fn refuses_a_page_that_does_not_match() {
        assert!(rewrite(b"<html></html>").is_err());
        let twice = br#""http://code.jquery.com/jquery-2.1.4.min.js" "http://code.jquery.com/jquery-2.1.4.min.js" "http://cdn.socket.io/socket.io-1.4.5.js""#;
        assert!(rewrite(twice).is_err());
    }
}
