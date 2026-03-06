//! User authentication and refresh token operations for IndentiaGraphStore.
//!
//! Implements user CRUD (password + OIDC), refresh token lifecycle,
//! and provider-based lookups.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cortex_core::models::{AuthProvider, RefreshTokenNode, UserNode};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

use crate::client::{rid_to_uuid, IndentiaGraphStore};

// ---------------------------------------------------------------------------
// Record types (module-level for SurrealValue derive)
// ---------------------------------------------------------------------------

#[derive(Debug, SurrealValue)]
struct UserRecord {
    id: RecordId,
    email: String,
    name: Option<String>,
    password_hash: Option<String>,
    auth_provider: String,
    external_id: Option<String>,
    picture: Option<String>,
    created_at: String,
    last_login: Option<String>,
}

impl UserRecord {
    fn into_node(self) -> Result<UserNode> {
        Ok(UserNode {
            id: rid_to_uuid(&self.id)?,
            email: self.email,
            name: self.name.unwrap_or_default(),
            picture_url: self.picture,
            auth_provider: parse_auth_provider(&self.auth_provider),
            external_id: self.external_id,
            password_hash: self.password_hash,
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            last_login_at: self
                .last_login
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .unwrap_or_else(Utc::now),
        })
    }
}

#[derive(Debug, SurrealValue)]
struct RefreshTokenRecord {
    id: RecordId,
    token_hash: String,
    user_id: String,
    expires_at: String,
    created_at: String,
    revoked: Option<bool>,
}

