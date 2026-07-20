//! Integration tests for the HTTP-backed Transaction Extension methods on
//! [`StacClient`]: `create_item`, `update_item`, `upsert_item`, `delete_item`.
//!
//! Tests use a hand-rolled TCP mock server so no new dependencies are needed.

#![allow(clippy::panic)]
#![allow(clippy::expect_used)]

#[cfg(feature = "async")]
mod http_transaction_tests {
    use oxigdal_stac::{StacClient, StacError};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    // ── HTTP response canned strings ──────────────────────────────────────────

    const HTTP_201_CREATED: &str = concat!(
        "HTTP/1.1 201 Created\r\n",
        "Content-Length: 0\r\n",
        "Location: /collections/col/items/item1\r\n",
        "\r\n"
    );

    const HTTP_200_OK: &str = concat!("HTTP/1.1 200 OK\r\n", "Content-Length: 0\r\n", "\r\n");

    const HTTP_204_NO_CONTENT: &str = concat!(
        "HTTP/1.1 204 No Content\r\n",
        "Content-Length: 0\r\n",
        "\r\n"
    );

    const HTTP_404_NOT_FOUND: &str = concat!(
        "HTTP/1.1 404 Not Found\r\n",
        "Content-Length: 0\r\n",
        "\r\n"
    );

    const HTTP_409_CONFLICT: &str =
        concat!("HTTP/1.1 409 Conflict\r\n", "Content-Length: 0\r\n", "\r\n");

    const HTTP_500_ERROR: &str = concat!(
        "HTTP/1.1 500 Internal Server Error\r\n",
        "Content-Length: 5\r\n",
        "\r\n",
        "oops!"
    );

    // ── Mock server helper ────────────────────────────────────────────────────

