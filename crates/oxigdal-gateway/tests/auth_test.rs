//! Authentication integration tests.

use oxigdal_gateway::auth::{
    AuthMethod, Authenticator, Identity, api_key::ApiKeyAuthenticator, jwt::JwtAuthenticator,
    session::SessionAuthenticator,
};

#[tokio::test]
async fn test_api_key_authentication() {
    let auth = ApiKeyAuthenticator::new();

    // Generate a key
    let key = auth.generate_key(
        "user123".to_string(),
        "test-key".to_string(),
        vec!["read".to_string(), "write".to_string()],
    );

    assert!(key.is_ok());
    let key = key.ok().unwrap_or_default();

    // Authenticate with the key
    let result = auth.authenticate(&key).await;
    assert!(result.is_ok());

    let context = result.ok().unwrap_or_else(|| {
        oxigdal_gateway::auth::AuthContext::new(Identity::new("".to_string()), AuthMethod::ApiKey)
    });

    assert_eq!(context.identity.user_id, "user123");
    assert!(context.identity.has_permission("read"));
    assert!(context.identity.has_permission("write"));
}

#[tokio::test]
async fn test_jwt_authentication() {
    let auth = JwtAuthenticator::new(b"test_secret_key_12345678901234567890", 3600);

    let mut identity = Identity::new("user456".to_string());
    identity.roles.insert("admin".to_string());

    // Create token
    let token = auth.create_token(&identity);
    assert!(token.is_ok());

    let token = token.ok().unwrap_or_default();

    // Authenticate with token
    let result = auth.authenticate(&token).await;
    assert!(result.is_ok());

    let context = result.ok().unwrap_or_else(|| {
        oxigdal_gateway::auth::AuthContext::new(Identity::new("".to_string()), AuthMethod::Jwt)
    });

    assert_eq!(context.identity.user_id, "user456");
    assert!(context.identity.has_role("admin"));
}

#[tokio::test]
async fn test_session_authentication() {
    let auth = SessionAuthenticator::new(1800);

    let identity = Identity::new("user789".to_string());

    // Create session
    let session_id = auth.create_session(identity);
    assert!(session_id.is_ok());

    let session_id = session_id.ok().unwrap_or_default();

    // Authenticate with session
    let result = auth.authenticate(&session_id).await;
    assert!(result.is_ok());

    let context = result.ok().unwrap_or_else(|| {
        oxigdal_gateway::auth::AuthContext::new(Identity::new("".to_string()), AuthMethod::Session)
    });

    assert_eq!(context.identity.user_id, "user789");
}

#[tokio::test]
async fn test_jwt_token_refresh() {
    let auth = JwtAuthenticator::new(b"test_secret_key_12345678901234567890", 3600);

    let identity = Identity::new("user999".to_string());

    let token = auth.create_token(&identity).ok().unwrap_or_default();
    let context = auth.authenticate(&token).await.ok().unwrap_or_else(|| {
        oxigdal_gateway::auth::AuthContext::new(Identity::new("".to_string()), AuthMethod::Jwt)
    });

    // Refresh token
    let new_token = auth.refresh(&context).await;
    assert!(new_token.is_ok());

    let new_token = new_token.ok().unwrap_or_default();
    assert_ne!(token, new_token);

    // New token should be valid
    let result = auth.authenticate(&new_token).await;
    assert!(result.is_ok());
}
