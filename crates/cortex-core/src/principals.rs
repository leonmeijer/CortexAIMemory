//! Canonical search-ACL principal format (ADR-220), shared with conflux
//! (`conflux_shared.principals`) and ad-ldap-sync. ONE byte-identical format so
//! a memory's `allowed_principals` matches the caller's principal set under an
//! exact, case-sensitive comparison:
//!
//! - `user:<email-or-upn>`  — lowercased
//! - `group:entra:<obj-id>` — GUID as-is
//! - `group:sp:<name>`      — lowercased, spaces→_
//! - `group:ad:<sid>`       — SID as-is

/// Mint a `user:` principal from an email/UPN (lowercased).
pub fn user(email_or_upn: &str) -> String {
    format!("user:{}", email_or_upn.trim().to_lowercase())
}

const AS_IS: [&str; 3] = ["user:id:", "group:entra:", "group:ad:"];

/// Canonicalize one principal string (idempotent). Unknown shapes pass through.
pub fn normalize(principal: &str) -> String {
    let p = principal.trim();
    if p.is_empty() {
        return String::new();
    }
    if AS_IS.iter().any(|pre| p.starts_with(pre)) {
        return p.to_string();
    }
    if let Some(rest) = p.strip_prefix("group:sp:") {
        return format!("group:sp:{}", rest.to_lowercase().replace(' ', "_"));
    }
    if let Some(rest) = p.strip_prefix("user:") {
        return format!("user:{}", rest.to_lowercase());
    }
    p.to_string()
}

/// ACL visibility (ADR-220 semantics): an item is visible when it has no ACL
/// (public), or its ACL intersects the caller's principals, or the caller is the
/// owner. Mirrors the Cypher filter pushed to the store — kept here for testing
/// and any in-Rust filtering.
pub fn is_visible(allowed_principals: &[String], owner: Option<&str>, caller: &[String]) -> bool {
    if allowed_principals.is_empty() {
        return true; // public
    }
    if let Some(o) = owner {
        if caller.iter().any(|c| c == o) {
            return true;
        }
    }
    allowed_principals.iter().any(|a| caller.iter().any(|c| c == a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_is_lowercased() {
        assert_eq!(user("Alice@Contoso.COM"), "user:alice@contoso.com");
    }

    #[test]
    fn normalize_keeps_sid_lowers_user() {
        assert_eq!(normalize("user:Bob@X.COM"), "user:bob@x.com");
        assert_eq!(normalize("group:ad:S-1-5-32-544"), "group:ad:S-1-5-32-544");
        assert_eq!(normalize("group:sp:Site Owners"), "group:sp:site_owners");
    }

    #[test]
    fn visibility_rules() {
        // public
        assert!(is_visible(&[], None, &[]));
        // owner sees own restricted item
        assert!(is_visible(
            &["user:alice@x.com".into()],
            Some("user:alice@x.com"),
            &["user:alice@x.com".into()],
        ));
        // group member sees it
        assert!(is_visible(
            &["group:ad:S-1".into()],
            None,
            &["user:bob@x.com".into(), "group:ad:S-1".into()],
        ));
        // outsider does not
        assert!(!is_visible(
            &["user:alice@x.com".into()],
            Some("user:alice@x.com"),
            &["user:eve@x.com".into()],
        ));
    }
}
