//! Vector Analysis with PostGIS Example
//!
//! This example demonstrates advanced spatial vector analysis using PostGIS including:
//! - Loading vector data from multiple formats (GeoJSON, Shapefile, GeoParquet)
//! - Connecting to PostGIS database
//! - Spatial indexing and optimization
//! - Complex spatial queries (intersections, buffers, unions)
//! - Spatial joins and aggregations
//! - Topology validation and repair
//! - Hot spot analysis
//! - Network analysis
//! - Exporting results back to files or database

use oxigdal_core::{Dataset, Feature};
use oxigdal_postgis::{
    PostGisConnection, PostGisConfig, SpatialQuery, QueryBuilder,
    IndexType, TopologyValidator,
};
use oxigdal_vector::{VectorDataset, Geometry};
use oxigdal_algorithms::{SpatialClustering, HotspotAnalysis};
use oxigdal_geojson::GeoJsonDriver;
use oxigdal_geoparquet::GeoParquetDriver;
use std::collections::HashMap;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("vector_postgis=info")
        .init();

    info!("Starting Vector Analysis with PostGIS");

    // Configuration
    let config = AnalysisConfig {
        database: PostGisConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "gis_analysis".to_string(),
            user: "postgres".to_string(),
            password: std::env::var("PGPASSWORD")
                .unwrap_or_else(|_| "postgres".to_string()),
            pool_size: 10,
        },
        input_data: InputData {
            points_of_interest: "data/pois.geojson",
            road_network: "data/roads.shp",
            administrative_boundaries: "data/boundaries.gpkg",
            land_use: "data/landuse.parquet",
        },
        analysis_tasks: vec![
            AnalysisTask::ProximityAnalysis {
                search_radius_meters: 1000.0,
            },
            AnalysisTask::SpatialJoin {
                method: JoinMethod::Contains,
            },
            AnalysisTask::BufferAnalysis {
                buffer_distance_meters: 500.0,
            },
            AnalysisTask::HotspotDetection {
                threshold: 0.05,
            },
            AnalysisTask::NetworkAnalysis {
                analysis_type: NetworkAnalysisType::ShortestPath,
            },
        ],
        output_format: OutputFormat::GeoParquet,
        create_indexes: true,
        validate_topology: true,
    };

    // Step 1: Connect to PostGIS database
    info!("Step 1: Connecting to PostGIS database");

    let mut conn = PostGisConnection::new(&config.database).await?;

    info!("  Connected to: {}@{}/{}",
          config.database.user, config.database.host, config.database.database);

    // Verify PostGIS extension
    let postgis_version = conn.postgis_version().await?;
    info!("  PostGIS version: {}", postgis_version);

    // Step 2: Load and import vector data
    info!("Step 2: Loading vector data into PostGIS");

    // Load POIs
    info!("  Loading POIs from: {}", config.input_data.points_of_interest);
    let pois = VectorDataset::open(config.input_data.points_of_interest).await?;
    let poi_count = pois.feature_count();
    info!("    Loaded {} POIs", poi_count);

    // Import to PostGIS
    conn.import_dataset(&pois, "pois", true).await?;
    info!("    Imported to table: pois");

    // Load road network
    info!("  Loading road network from: {}", config.input_data.road_network);
    let roads = VectorDataset::open(config.input_data.road_network).await?;
    let road_count = roads.feature_count();
    info!("    Loaded {} road segments", road_count);

    conn.import_dataset(&roads, "roads", true).await?;
    info!("    Imported to table: roads");

    // Load boundaries
    info!("  Loading boundaries from: {}", config.input_data.administrative_boundaries);
    let boundaries = VectorDataset::open(config.input_data.administrative_boundaries).await?;
    let boundary_count = boundaries.feature_count();
    info!("    Loaded {} boundaries", boundary_count);

    conn.import_dataset(&boundaries, "boundaries", true).await?;
    info!("    Imported to table: boundaries");

    // Load land use
    info!("  Loading land use from: {}", config.input_data.land_use);
    let landuse = VectorDataset::open(config.input_data.land_use).await?;
    let landuse_count = landuse.feature_count();
    info!("    Loaded {} land use polygons", landuse_count);

    conn.import_dataset(&landuse, "landuse", true).await?;
    info!("    Imported to table: landuse");

    // Step 3: Create spatial indexes
    if config.create_indexes {
        info!("Step 3: Creating spatial indexes");

        for table in &["pois", "roads", "boundaries", "landuse"] {
            info!("  Creating GIST index on: {}", table);
            conn.create_spatial_index(table, "geom", IndexType::Gist).await?;
        }

        // Create additional attribute indexes for faster queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pois_category ON pois(category)",
            &[]
        ).await?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_roads_type ON roads(road_type)",
            &[]
        ).await?;

        info!("  Indexes created successfully");
    } else {
        info!("Step 3: Skipping index creation");
    }

    // Step 4: Validate topology
    if config.validate_topology {
        info!("Step 4: Validating topology");

        let validator = TopologyValidator::new();

        for table in &["boundaries", "landuse"] {
            info!("  Validating: {}", table);

            let issues = validator.validate_table(&conn, table).await?;

            if issues.is_empty() {
                info!("    ✓ No topology issues found");
            } else {
                warn!("    Found {} topology issues:", issues.len());
                for (idx, issue) in issues.iter().take(5).enumerate() {
                    warn!("      {}. {}", idx + 1, issue);
                }

                if issues.len() > 5 {
                    warn!("      ... and {} more", issues.len() - 5);
                }

                // Auto-repair if possible
                info!("    Attempting automatic repair...");
                let repaired = validator.repair_topology(&conn, table).await?;
                info!("    Repaired {} geometries", repaired);
            }
        }
    } else {
        info!("Step 4: Skipping topology validation");
    }

    // Step 5: Perform spatial analyses
    info!("Step 5: Performing spatial analyses");

    let mut analysis_results = HashMap::new();

    for (idx, task) in config.analysis_tasks.iter().enumerate() {
        info!("  Analysis {}/{}: {:?}", idx + 1, config.analysis_tasks.len(), task);

        match task {
            AnalysisTask::ProximityAnalysis { search_radius_meters } => {
                let result = proximity_analysis(&conn, *search_radius_meters).await?;
                info!("    Found {} POIs within {} m of roads",
                      result.feature_count(), search_radius_meters);
                analysis_results.insert("proximity", result);
            }

            AnalysisTask::SpatialJoin { method } => {
                let result = spatial_join(&conn, method).await?;
                info!("    Joined {} POIs with administrative boundaries",
                      result.feature_count());
                analysis_results.insert("spatial_join", result);
            }

            AnalysisTask::BufferAnalysis { buffer_distance_meters } => {
                let result = buffer_analysis(&conn, *buffer_distance_meters).await?;
                info!("    Created {} m buffers around {} road segments",
                      buffer_distance_meters, result.feature_count());
                analysis_results.insert("buffers", result);
            }

            AnalysisTask::HotspotDetection { threshold } => {
                let result = hotspot_detection(&conn, *threshold).await?;
                info!("    Detected {} POI hotspots (threshold: {})",
                      result.hotspot_count, threshold);
                analysis_results.insert("hotspots", result);
            }

            AnalysisTask::NetworkAnalysis { analysis_type } => {
                let result = network_analysis(&conn, analysis_type).await?;
                info!("    Completed {:?} network analysis", analysis_type);
                analysis_results.insert("network", result);
            }
        }
    }

    // Step 6: Advanced spatial queries
    info!("Step 6: Running advanced spatial queries");

    // Query 1: Find all POIs within specific land use zones
    info!("  Query 1: POIs in residential areas");
    let residential_pois = QueryBuilder::new()
        .select("pois.*, landuse.type as landuse_type")
        .from("pois")
        .spatial_join("landuse", "ST_Within(pois.geom, landuse.geom)")
        .where_clause("landuse.type = 'residential'")
        .execute(&conn)
        .await?;

    info!("    Found {} POIs in residential areas", residential_pois.len());

    // Query 2: Calculate road density by administrative boundary
    info!("  Query 2: Road density analysis");
    let road_density = conn.execute_query(
        "SELECT b.name,
                SUM(ST_Length(ST_Intersection(r.geom, b.geom))) / ST_Area(b.geom) * 1000000 as road_density_km_per_km2
         FROM boundaries b
         LEFT JOIN roads r ON ST_Intersects(r.geom, b.geom)
         GROUP BY b.id, b.name, b.geom
         ORDER BY road_density_km_per_km2 DESC",
        &[]
    ).await?;

    for row in road_density.iter().take(5) {
        let name: String = row.get("name");
        let density: f64 = row.get("road_density_km_per_km2");
        info!("    {}: {:.2} km/km²", name, density);
    }

    // Query 3: Find nearest road to each POI
    info!("  Query 3: Nearest road to each POI");
    let nearest_roads = conn.execute_query(
        "SELECT DISTINCT ON (p.id)
                p.id as poi_id,
                p.name as poi_name,
                r.name as road_name,
                ST_Distance(p.geom, r.geom) as distance_meters
         FROM pois p
         CROSS JOIN LATERAL (
             SELECT id, name, geom
             FROM roads r
             WHERE ST_DWithin(p.geom, r.geom, 1000)
             ORDER BY p.geom <-> r.geom
             LIMIT 1
         ) r
         ORDER BY p.id",
        &[]
    ).await?;

    info!("    Calculated nearest roads for {} POIs", nearest_roads.len());

    // Step 7: Spatial aggregations
    info!("Step 7: Performing spatial aggregations");

    // Count POIs by category within each boundary
    let poi_counts = conn.execute_query(
        "SELECT b.name as boundary_name,
                p.category,
                COUNT(*) as poi_count
         FROM boundaries b
         LEFT JOIN pois p ON ST_Contains(b.geom, p.geom)
         WHERE p.category IS NOT NULL
         GROUP BY b.name, p.category
         ORDER BY b.name, poi_count DESC",
        &[]
    ).await?;

    info!("  POI distribution by boundary:");
    let mut current_boundary = String::new();
    for row in poi_counts.iter().take(20) {
        let boundary: String = row.get("boundary_name");
        let category: String = row.get("category");
        let count: i64 = row.get("poi_count");

        if boundary != current_boundary {
            info!("    {}:", boundary);
            current_boundary = boundary;
        }
        info!("      {}: {} POIs", category, count);
    }

    // Step 8: Export results
    info!("Step 8: Exporting analysis results");

    std::fs::create_dir_all("output/vector_analysis")?;

    match config.output_format {
        OutputFormat::GeoJSON => {
            let driver = GeoJsonDriver::new();

            for (name, result) in &analysis_results {
                let output_path = format!("output/vector_analysis/{}_{}.geojson",
                    chrono::Local::now().format("%Y%m%d"),
                    name
                );

                info!("  Writing: {}", output_path);
                driver.write(&result, &output_path).await?;
            }
        }

        OutputFormat::GeoParquet => {
            let driver = GeoParquetDriver::new();

            for (name, result) in &analysis_results {
                let output_path = format!("output/vector_analysis/{}_{}.parquet",
                    chrono::Local::now().format("%Y%m%d"),
                    name
                );

                info!("  Writing: {}", output_path);
                driver.write(&result, &output_path).await?;
            }
        }

        OutputFormat::Database => {
            info!("  Results stored in database tables");
        }
    }

    // Step 9: Generate analysis report
    info!("Step 9: Generating analysis report");

    let report = AnalysisReport {
        timestamp: chrono::Local::now(),
        input_datasets: vec![
            ("POIs".to_string(), poi_count),
            ("Roads".to_string(), road_count),
            ("Boundaries".to_string(), boundary_count),
            ("Land Use".to_string(), landuse_count),
        ],
        analyses_performed: config.analysis_tasks.len(),
        total_processing_time: std::time::Instant::now().elapsed(),
        results_summary: analysis_results.iter()
            .map(|(name, result)| (name.to_string(), result.feature_count()))
            .collect(),
    };

    let report_path = format!("output/vector_analysis/{}_report.json",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );

    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, report_json)?;

    info!("  Report saved to: {}", report_path);

    // Print summary
    info!("");
    info!("=== Analysis Summary ===");
    info!("  Input datasets: {}", report.input_datasets.len());
    info!("  Total features processed: {}",
          report.input_datasets.iter().map(|(_, count)| count).sum::<usize>());
    info!("  Analyses performed: {}", report.analyses_performed);
    info!("  Results generated: {}", report.results_summary.len());

    info!("");
    info!("✓ Vector analysis completed successfully!");

    Ok(())
}

