//! GraphQL API integration tests.

use oxigdal_gateway::graphql::{GraphQLConfig, GraphQLContext, GraphQLServer};

#[tokio::test]
async fn test_graphql_query_dataset() {
    let server = GraphQLServer::new(GraphQLConfig::default());
    let context = GraphQLContext {
        user_id: Some("test_user".to_string()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    let query = r#"
        query {
            dataset(id: "test_dataset") {
                id
                name
                format
                srs
            }
        }
    "#;

    let result = server.execute(query, None, context).await;
    assert!(result.is_ok());

    let response = result.ok().unwrap_or_default();
    assert!(response.get("data").is_some());
}

#[tokio::test]
async fn test_graphql_query_datasets_with_pagination() {
    let server = GraphQLServer::new(GraphQLConfig::default());
    let context = GraphQLContext {
        user_id: Some("test_user".to_string()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    let query = r#"
        query {
            datasets(limit: 5, offset: 0) {
                id
                name
                description
            }
        }
    "#;

    let result = server.execute(query, None, context).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_graphql_mutation_create_dataset() {
    let server = GraphQLServer::new(GraphQLConfig::default());
    let context = GraphQLContext {
        user_id: Some("test_user".to_string()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    let query = r#"
        mutation {
            createDataset(
                name: "New Dataset",
                format: "GeoTIFF",
                srs: "EPSG:4326"
            ) {
                id
                name
                format
            }
        }
    "#;

    let result = server.execute(query, None, context).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_graphql_query_with_variables() {
    let server = GraphQLServer::new(GraphQLConfig::default());
    let context = GraphQLContext {
        user_id: Some("test_user".to_string()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    let query = r#"
        query GetDataset($id: String!) {
            dataset(id: $id) {
                id
                name
            }
        }
    "#;

    let variables = serde_json::json!({
        "id": "test_dataset_123"
    });

    let result = server.execute(query, Some(variables), context).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_graphql_search_datasets() {
    let server = GraphQLServer::new(GraphQLConfig::default());
    let context = GraphQLContext {
        user_id: Some("test_user".to_string()),
        request_id: uuid::Uuid::new_v4().to_string(),
    };

    let query = r#"
        query {
            searchDatasets(query: "test", limit: 10) {
                id
                name
            }
        }
    "#;

    let result = server.execute(query, None, context).await;
    assert!(result.is_ok());
}
