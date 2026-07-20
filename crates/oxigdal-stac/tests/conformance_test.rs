//! Integration tests for the W5 conformance-class auto-detection feature
//! on [`StacClient`]: `with_conformance()` and `supports()`.

#![allow(clippy::panic)]
#![allow(clippy::expect_used)]

#[cfg(feature = "async")]
mod conformance_tests {
    use oxigdal_stac::StacClient;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    // ── Mock server helper ────────────────────────────────────────────────────

    /// Builds a minimal HTTP/1.1 response with the given JSON body.
    fn build_json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// Starts a TCP mock server that serves one response per connection.
    fn start_mock_server(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        let handle = thread::spawn(move || {
            for response in responses {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 8192];
                        let _ = stream.read(&mut buf);
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(_) => break,
                }
            }
        });

        (format!("http://{}", addr), handle)
    }

    // ── Landing page JSON builders ────────────────────────────────────────────

    /// Builds a landing page JSON string with the given conformsTo classes.
    fn landing_page_json(conforms_to: &[&str]) -> String {
        let page = json!({
            "stac_version": "1.0.0",
            "type": "Catalog",
            "conformsTo": conforms_to,
            "links": []
        });
        serde_json::to_string(&page).expect("serialize landing page")
    }

    // ── supports() before with_conformance() ─────────────────────────────────

    #[tokio::test]
    async fn test_supports_returns_false_before_with_conformance() {
        let client = StacClient::new("http://127.0.0.1:19999").expect("client");
        // No with_conformance() called — should be false for everything
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/item-search"));
    }

    // ── with_conformance() populates cache ────────────────────────────────────

    #[tokio::test]
    async fn test_with_conformance_parses_conforms_to() {
        let body = landing_page_json(&[
            "https://api.stacspec.org/v1.0.0/core",
            "https://api.stacspec.org/v1.0.0/item-search",
        ]);
        let response = build_json_response(&body);
        let (base_url, _handle) = start_mock_server(vec![response]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("with_conformance");

        assert!(client.supports("https://api.stacspec.org/v1.0.0/core"));
        assert!(client.supports("https://api.stacspec.org/v1.0.0/item-search"));
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/transaction"));
    }

    #[tokio::test]
    async fn test_with_conformance_transaction_class() {
        let transaction_uri =
            "https://api.stacspec.org/v1.0.0/ogcapi-features/extensions/transaction";
        let body = landing_page_json(&["https://api.stacspec.org/v1.0.0/core", transaction_uri]);
        let response = build_json_response(&body);
        let (base_url, _handle) = start_mock_server(vec![response]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("with_conformance");

        assert!(client.supports("https://api.stacspec.org/v1.0.0/core"));
        assert!(client.supports(transaction_uri));
    }

    #[tokio::test]
    async fn test_with_conformance_empty_conforms_to() {
        let body = landing_page_json(&[]);
        let response = build_json_response(&body);
        let (base_url, _handle) = start_mock_server(vec![response]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("with_conformance");

        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
    }

    // ── Tolerance of network / server failures ────────────────────────────────

    #[tokio::test]
    async fn test_with_conformance_tolerates_connection_refused() {
        // Use a port that is not listening — should not return an error
        let client = StacClient::new("http://127.0.0.1:1")
            .expect("client")
            .with_conformance()
            .await
            .expect("should not error on network failure");

        // Cache is empty — all supports() calls return false
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
    }

    #[tokio::test]
    async fn test_with_conformance_tolerates_non_200_response() {
        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        let (base_url, _handle) = start_mock_server(vec![response.to_string()]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("should not error on 404");

        // Non-success response — cache remains empty
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
    }

    #[tokio::test]
    async fn test_with_conformance_tolerates_invalid_json() {
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nnot valid json";
        let (base_url, _handle) = start_mock_server(vec![response.to_string()]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("should not error on malformed JSON");

        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
    }

    // ── supports() is case-sensitive (URI exact match) ────────────────────────

    #[tokio::test]
    async fn test_supports_is_case_sensitive() {
        let body = landing_page_json(&["https://api.stacspec.org/v1.0.0/core"]);
        let response = build_json_response(&body);
        let (base_url, _handle) = start_mock_server(vec![response]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("with_conformance");

        // Exact URI matches
        assert!(client.supports("https://api.stacspec.org/v1.0.0/core"));
        // Different case — must not match
        assert!(!client.supports("https://API.stacspec.org/v1.0.0/core"));
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/CORE"));
    }

    // ── with_conformance() is idempotent (chain twice) ────────────────────────

    #[tokio::test]
    async fn test_supports_without_conforms_to_key_in_response() {
        // Landing page without a `conformsTo` key at all
        let body = json!({
            "stac_version": "1.0.0",
            "type": "Catalog",
            "links": []
        })
        .to_string();
        let response = build_json_response(&body);
        let (base_url, _handle) = start_mock_server(vec![response]);

        let client = StacClient::new(&base_url)
            .expect("client")
            .with_conformance()
            .await
            .expect("with_conformance");

        // No conformsTo key — treated as empty set
        assert!(!client.supports("https://api.stacspec.org/v1.0.0/core"));
    }
}