/// Configuration for vector analysis
#[derive(Debug, Clone)]
struct AnalysisConfig {
    database: PostGisConfig,
    input_data: InputData,
    analysis_tasks: Vec<AnalysisTask>,
    output_format: OutputFormat,
    create_indexes: bool,
    validate_topology: bool,
}

#[derive(Debug, Clone)]
struct InputData {
    points_of_interest: &'static str,
    road_network: &'static str,
    administrative_boundaries: &'static str,
    land_use: &'static str,
}

#[derive(Debug, Clone)]
enum AnalysisTask {
    ProximityAnalysis { search_radius_meters: f64 },
    SpatialJoin { method: JoinMethod },
    BufferAnalysis { buffer_distance_meters: f64 },
    HotspotDetection { threshold: f64 },
    NetworkAnalysis { analysis_type: NetworkAnalysisType },
}

#[derive(Debug, Clone)]
enum JoinMethod {
    Intersects,
    Contains,
    Within,
    Overlaps,
}

#[derive(Debug, Clone)]
enum NetworkAnalysisType {
    ShortestPath,
    ServiceArea,
    Connectivity,
}

#[derive(Debug, Clone)]
enum OutputFormat {
    GeoJSON,
    GeoParquet,
    Database,
}

#[derive(Debug, serde::Serialize)]
struct AnalysisReport {
    timestamp: chrono::DateTime<chrono::Local>,
    input_datasets: Vec<(String, usize)>,
    analyses_performed: usize,
    #[serde(skip)]
    total_processing_time: std::time::Duration,
    results_summary: HashMap<String, usize>,
}

