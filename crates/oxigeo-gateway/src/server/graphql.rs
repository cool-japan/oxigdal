//! GraphQL HTTP endpoints for the gateway serving layer.
//!
//! Wires the crate's async-graphql schema into axum. [`graphql_post`] executes queries and
//! mutations, injecting a fresh [`GraphQLContext`] so resolvers can read the authenticated user id
//! (when present) and a per-request trace id. [`graphiql`] serves the GraphiQL playground pointed
//! at the subscription websocket endpoint. Router assembly mounts these handlers only when a
//! schema exists (`config.enable_graphql`) and, for GraphiQL, only when introspection is enabled.

use async_graphql::http::{GraphQLPlaygroundConfig, playground_source};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::Extension;
use axum::extract::State;
use axum::response::Html;

use crate::auth::AuthContext;
use crate::graphql::GraphQLContext;
use crate::server::state::GatewayState;

/// Executes a GraphQL query or mutation over HTTP.
///
/// A [`GraphQLContext`] is attached to every request via `Request::data` so that the schema's
/// resolvers (all of which read `ctx.data::<GraphQLContext>()`) succeed. The context carries the
/// caller's user id (extracted from an [`AuthContext`] injected by the auth middleware, when
/// authenticated) and a freshly generated request id.
///
/// When GraphQL is disabled on this gateway (`state.graphql` is `None`) a GraphQL-shaped error
/// response is returned rather than panicking; in normal operation the route is only mounted while
/// a schema is present.
pub(crate) async fn graphql_post(
    State(state): State<GatewayState>,
    ctx: Option<Extension<AuthContext>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let Some(schema) = state.graphql.as_ref() else {
        return async_graphql::Response::from_errors(vec![async_graphql::ServerError::new(
            "GraphQL is not enabled on this gateway",
            None,
        )])
        .into();
    };

    let gctx = GraphQLContext {
        user_id: ctx.map(|Extension(auth)| auth.identity.user_id.clone()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    schema.execute(req.into_inner().data(gctx)).await.into()
}

/// Serves the GraphiQL playground user interface.
///
/// The playground submits operations to `POST /graphql` and opens subscriptions against
/// `/graphql/ws`. The gateway state is accepted (and ignored) so the handler shares the router's
/// state type; router assembly only mounts this route when introspection is enabled.
pub(crate) async fn graphiql(State(_state): State<GatewayState>) -> Html<String> {
    Html(playground_source(
        GraphQLPlaygroundConfig::new("/graphql").subscription_endpoint("/graphql/ws"),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use crate::GatewayConfig;
    use crate::server::GatewayServer;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn body_text(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("collect body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn graphql_post_injects_context_and_returns_data() {
        // A default gateway has GraphQL enabled; POST a trivial introspection query. If the
        // GraphQLContext were not injected the resolvers would error, but `__typename` resolves
        // to the query root type name regardless, proving the endpoint is wired and executing.
        let server = GatewayServer::builder(GatewayConfig::default())
            .build()
            .expect("build gateway server");
        let request = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"query":"{ __typename }"}"#))
            .expect("build request");

        let response = server.router().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let text = body_text(response).await;
        assert!(
            text.contains("QueryRoot"),
            "expected query root type in body, got: {text}"
        );
        assert!(
            !text.contains("\"errors\""),
            "expected no GraphQL errors, got: {text}"
        );
    }

    #[tokio::test]
    async fn graphql_query_with_context_dependent_resolver_succeeds() {
        // `datasets` resolvers read GraphQLContext; a successful data response (no errors[])
        // proves the per-request context injection is working end to end.
        let server = GatewayServer::builder(GatewayConfig::default())
            .build()
            .expect("build gateway server");
        let request = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"query":"{ datasets { totalCount } }"}"#))
            .expect("build request");

        let response = server.router().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let text = body_text(response).await;
        assert!(
            !text.contains("\"errors\""),
            "context-dependent resolver returned errors: {text}"
        );
        assert!(
            text.contains("totalCount"),
            "expected totalCount in body, got: {text}"
        );
    }

    #[tokio::test]
    async fn graphiql_playground_is_served() {
        let server = GatewayServer::builder(GatewayConfig::default())
            .build()
            .expect("build gateway server");
        let request = Request::builder()
            .method("GET")
            .uri("/graphql")
            .body(Body::empty())
            .expect("build request");

        let response = server.router().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let text = body_text(response).await;
        assert!(
            text.contains("/graphql/ws"),
            "playground should reference the subscription endpoint"
        );
    }
}