    /// Starts a minimal TCP server that responds to each incoming connection
    /// with the next response in `responses`, then closes that connection.
    ///
    /// Returns the base URL and a join handle so the test can wait for the
    /// server thread to finish cleanly.
    fn start_mock_server(responses: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let responses: Vec<String> = responses.iter().map(|s| s.to_string()).collect();

        let handle = thread::spawn(move || {
            for response in responses {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 4096];
                        let _ = stream.read(&mut buf);
                        let _ = stream.write_all(response.as_bytes());
                        // stream drops here, closing the connection
                    }
                    Err(_) => break,
                }
            }
        });

        (format!("http://{}", addr), handle)
    }

    // ── create_item tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_item_returns_201_with_location() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_201_CREATED]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item1", "type": "Feature", "stac_version": "1.0.0" });

        let result = client.create_item("col", &item).await.expect("create");
        assert_eq!(result.status, 201);
        assert_eq!(
            result.location.as_deref(),
            Some("/collections/col/items/item1")
        );
    }

    #[tokio::test]
    async fn test_create_item_200_ok() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_200_OK]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item2", "type": "Feature", "stac_version": "1.0.0" });

        let result = client.create_item("col", &item).await.expect("create 200");
        assert_eq!(result.status, 200);
        assert!(result.location.is_none());
    }

    #[tokio::test]
    async fn test_create_item_404_returns_not_found() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_404_NOT_FOUND]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item3", "type": "Feature", "stac_version": "1.0.0" });

        let err = client
            .create_item("no-such-col", &item)
            .await
            .expect_err("expected NotFound error");
        assert!(
            matches!(err, StacError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_create_item_409_returns_already_exists() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_409_CONFLICT]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "dup-item", "type": "Feature", "stac_version": "1.0.0" });

        let err = client
            .create_item("col", &item)
            .await
            .expect_err("expected AlreadyExists error");
        assert!(
            matches!(err, StacError::AlreadyExists(_)),
            "expected AlreadyExists, got {err:?}"
        );
    }

    #[tokio::test]
    async fn test_create_item_500_returns_api_response() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_500_ERROR]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "boom", "type": "Feature", "stac_version": "1.0.0" });

        let err = client
            .create_item("col", &item)
            .await
            .expect_err("expected ApiResponse error");
        assert!(
            matches!(err, StacError::ApiResponse(_)),
            "expected ApiResponse, got {err:?}"
        );
        if let StacError::ApiResponse(msg) = err {
            assert!(msg.contains("500"), "message should include status code");
        }
    }

    // ── update_item tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_item_200_ok() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_200_OK]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item1", "type": "Feature", "stac_version": "1.0.0" });

        let result = client
            .update_item("col", "item1", &item)
            .await
            .expect("update 200");
        assert_eq!(result.status, 200);
    }

    #[tokio::test]
    async fn test_update_item_204_no_content() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_204_NO_CONTENT]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item1", "type": "Feature", "stac_version": "1.0.0" });

        let result = client
            .update_item("col", "item1", &item)
            .await
            .expect("update 204");
        assert_eq!(result.status, 204);
    }

    #[tokio::test]
    async fn test_update_item_404_returns_not_found() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_404_NOT_FOUND]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "ghost", "type": "Feature", "stac_version": "1.0.0" });

        let err = client
            .update_item("col", "ghost", &item)
            .await
            .expect_err("expected NotFound error");
        assert!(
            matches!(err, StacError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
        if let StacError::NotFound(msg) = err {
            assert!(msg.contains("ghost"), "message should contain item_id");
            assert!(msg.contains("col"), "message should contain collection_id");
        }
    }

    // ── delete_item tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_item_204_no_content() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_204_NO_CONTENT]);

        let client = StacClient::new(&base_url).expect("client");
        let result = client
            .delete_item("col", "item1")
            .await
            .expect("delete 204");
        assert_eq!(result.status, 204);
        assert!(result.location.is_none());
    }

    #[tokio::test]
    async fn test_delete_item_200_ok() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_200_OK]);

        let client = StacClient::new(&base_url).expect("client");
        let result = client
            .delete_item("col", "item2")
            .await
            .expect("delete 200");
        assert_eq!(result.status, 200);
    }

    #[tokio::test]
    async fn test_delete_item_404_returns_not_found() {
        let (base_url, _handle) = start_mock_server(vec![HTTP_404_NOT_FOUND]);

        let client = StacClient::new(&base_url).expect("client");
        let err = client
            .delete_item("col", "gone")
            .await
            .expect_err("expected NotFound error");
        assert!(
            matches!(err, StacError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
        if let StacError::NotFound(msg) = err {
            assert!(msg.contains("gone"), "message should contain item_id");
        }
    }

    // ── upsert_item tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_upsert_item_creates_when_not_exists() {
        // Server returns 201 on the first (POST) request
        let (base_url, _handle) = start_mock_server(vec![HTTP_201_CREATED]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item1", "type": "Feature", "stac_version": "1.0.0" });

        let result = client
            .upsert_item("col", &item)
            .await
            .expect("upsert create");
        assert_eq!(result.status, 201);
        assert!(result.location.is_some());
    }

    #[tokio::test]
    async fn test_upsert_item_falls_back_to_update_on_conflict() {
        // First request (POST) → 409 Conflict
        // Second request (PUT) → 200 OK
        let (base_url, _handle) = start_mock_server(vec![HTTP_409_CONFLICT, HTTP_200_OK]);

        let client = StacClient::new(&base_url).expect("client");
        let item = json!({ "id": "item1", "type": "Feature", "stac_version": "1.0.0" });

        let result = client
            .upsert_item("col", &item)
            .await
            .expect("upsert fallback");
        assert_eq!(result.status, 200);
    }

    #[tokio::test]
    async fn test_upsert_item_missing_id_returns_missing_field() {
        // POST returns 409, then upsert tries to extract item id for PUT
        let (base_url, _handle) = start_mock_server(vec![HTTP_409_CONFLICT]);

        let client = StacClient::new(&base_url).expect("client");
        // Item without an "id" field
        let item = json!({ "type": "Feature", "stac_version": "1.0.0" });

        let err = client
            .upsert_item("col", &item)
            .await
            .expect_err("expected MissingField error");
        assert!(
            matches!(err, StacError::MissingField(_)),
            "expected MissingField, got {err:?}"
        );
    }
}
