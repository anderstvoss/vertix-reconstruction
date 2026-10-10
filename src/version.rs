//! The version string the server issues to clients.
//!
//! Both clients show a version in their menu footer, as a link labelled
//! `<version> (CHANGELOG)`. The server writes its current version string
//! into that label as it serves each client's page and scripts, and answers
//! `/api/version`. The admin console can change it at run time, to one of
//! the game versions the research dates or to any string; pages pick it up
//! on their next load.

use std::sync::{Arc, RwLock};

/// What follows the version in the menu's label.
pub const SUFFIX: &str = " (CHANGELOG)";
/// The label in KRP's built client (`App.svelte`).
pub const KRP_LABEL: &str = "V3.8 (CHANGELOG)";
/// Longest version string accepted.
pub const MAX_LEN: usize = 40;

/// The server's own version, the default string.
#[must_use]
pub fn default_string() -> String {
    format!("RECON {}", env!("CARGO_PKG_VERSION"))
}

/// The current version string, shared by the game and the HTTP routes.
#[derive(Debug, Clone)]
pub struct Version(Arc<RwLock<String>>);

impl Default for Version {
    fn default() -> Self {
        Self::new(default_string())
    }
}

impl Version {
    #[must_use]
    pub fn new(s: String) -> Self {
        Self(Arc::new(RwLock::new(s)))
    }

    #[must_use]
    pub fn get(&self) -> String {
        self.0
            .read()
            .map_or_else(|e| e.into_inner().clone(), |s| s.clone())
    }

    /// Sets the string after [`check`]ing it.
    ///
    /// # Errors
    /// As [`check`].
    pub fn set(&self, s: &str) -> Result<(), String> {
        check(s)?;
        let mut w = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.clone_into(&mut w);
        Ok(())
    }

    /// The menu label: `<version> (CHANGELOG)`.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}{SUFFIX}", self.get())
    }
}

/// A version string must be short and plain: it is written into HTML and
/// script text unescaped.
///
/// # Errors
/// Fails on an empty or long string, or one with other characters than
/// letters, digits, spaces and `. - _ + : / # ! , ( )`.
pub fn check(s: &str) -> Result<(), String> {
    if s.trim().is_empty() || s.chars().count() > MAX_LEN {
        return Err(format!("a version string is 1 to {MAX_LEN} characters"));
    }
    if let Some(bad) = s
        .chars()
        .find(|c| !(c.is_alphanumeric() || " .-_+:/#!,()".contains(*c)))
    {
        return Err(format!("{bad:?} is not allowed in a version string"));
    }
    Ok(())
}

/// Replaces every `from` in `bytes` with `to`, if there is one.
#[must_use]
pub fn relabel(bytes: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.contains(from)
        .then(|| text.replace(from, to).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_checked() {
        assert!(check("V3.8").is_ok());
        assert!(check("RECON 0.1.0 (test build)").is_ok());
        assert!(check("").is_err());
        assert!(check("<script>").is_err());
        assert!(check("a\"b").is_err());
        assert!(check(&"x".repeat(41)).is_err());
    }

    #[test]
    fn labels_follow_the_string() {
        let v = Version::default();
        assert!(v.label().starts_with("RECON "));
        v.set("V2.0").unwrap();
        assert_eq!(v.label(), "V2.0 (CHANGELOG)");
        assert!(v.set("bad<").is_err());
        assert_eq!(v.get(), "V2.0");
        let page = b"<a href=\"./versions.txt\">V3.8 (CHANGELOG)</a>";
        let out = relabel(page, KRP_LABEL, &v.label()).unwrap();
        assert_eq!(out, b"<a href=\"./versions.txt\">V2.0 (CHANGELOG)</a>");
        assert!(relabel(b"nothing", KRP_LABEL, "x").is_none());
    }
}