impl RefreshTokenRecord {
    fn into_node(self) -> Result<RefreshTokenNode> {
        Ok(RefreshTokenNode {
            token_hash: self.token_hash,
            user_id: Uuid::parse_str(&self.user_id).unwrap_or_else(|_| Uuid::default()),
            expires_at: self
                .expires_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            created_at: self
                .created_at
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            revoked: self.revoked.unwrap_or(false),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_auth_provider(s: &str) -> AuthProvider {
    s.parse().unwrap_or(AuthProvider::Password)
}

fn auth_provider_str(p: &AuthProvider) -> &'static str {
    match p {
        AuthProvider::Password => "password",
        AuthProvider::Oidc => "oidc",
    }
}

// ===========================================================================
// User CRUD
// ===========================================================================

impl IndentiaGraphStore {
    /// Upsert a user — create if not exists, update last_login if exists.
    ///
    /// For OIDC users, lookup by (auth_provider, external_id).
    /// For password users, lookup by (email, auth_provider).
    pub async fn upsert_user(&self, user: &UserNode) -> Result<UserNode> {
        // Try to find existing user
        let existing = if let Some(ref ext_id) = user.external_id {
            self.get_user_by_provider_id(auth_provider_str(&user.auth_provider), ext_id)
                .await?
        } else {
            self.get_user_by_email_and_provider(&user.email, auth_provider_str(&user.auth_provider))
                .await?
        };

        if let Some(existing_user) = existing {
            // Update last_login, name, picture
            let rid = RecordId::new("user", existing_user.id.to_string().as_str());
            self.db
                .query(
                    "UPDATE $rid SET \
                     name = $name, picture = $picture, \
                     last_login = $last_login \
                     RETURN NONE",
                )
                .bind(("rid", rid))
                .bind(("name", user.name.clone()))
                .bind(("picture", user.picture_url.clone()))
                .bind(("last_login", Utc::now().to_rfc3339()))
                .await
                .context("Failed to update user")?;

            // Return refreshed user
            self.get_user_by_id(existing_user.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("User disappeared after update"))
        } else {
            // Create new user
            let rid = RecordId::new("user", user.id.to_string().as_str());
            self.db
                .query(
                    "CREATE $rid SET \
                     email = $email, name = $name, \
                     password_hash = $pw_hash, \
                     auth_provider = $provider, \
                     external_id = $ext_id, \
                     picture = $picture, \
                     created_at = $created_at, \
                     last_login = $last_login \
                     RETURN NONE",
                )
                .bind(("rid", rid))
                .bind(("email", user.email.clone()))
                .bind(("name", user.name.clone()))
                .bind(("pw_hash", user.password_hash.clone()))
                .bind((
                    "provider",
                    auth_provider_str(&user.auth_provider).to_string(),
                ))
                .bind(("ext_id", user.external_id.clone()))
                .bind(("picture", user.picture_url.clone()))
                .bind(("created_at", user.created_at.to_rfc3339()))
                .bind(("last_login", user.last_login_at.to_rfc3339()))
                .await
                .context("Failed to create user")?;

            self.get_user_by_id(user.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("User not found after creation"))
        }
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<UserNode>> {
        let rid = RecordId::new("user", id.to_string().as_str());
        let mut resp = self
            .db
            .query("SELECT * FROM $rid")
            .bind(("rid", rid))
            .await
            .context("Failed to get user by id")?;
        let records: Vec<UserRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_user_by_provider_id(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<UserNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM user WHERE auth_provider = $provider AND external_id = $ext_id LIMIT 1",
            )
            .bind(("provider", provider.to_string()))
            .bind(("ext_id", external_id.to_string()))
            .await
            .context("Failed to get user by provider id")?;
        let records: Vec<UserRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_user_by_email_and_provider(
        &self,
        email: &str,
        provider: &str,
    ) -> Result<Option<UserNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM user WHERE email = $email AND auth_provider = $provider LIMIT 1")
            .bind(("email", email.to_string()))
            .bind(("provider", provider.to_string()))
            .await
            .context("Failed to get user by email and provider")?;
        let records: Vec<UserRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<UserNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM user WHERE email = $email LIMIT 1")
            .bind(("email", email.to_string()))
            .await
            .context("Failed to get user by email")?;
        let records: Vec<UserRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => Ok(Some(r.into_node()?)),
            None => Ok(None),
        }
    }

    pub async fn create_password_user(
        &self,
        email: &str,
        name: &str,
        password_hash: &str,
    ) -> Result<UserNode> {
        let user = UserNode {
            id: Uuid::new_v4(),
            email: email.to_string(),
            name: name.to_string(),
            picture_url: None,
            auth_provider: AuthProvider::Password,
            external_id: None,
            password_hash: Some(password_hash.to_string()),
            created_at: Utc::now(),
            last_login_at: Utc::now(),
        };

        let rid = RecordId::new("user", user.id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 email = $email, name = $name, \
                 password_hash = $pw_hash, \
                 auth_provider = $provider, \
                 created_at = $created_at, \
                 last_login = $last_login \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("email", user.email.clone()))
            .bind(("name", user.name.clone()))
            .bind(("pw_hash", Some(password_hash.to_string())))
            .bind(("provider", "password".to_string()))
            .bind(("created_at", user.created_at.to_rfc3339()))
            .bind(("last_login", user.last_login_at.to_rfc3339()))
            .await
            .context("Failed to create password user")?;

        self.get_user_by_id(user.id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found after creation"))
    }

    pub async fn list_users(&self) -> Result<Vec<UserNode>> {
        let mut resp = self
            .db
            .query("SELECT * FROM user ORDER BY created_at DESC")
            .await
            .context("Failed to list users")?;
        let records: Vec<UserRecord> = resp.take(0)?;
        records.into_iter().map(|r| r.into_node()).collect()
    }

    // =========================================================================
    // Refresh Tokens
    // =========================================================================

    pub async fn create_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let token_id = Uuid::new_v4();
        let rid = RecordId::new("refresh_token", token_id.to_string().as_str());
        self.db
            .query(
                "CREATE $rid SET \
                 token_hash = $hash, user_id = $uid, \
                 expires_at = $expires, created_at = $created_at, \
                 revoked = false \
                 RETURN NONE",
            )
            .bind(("rid", rid))
            .bind(("hash", token_hash.to_string()))
            .bind(("uid", user_id.to_string()))
            .bind(("expires", expires_at.to_rfc3339()))
            .bind(("created_at", Utc::now().to_rfc3339()))
            .await
            .context("Failed to create refresh token")?;
        Ok(())
    }

    pub async fn validate_refresh_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<RefreshTokenNode>> {
        let mut resp = self
            .db
            .query(
                "SELECT * FROM refresh_token WHERE token_hash = $hash AND (revoked = false OR revoked IS NONE) LIMIT 1",
            )
            .bind(("hash", token_hash.to_string()))
            .await
            .context("Failed to validate refresh token")?;
        let records: Vec<RefreshTokenRecord> = resp.take(0)?;
        match records.into_iter().next() {
            Some(r) => {
                let node = r.into_node()?;
                // Check expiration
                if node.expires_at < Utc::now() {
                    return Ok(None);
                }
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    pub async fn revoke_refresh_token(&self, token_hash: &str) -> Result<bool> {
        // Count matching tokens first
        let hash_str = token_hash.to_string();
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS total FROM refresh_token WHERE token_hash = $hash AND (revoked = false OR revoked IS NONE) GROUP ALL",
            )
            .bind(("hash", hash_str.clone()))
            .await
            .context("Failed to count refresh tokens")?;
        let count_result: Vec<serde_json::Value> = count_resp.take(0)?;
        let found = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if found == 0 {
            return Ok(false);
        }

        self.db
            .query(
                "UPDATE refresh_token SET revoked = true WHERE token_hash = $hash AND (revoked = false OR revoked IS NONE) RETURN NONE",
            )
            .bind(("hash", hash_str))
            .await
            .context("Failed to revoke refresh token")?;
        Ok(true)
    }

    pub async fn revoke_all_user_tokens(&self, user_id: Uuid) -> Result<u64> {
        let uid = user_id.to_string();
        // Count matching tokens first
        let mut count_resp = self
            .db
            .query(
                "SELECT count() AS total FROM refresh_token WHERE user_id = $uid AND (revoked = false OR revoked IS NONE) GROUP ALL",
            )
            .bind(("uid", uid.clone()))
            .await
            .context("Failed to count user tokens")?;
        let count_result: Vec<serde_json::Value> = count_resp.take(0)?;
        let found = count_result
            .first()
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if found > 0 {
            self.db
                .query(
                    "UPDATE refresh_token SET revoked = true WHERE user_id = $uid AND (revoked = false OR revoked IS NONE) RETURN NONE",
                )
                .bind(("uid", uid))
                .await
                .context("Failed to revoke all user tokens")?;
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IndentiaGraphStore;

    async fn setup() -> IndentiaGraphStore {
        let store = IndentiaGraphStore::new_memory().await.unwrap();
        store.init_schema().await.unwrap();
        store
    }

    fn test_oidc_user() -> UserNode {
        UserNode {
            id: Uuid::new_v4(),
            email: "alice@example.com".to_string(),
            name: "Alice".to_string(),
            picture_url: Some("https://example.com/alice.jpg".to_string()),
            auth_provider: AuthProvider::Oidc,
            external_id: Some("google-123".to_string()),
            password_hash: None,
            created_at: Utc::now(),
            last_login_at: Utc::now(),
        }
    }

    // =========================================================================
    // User tests
    // =========================================================================

    #[tokio::test]
    async fn test_upsert_new_user() {
        let store = setup().await;
        let user = test_oidc_user();
        let created = store.upsert_user(&user).await.unwrap();
        assert_eq!(created.email, "alice@example.com");
        assert_eq!(created.auth_provider, AuthProvider::Oidc);
        assert_eq!(created.external_id, Some("google-123".to_string()));
    }

    #[tokio::test]
    async fn test_upsert_existing_user() {
        let store = setup().await;
        let user = test_oidc_user();
        let created = store.upsert_user(&user).await.unwrap();

        // Upsert again with updated name
        let mut updated_user = user.clone();
        updated_user.name = "Alice Updated".to_string();
        let upserted = store.upsert_user(&updated_user).await.unwrap();
        assert_eq!(upserted.id, created.id); // Same user
        assert_eq!(upserted.name, "Alice Updated");
    }

    #[tokio::test]
    async fn test_get_user_by_provider_id() {
        let store = setup().await;
        let user = test_oidc_user();
        store.upsert_user(&user).await.unwrap();

        let found = store
            .get_user_by_provider_id("oidc", "google-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.email, "alice@example.com");

        let not_found = store
            .get_user_by_provider_id("oidc", "nonexistent")
            .await
            .unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_create_password_user() {
        let store = setup().await;
        let user = store
            .create_password_user("bob@example.com", "Bob", "$2b$12$hash...")
            .await
            .unwrap();
        assert_eq!(user.email, "bob@example.com");
        assert_eq!(user.auth_provider, AuthProvider::Password);
        assert!(user.password_hash.is_some());
    }

    #[tokio::test]
    async fn test_get_user_by_email() {
        let store = setup().await;
        store
            .create_password_user("carol@example.com", "Carol", "$hash")
            .await
            .unwrap();

        let found = store
            .get_user_by_email("carol@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.name, "Carol");

        let not_found = store.get_user_by_email("nobody@example.com").await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_list_users() {
        let store = setup().await;
        store
            .create_password_user("a@example.com", "A", "$hash1")
            .await
            .unwrap();
        store
            .create_password_user("b@example.com", "B", "$hash2")
            .await
            .unwrap();

        let users = store.list_users().await.unwrap();
        assert_eq!(users.len(), 2);
    }

    // =========================================================================
    // Refresh token tests
    // =========================================================================

    #[tokio::test]
    async fn test_refresh_token_lifecycle() {
        let store = setup().await;
        let user = store
            .create_password_user("token@example.com", "Token User", "$hash")
            .await
            .unwrap();

        let expires = Utc::now() + chrono::Duration::hours(24);
        store
            .create_refresh_token(user.id, "hash_abc123", expires)
            .await
            .unwrap();

        // Validate
        let token = store
            .validate_refresh_token("hash_abc123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(token.user_id, user.id);
        assert!(!token.revoked);

        // Revoke
        let revoked = store.revoke_refresh_token("hash_abc123").await.unwrap();
        assert!(revoked);

        // Validate again — should be None
        let invalid = store.validate_refresh_token("hash_abc123").await.unwrap();
        assert!(invalid.is_none());
    }

    #[tokio::test]
    async fn test_revoke_all_user_tokens() {
        let store = setup().await;
        let user = store
            .create_password_user("multi@example.com", "Multi", "$hash")
            .await
            .unwrap();

        let expires = Utc::now() + chrono::Duration::hours(24);
        store
            .create_refresh_token(user.id, "token_1", expires)
            .await
            .unwrap();
        store
            .create_refresh_token(user.id, "token_2", expires)
            .await
            .unwrap();
        store
            .create_refresh_token(user.id, "token_3", expires)
            .await
            .unwrap();

        let revoked = store.revoke_all_user_tokens(user.id).await.unwrap();
        assert_eq!(revoked, 3);

        // All should be invalid
        assert!(store
            .validate_refresh_token("token_1")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .validate_refresh_token("token_2")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .validate_refresh_token("token_3")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_expired_token_is_invalid() {
        let store = setup().await;
        let user = store
            .create_password_user("expired@example.com", "Expired", "$hash")
            .await
            .unwrap();

        // Create a token that expired in the past
        let expired_at = Utc::now() - chrono::Duration::hours(1);
        store
            .create_refresh_token(user.id, "expired_hash", expired_at)
            .await
            .unwrap();

        let result = store.validate_refresh_token("expired_hash").await.unwrap();
        assert!(result.is_none());
    }
}
