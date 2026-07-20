//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use async_graphql::Schema;

use super::types::{GraphQLConfig, MutationRoot, QueryRoot, SubscriptionRoot};

#[cfg(test)]
use super::types::{BoundingBoxInput, DataFormat, GraphQLContext, GraphQLServer, PaginationInput};

/// GraphQL schema type.
pub type GraphQLSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;
/// Creates a new GraphQL schema with the given configuration.
pub fn create_schema(config: GraphQLConfig) -> GraphQLSchema {
    let mut builder = Schema::build(QueryRoot, MutationRoot, SubscriptionRoot);
    if !config.enable_introspection {
        builder = builder.disable_introspection();
    }
    builder
        .limit_depth(config.max_depth)
        .limit_complexity(config.max_complexity)
        .finish()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn create_test_context() -> GraphQLContext {
        GraphQLContext {
            user_id: Some("test_user".to_string()),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    #[tokio::test]
    async fn test_query_dataset() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                dataset(id: "test_id") {
                    id
                    name
                    format
                    srs
                    bounds {
                        minX
                        minY
                        maxX
                        maxY
                    }
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_query_datasets_with_pagination() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                datasets(pagination: { limit: 5, offset: 0 }) {
                    edges {
                        cursor
                        node {
                            id
                            name
                        }
                    }
                    pageInfo {
                        hasNextPage
                        hasPreviousPage
                    }
                    totalCount
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_query_layer() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                layer(id: "layer_1") {
                    id
                    name
                    layerType
                    geometryType
                    featureCount
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_query_raster_layer() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                rasterLayer(id: "raster_1") {
                    id
                    name
                    width
                    height
                    bandCount
                    bands {
                        index
                        name
                        dataType
                        colorInterpretation
                    }
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_mutation_create_dataset() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            mutation {
                createDataset(input: {
                    name: "Test Dataset"
                    format: GEO_TIFF
                    srs: "EPSG:4326"
                }) {
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
    async fn test_mutation_create_layer() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            mutation {
                createLayer(input: {
                    name: "Test Layer"
                    layerType: VECTOR
                    datasetId: "dataset_1"
                    geometryType: POLYGON
                }) {
                    id
                    name
                    layerType
                    geometryType
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_mutation_create_feature() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            mutation {
                createFeature(input: {
                    layerId: "layer_1"
                    geometry: "{\"type\":\"Point\",\"coordinates\":[0,0]}"
                    properties: "{\"name\":\"Test\"}"
                }) {
                    id
                    geometryType
                    geometry
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_query_tile() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                tile(input: {
                    layerId: "layer_1"
                    z: 0
                    x: 0
                    y: 0
                }) {
                    x
                    y
                    z
                    contentType
                    sizeBytes
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[tokio::test]
    async fn test_query_statistics() {
        let server = GraphQLServer::new(GraphQLConfig::default());
        let context = create_test_context();
        let query = r#"
            query {
                rasterStatistics(layerId: "layer_1", band: 1) {
                    min
                    max
                    mean
                    stdDev
                    validCount
                }
            }
        "#;
        let result = server.execute(query, None, context).await;
        assert!(result.is_ok());
    }
    #[test]
    fn test_config_default() {
        let config = GraphQLConfig::default();
        assert!(config.enable_introspection);
        assert_eq!(config.max_depth, 10);
        assert_eq!(config.max_complexity, 1000);
        assert!(config.enable_subscriptions);
        assert!(config.enable_dataloader);
    }
    #[test]
    fn test_bounding_box_input_validation() {
        let valid = BoundingBoxInput {
            min_x: -180.0,
            min_y: -90.0,
            max_x: 180.0,
            max_y: 90.0,
        };
        assert!(valid.validate().is_ok());
        let invalid_x = BoundingBoxInput {
            min_x: 180.0,
            min_y: -90.0,
            max_x: -180.0,
            max_y: 90.0,
        };
        assert!(invalid_x.validate().is_err());
        let invalid_y = BoundingBoxInput {
            min_x: -180.0,
            min_y: 90.0,
            max_x: 180.0,
            max_y: -90.0,
        };
        assert!(invalid_y.validate().is_err());
    }
    #[test]
    fn test_pagination_normalize() {
        let pagination = PaginationInput {
            limit: 200,
            offset: -10,
            after: None,
            before: None,
        };
        let (limit, offset) = pagination.normalize(100);
        assert_eq!(limit, 100);
        assert_eq!(offset, 0);
    }
    #[test]
    fn test_data_format_display() {
        assert_eq!(DataFormat::GeoTiff.to_string(), "GeoTIFF");
        assert_eq!(DataFormat::Cog.to_string(), "COG");
        assert_eq!(DataFormat::GeoJson.to_string(), "GeoJSON");
    }
}