/// Perform proximity analysis
async fn proximity_analysis(
    conn: &PostGisConnection,
    radius: f64,
) -> Result<VectorDataset, Box<dyn std::error::Error>> {
    let query = format!(
        "SELECT p.*, r.name as nearest_road
         FROM pois p
         JOIN LATERAL (
             SELECT name
             FROM roads
             WHERE ST_DWithin(p.geom, roads.geom, {})
             ORDER BY ST_Distance(p.geom, roads.geom)
             LIMIT 1
         ) r ON true",
        radius
    );

    let features = conn.execute_query(&query, &[]).await?;
    Ok(VectorDataset::from_features(features))
}

/// Perform spatial join
async fn spatial_join(
    conn: &PostGisConnection,
    method: &JoinMethod,
) -> Result<VectorDataset, Box<dyn std::error::Error>> {
    let predicate = match method {
        JoinMethod::Intersects => "ST_Intersects",
        JoinMethod::Contains => "ST_Contains",
        JoinMethod::Within => "ST_Within",
        JoinMethod::Overlaps => "ST_Overlaps",
    };

    let query = format!(
        "SELECT p.*, b.name as boundary_name
         FROM pois p
         JOIN boundaries b ON {}(b.geom, p.geom)",
        predicate
    );

    let features = conn.execute_query(&query, &[]).await?;
    Ok(VectorDataset::from_features(features))
}

/// Perform buffer analysis
async fn buffer_analysis(
    conn: &PostGisConnection,
    distance: f64,
) -> Result<VectorDataset, Box<dyn std::error::Error>> {
    let query = format!(
        "SELECT id, name, ST_Buffer(geom, {}) as geom
         FROM roads",
        distance
    );

    let features = conn.execute_query(&query, &[]).await?;
    Ok(VectorDataset::from_features(features))
}

/// Perform hotspot detection
async fn hotspot_detection(
    conn: &PostGisConnection,
    threshold: f64,
) -> Result<HotspotResult, Box<dyn std::error::Error>> {
    let analyzer = HotspotAnalysis::new();

    // Load POI points
    let pois = conn.load_table("pois").await?;

    // Run Getis-Ord Gi* analysis
    let hotspots = analyzer.getis_ord_gi_star(&pois, threshold).await?;

    Ok(HotspotResult {
        hotspots: VectorDataset::from_features(hotspots),
        hotspot_count: hotspots.len(),
    })
}

/// Perform network analysis
async fn network_analysis(
    conn: &PostGisConnection,
    analysis_type: &NetworkAnalysisType,
) -> Result<VectorDataset, Box<dyn std::error::Error>> {
    match analysis_type {
        NetworkAnalysisType::ShortestPath => {
            // Example: Find shortest path between two points
            let query = "
                WITH start_point AS (SELECT geom FROM pois WHERE id = 1),
                     end_point AS (SELECT geom FROM pois WHERE id = 100)
                SELECT ST_ShortestLine(s.geom, e.geom) as geom
                FROM start_point s, end_point e
            ";

            let features = conn.execute_query(query, &[]).await?;
            Ok(VectorDataset::from_features(features))
        }

        NetworkAnalysisType::ServiceArea => {
            // Calculate reachable area from a point within distance
            let query = "
                SELECT ST_Union(ST_Buffer(geom, 1000)) as geom
                FROM roads
                WHERE ST_DWithin(geom, (SELECT geom FROM pois WHERE id = 1), 5000)
            ";

            let features = conn.execute_query(query, &[]).await?;
            Ok(VectorDataset::from_features(features))
        }

        NetworkAnalysisType::Connectivity => {
            // Analyze road network connectivity
            let query = "
                SELECT r.*, COUNT(DISTINCT r2.id) as connected_segments
                FROM roads r
                LEFT JOIN roads r2 ON ST_Touches(r.geom, r2.geom)
                GROUP BY r.id
                HAVING COUNT(DISTINCT r2.id) <= 1
            ";

            let features = conn.execute_query(query, &[]).await?;
            Ok(VectorDataset::from_features(features))
        }
    }
}

#[derive(Debug)]
struct HotspotResult {
    hotspots: VectorDataset,
    hotspot_count: usize,
}
