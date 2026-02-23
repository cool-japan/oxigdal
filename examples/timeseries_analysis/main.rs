//! Time-Series Analysis Example
//!
//! This example demonstrates comprehensive temporal raster analysis including:
//! - Loading multi-temporal satellite imagery
//! - Building temporal data cubes
//! - Trend analysis and seasonality detection
//! - Anomaly detection
//! - Change detection (breakpoint analysis)
//! - Time-series forecasting
//! - Gap filling and temporal interpolation
//! - Temporal compositing (median, mean, max)
//! - Exporting analysis results

use oxigdal_core::{Dataset, DataType};
use oxigdal_temporal::{
    TimeSeriesCollection, DataCube, TrendAnalysis, SeasonalityDetection,
    AnomalyDetection, BreakpointDetection, GapFilling, Compositing,
    TimeSeriesForecast,
};
use oxigdal_sensors::SpectralIndex;
use oxigdal_algorithms::{Statistics, RasterCalculator};
use oxigdal_geotiff::GeoTiffDriver;
use oxigdal_zarr::ZarrDriver;
use std::path::{Path, PathBuf};
use chrono::{NaiveDate, Datelike};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("timeseries_analysis=info")
        .init();

    info!("Starting Time-Series Analysis");

    // Configuration
    let config = AnalysisConfig {
        input_pattern: "data/timeseries/landsat_ndvi_*.tif",
        time_range: TimeRange {
            start: NaiveDate::from_ymd_opt(2020, 1, 1).ok_or("Invalid date")?,
            end: NaiveDate::from_ymd_opt(2024, 12, 31).ok_or("Invalid date")?,
        },
        temporal_resolution: TemporalResolution::BiWeekly,
        index_type: SpectralIndex::Ndvi,
        region_of_interest: Some("data/roi.geojson".to_string()),
        analyses: vec![
            TimeSeriesAnalysis::TrendDetection {
                method: TrendMethod::MannKendall,
            },
            TimeSeriesAnalysis::SeasonalDecomposition {
                period: 12, // Monthly data
            },
            TimeSeriesAnalysis::AnomalyDetection {
                method: AnomalyMethod::ZScore { threshold: 3.0 },
            },
            TimeSeriesAnalysis::ChangeDetection {
                method: ChangeMethod::Bfast,
            },
            TimeSeriesAnalysis::Forecasting {
                horizon: 12, // 12 periods ahead
                method: ForecastMethod::ExponentialSmoothing,
            },
        ],
        gap_filling: GapFillingConfig {
            enabled: true,
            method: GapFillingMethod::HarmonicInterpolation,
            max_gap_size: 3, // Up to 3 missing observations
        },
        compositing: CompositingConfig {
            enabled: true,
            window_size: 30, // 30-day windows
            method: CompositeMethod::Median,
        },
        output_dir: PathBuf::from("output/timeseries"),
        export_format: ExportFormat::Zarr,
        generate_plots: true,
    };

    // Step 1: Discover and load time series data
    info!("Step 1: Loading time-series data");

    let file_paths = discover_timeseries_files(&config.input_pattern)?;
    info!("  Found {} temporal observations", file_paths.len());

    if file_paths.is_empty() {
        return Err("No time-series data found".into());
    }

    // Parse dates from filenames or metadata
    let mut timeseries_data = Vec::new();
    for path in &file_paths {
        let date = extract_date_from_filename(path)?;
        let dataset = Dataset::open(path).await?;

        timeseries_data.push((date, dataset));
        info!("    {}: {}", date, path.display());
    }

    // Sort by date
    timeseries_data.sort_by_key(|(date, _)| *date);

    info!("  Time range: {} to {}",
          timeseries_data.first().map(|(d, _)| d).ok_or("Empty")?,
          timeseries_data.last().map(|(d, _)| d).ok_or("Empty")?);

    // Step 2: Build temporal data cube
    info!("Step 2: Building temporal data cube");

    let mut data_cube = DataCube::new();

    for (date, dataset) in &timeseries_data {
        let raster = dataset.read_band(1).await?;
        data_cube.add_layer(*date, raster)?;
    }

    info!("  Data cube dimensions:");
    info!("    Temporal: {} observations", data_cube.temporal_size());
    info!("    Spatial: {} x {} pixels", data_cube.width(), data_cube.height());
    info!("    Total cells: {}", data_cube.total_cells());

    // Check for gaps
    let gaps = data_cube.find_temporal_gaps(&config.temporal_resolution)?;
    if !gaps.is_empty() {
        warn!("  Found {} temporal gaps", gaps.len());
        for gap in gaps.iter().take(5) {
            warn!("    Gap: {} to {}", gap.start, gap.end);
        }
    }

    // Step 3: Quality control and gap filling
    if config.gap_filling.enabled && !gaps.is_empty() {
        info!("Step 3: Filling temporal gaps");

        let gap_filler = match config.gap_filling.method {
            GapFillingMethod::LinearInterpolation => {
                GapFilling::linear()
            }
            GapFillingMethod::SplineInterpolation => {
                GapFilling::spline()
            }
            GapFillingMethod::HarmonicInterpolation => {
                GapFilling::harmonic(3) // 3 harmonics
            }
        };

        let filled_count = gap_filler.fill(&mut data_cube, config.gap_filling.max_gap_size).await?;
        info!("  Filled {} gaps", filled_count);
    } else {
        info!("Step 3: Skipping gap filling");
    }

    // Step 4: Temporal compositing
    if config.compositing.enabled {
        info!("Step 4: Generating temporal composites");

        let compositor = match config.compositing.method {
            CompositeMethod::Mean => Compositing::mean(),
            CompositeMethod::Median => Compositing::median(),
            CompositeMethod::Max => Compositing::max(),
            CompositeMethod::MaxNdvi => Compositing::max_ndvi(),
        };

        let composites = compositor.create_composites(
            &data_cube,
            config.compositing.window_size,
        ).await?;

        info!("  Created {} composite images", composites.len());

        // Export composites
        for (date, composite) in &composites {
            let output_path = config.output_dir.join(format!("composite_{}.tif", date.format("%Y%m%d")));
            save_raster(&composite, &output_path).await?;
        }
    } else {
        info!("Step 4: Skipping temporal compositing");
    }

    // Step 5: Perform time-series analyses
    info!("Step 5: Performing time-series analyses");

    let mut analysis_results = TimeSeriesResults::new();

    for (idx, analysis) in config.analyses.iter().enumerate() {
        info!("  Analysis {}/{}: {:?}", idx + 1, config.analyses.len(), analysis);

        match analysis {
            TimeSeriesAnalysis::TrendDetection { method } => {
                let analyzer = TrendAnalysis::new();

                let trend = match method {
                    TrendMethod::Linear => analyzer.linear_trend(&data_cube).await?,
                    TrendMethod::MannKendall => analyzer.mann_kendall(&data_cube).await?,
                    TrendMethod::SenSlope => analyzer.sen_slope(&data_cube).await?,
                };

                info!("    Trend statistics:");
                info!("      Pixels with significant trend: {}/{} ({:.1}%)",
                      trend.significant_pixels, trend.total_pixels,
                      trend.significant_pixels as f64 / trend.total_pixels as f64 * 100.0);
                info!("      Mean slope: {:.6} per year", trend.mean_slope);
                info!("      Positive trends: {:.1}%", trend.positive_trend_pct);
                info!("      Negative trends: {:.1}%", trend.negative_trend_pct);

                analysis_results.trend = Some(trend);
            }

            TimeSeriesAnalysis::SeasonalDecomposition { period } => {
                let analyzer = SeasonalityDetection::new();

                let seasonal = analyzer.decompose(&data_cube, *period).await?;

                info!("    Seasonal decomposition complete:");
                info!("      Seasonal amplitude (mean): {:.4}", seasonal.seasonal_amplitude);
                info!("      Seasonality strength: {:.2}%", seasonal.seasonality_strength * 100.0);

                analysis_results.seasonal = Some(seasonal);
            }

            TimeSeriesAnalysis::AnomalyDetection { method } => {
                let detector = match method {
                    AnomalyMethod::ZScore { threshold } => {
                        AnomalyDetection::z_score(*threshold)
                    }
                    AnomalyMethod::IsolationForest => {
                        AnomalyDetection::isolation_forest()
                    }
                    AnomalyMethod::Iqr { multiplier } => {
                        AnomalyDetection::iqr(*multiplier)
                    }
                };

                let anomalies = detector.detect(&data_cube).await?;

                info!("    Detected {} temporal anomalies", anomalies.len());

                for (idx, anomaly) in anomalies.iter().take(10).enumerate() {
                    info!("      {}: {} (score: {:.2})",
                          idx + 1, anomaly.date, anomaly.score);
                }

                analysis_results.anomalies = anomalies;
            }

            TimeSeriesAnalysis::ChangeDetection { method } => {
                let detector = match method {
                    ChangeMethod::Bfast => BreakpointDetection::bfast(),
                    ChangeMethod::Cusum => BreakpointDetection::cusum(),
                    ChangeMethod::Ewma => BreakpointDetection::ewma(),
                };

                let changes = detector.detect(&data_cube).await?;

                info!("    Detected {} change events", changes.breakpoints.len());

                for (idx, breakpoint) in changes.breakpoints.iter().take(5).enumerate() {
                    info!("      {}: {} (magnitude: {:.4})",
                          idx + 1, breakpoint.date, breakpoint.magnitude);
                }

                analysis_results.changes = Some(changes);
            }

            TimeSeriesAnalysis::Forecasting { horizon, method } => {
                let forecaster = match method {
                    ForecastMethod::ExponentialSmoothing => {
                        TimeSeriesForecast::exponential_smoothing()
                    }
                    ForecastMethod::Arima { p, d, q } => {
                        TimeSeriesForecast::arima(*p, *d, *q)
                    }
                    ForecastMethod::Prophet => {
                        TimeSeriesForecast::prophet()
                    }
                };

                let forecast = forecaster.predict(&data_cube, *horizon).await?;

                info!("    Forecast generated:");
                info!("      Horizon: {} periods", horizon);
                info!("      Predictions: {}", forecast.predictions.len());
                info!("      Mean absolute error: {:.4}", forecast.mae);
                info!("      RMSE: {:.4}", forecast.rmse);

                analysis_results.forecast = Some(forecast);
            }
        }
    }

    // Step 6: Calculate per-pixel statistics
    info!("Step 6: Calculating temporal statistics");

    let stats = calculate_temporal_statistics(&data_cube).await?;

    info!("  Temporal statistics:");
    info!("    Mean: min={:.4}, max={:.4}, mean={:.4}",
          stats.mean_min, stats.mean_max, stats.mean_mean);
    info!("    Std Dev: min={:.4}, max={:.4}, mean={:.4}",
          stats.std_min, stats.std_max, stats.std_mean);
    info!("    Coefficient of Variation: mean={:.2}%", stats.cv_mean * 100.0);

    // Step 7: Export results
    info!("Step 7: Exporting analysis results");

    std::fs::create_dir_all(&config.output_dir)?;

    match config.export_format {
        ExportFormat::GeoTiff => {
            let driver = GeoTiffDriver::new();

            // Export trend map
            if let Some(ref trend) = analysis_results.trend {
                let path = config.output_dir.join("trend_slope.tif");
                info!("  Writing: {}", path.display());
                driver.write(&trend.slope_map, &path).await?;

                let path = config.output_dir.join("trend_pvalue.tif");
                driver.write(&trend.pvalue_map, &path).await?;
            }

            // Export seasonal components
            if let Some(ref seasonal) = analysis_results.seasonal {
                let path = config.output_dir.join("seasonal_amplitude.tif");
                driver.write(&seasonal.seasonal_component, &path).await?;

                let path = config.output_dir.join("trend_component.tif");
                driver.write(&seasonal.trend_component, &path).await?;
            }

            // Export change detection
            if let Some(ref changes) = analysis_results.changes {
                let path = config.output_dir.join("change_magnitude.tif");
                driver.write(&changes.magnitude_map, &path).await?;

                let path = config.output_dir.join("change_date.tif");
                driver.write(&changes.date_map, &path).await?;
            }
        }

        ExportFormat::Zarr => {
            info!("  Exporting as Zarr data cube");

            let zarr_path = config.output_dir.join("timeseries_analysis.zarr");
            let driver = ZarrDriver::new();

            // Export entire data cube
            driver.write_datacube(&data_cube, &zarr_path).await?;

            // Export analysis results as separate arrays
            if let Some(ref trend) = analysis_results.trend {
                driver.write_array(&trend.slope_map, &zarr_path, "trend_slope").await?;
                driver.write_array(&trend.pvalue_map, &zarr_path, "trend_pvalue").await?;
            }

            info!("    Zarr cube written to: {}", zarr_path.display());
        }

        ExportFormat::NetCdf => {
            info!("  Exporting as NetCDF");

            let nc_path = config.output_dir.join("timeseries_analysis.nc");
            export_to_netcdf(&data_cube, &analysis_results, &nc_path).await?;

            info!("    NetCDF written to: {}", nc_path.display());
        }
    }

    // Step 8: Generate plots and visualizations
    if config.generate_plots {
        info!("Step 8: Generating visualization plots");

        generate_time_series_plot(&data_cube, &config.output_dir).await?;

        if let Some(ref trend) = analysis_results.trend {
            generate_trend_plot(trend, &config.output_dir).await?;
        }

        if let Some(ref seasonal) = analysis_results.seasonal {
            generate_seasonal_plot(seasonal, &config.output_dir).await?;
        }

        if !analysis_results.anomalies.is_empty() {
            generate_anomaly_plot(&analysis_results.anomalies, &config.output_dir).await?;
        }

        info!("  Plots saved to: {}", config.output_dir.display());
    } else {
        info!("Step 8: Skipping plot generation");
    }

    // Step 9: Generate summary report
    info!("Step 9: Generating analysis report");

    let report = AnalysisReport {
        time_range: config.time_range,
        observations: data_cube.temporal_size(),
        spatial_extent: (data_cube.width(), data_cube.height()),
        analyses_performed: config.analyses.len(),
        gaps_filled: gaps.len(),
        trend_summary: analysis_results.trend.as_ref().map(|t| TrendSummary {
            significant_pixels: t.significant_pixels,
            mean_slope: t.mean_slope,
            positive_trend_pct: t.positive_trend_pct,
        }),
        anomalies_detected: analysis_results.anomalies.len(),
        change_events: analysis_results.changes.as_ref()
            .map(|c| c.breakpoints.len())
            .unwrap_or(0),
    };

    let report_path = config.output_dir.join(format!(
        "analysis_report_{}.json",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    ));

    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, report_json)?;

    info!("  Report saved to: {}", report_path.display());

    // Print summary
    info!("");
    info!("=== Analysis Summary ===");
    info!("  Time range: {} to {}", config.time_range.start, config.time_range.end);
    info!("  Observations: {}", report.observations);
    info!("  Spatial extent: {} x {} pixels", report.spatial_extent.0, report.spatial_extent.1);
    info!("  Analyses performed: {}", report.analyses_performed);

    if let Some(ref trend_summary) = report.trend_summary {
        info!("  Trend: {:.1}% pixels significant, mean slope: {:.6}",
              trend_summary.significant_pixels as f64 / (report.spatial_extent.0 * report.spatial_extent.1) as f64 * 100.0,
              trend_summary.mean_slope);
    }

    if report.anomalies_detected > 0 {
        info!("  Anomalies detected: {}", report.anomalies_detected);
    }

    if report.change_events > 0 {
        info!("  Change events: {}", report.change_events);
    }

    info!("");
    info!("✓ Time-series analysis completed successfully!");

    Ok(())
}

/// Configuration for time-series analysis
#[derive(Debug, Clone)]
struct AnalysisConfig {
    input_pattern: &'static str,
    time_range: TimeRange,
    temporal_resolution: TemporalResolution,
    index_type: SpectralIndex,
    region_of_interest: Option<String>,
    analyses: Vec<TimeSeriesAnalysis>,
    gap_filling: GapFillingConfig,
    compositing: CompositingConfig,
    output_dir: PathBuf,
    export_format: ExportFormat,
    generate_plots: bool,
}

#[derive(Debug, Clone, Copy)]
struct TimeRange {
    start: NaiveDate,
    end: NaiveDate,
}

#[derive(Debug, Clone, Copy)]
enum TemporalResolution {
    Daily,
    Weekly,
    BiWeekly,
    Monthly,
    Seasonal,
}

#[derive(Debug, Clone)]
enum TimeSeriesAnalysis {
    TrendDetection { method: TrendMethod },
    SeasonalDecomposition { period: usize },
    AnomalyDetection { method: AnomalyMethod },
    ChangeDetection { method: ChangeMethod },
    Forecasting { horizon: usize, method: ForecastMethod },
}

#[derive(Debug, Clone)]
enum TrendMethod {
    Linear,
    MannKendall,
    SenSlope,
}

#[derive(Debug, Clone)]
enum AnomalyMethod {
    ZScore { threshold: f64 },
    IsolationForest,
    Iqr { multiplier: f64 },
}

#[derive(Debug, Clone)]
enum ChangeMethod {
    Bfast,
    Cusum,
    Ewma,
}

#[derive(Debug, Clone)]
enum ForecastMethod {
    ExponentialSmoothing,
    Arima { p: usize, d: usize, q: usize },
    Prophet,
}

#[derive(Debug, Clone)]
struct GapFillingConfig {
    enabled: bool,
    method: GapFillingMethod,
    max_gap_size: usize,
}

#[derive(Debug, Clone)]
enum GapFillingMethod {
    LinearInterpolation,
    SplineInterpolation,
    HarmonicInterpolation,
}

#[derive(Debug, Clone)]
struct CompositingConfig {
    enabled: bool,
    window_size: u32,
    method: CompositeMethod,
}

#[derive(Debug, Clone)]
enum CompositeMethod {
    Mean,
    Median,
    Max,
    MaxNdvi,
}

#[derive(Debug, Clone)]
enum ExportFormat {
    GeoTiff,
    Zarr,
    NetCdf,
}

#[derive(Debug)]
struct TimeSeriesResults {
    trend: Option<TrendResult>,
    seasonal: Option<SeasonalResult>,
    anomalies: Vec<Anomaly>,
    changes: Option<ChangeResult>,
    forecast: Option<ForecastResult>,
}

impl TimeSeriesResults {
    fn new() -> Self {
        Self {
            trend: None,
            seasonal: None,
            anomalies: Vec::new(),
            changes: None,
            forecast: None,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct AnalysisReport {
    time_range: TimeRange,
    observations: usize,
    spatial_extent: (u32, u32),
    analyses_performed: usize,
    gaps_filled: usize,
    trend_summary: Option<TrendSummary>,
    anomalies_detected: usize,
    change_events: usize,
}

#[derive(Debug, serde::Serialize)]
struct TrendSummary {
    significant_pixels: usize,
    mean_slope: f64,
    positive_trend_pct: f64,
}

/// Discover time-series files matching pattern
fn discover_timeseries_files(pattern: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let paths = glob::glob(pattern)?
        .filter_map(|result| result.ok())
        .collect();

    Ok(paths)
}

/// Extract date from filename
fn extract_date_from_filename(path: &Path) -> Result<NaiveDate, Box<dyn std::error::Error>> {
    // Expected format: landsat_ndvi_YYYYMMDD.tif
    let filename = path.file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Invalid filename")?;

    let date_str = filename.split('_').last()
        .ok_or("No date in filename")?;

    let year = date_str[0..4].parse::<i32>()?;
    let month = date_str[4..6].parse::<u32>()?;
    let day = date_str[6..8].parse::<u32>()?;

    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| "Invalid date".into())
}

/// Calculate temporal statistics across the data cube
async fn calculate_temporal_statistics(
    data_cube: &DataCube,
) -> Result<TemporalStatistics, Box<dyn std::error::Error>> {
    let stats_calc = Statistics::new();

    let mean_map = stats_calc.temporal_mean(data_cube).await?;
    let std_map = stats_calc.temporal_std(data_cube).await?;
    let cv_map = stats_calc.temporal_cv(data_cube).await?;

    let mean_stats = stats_calc.compute(&mean_map).await?;
    let std_stats = stats_calc.compute(&std_map).await?;
    let cv_stats = stats_calc.compute(&cv_map).await?;

    Ok(TemporalStatistics {
        mean_min: mean_stats.min,
        mean_max: mean_stats.max,
        mean_mean: mean_stats.mean,
        std_min: std_stats.min,
        std_max: std_stats.max,
        std_mean: std_stats.mean,
        cv_mean: cv_stats.mean,
    })
}

#[derive(Debug)]
struct TemporalStatistics {
    mean_min: f64,
    mean_max: f64,
    mean_mean: f64,
    std_min: f64,
    std_max: f64,
    std_mean: f64,
    cv_mean: f64,
}

// Placeholder implementations for demonstration
async fn save_raster(_raster: &Dataset, _path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn export_to_netcdf(
    _data_cube: &DataCube,
    _results: &TimeSeriesResults,
    _path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn generate_time_series_plot(_data_cube: &DataCube, _output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn generate_trend_plot(_trend: &TrendResult, _output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn generate_seasonal_plot(_seasonal: &SeasonalResult, _output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

async fn generate_anomaly_plot(_anomalies: &[Anomaly], _output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

// Type placeholders
struct TrendResult {
    slope_map: Dataset,
    pvalue_map: Dataset,
    significant_pixels: usize,
    total_pixels: usize,
    mean_slope: f64,
    positive_trend_pct: f64,
    negative_trend_pct: f64,
}

struct SeasonalResult {
    seasonal_component: Dataset,
    trend_component: Dataset,
    seasonal_amplitude: f64,
    seasonality_strength: f64,
}

struct Anomaly {
    date: NaiveDate,
    score: f64,
}

struct ChangeResult {
    breakpoints: Vec<Breakpoint>,
    magnitude_map: Dataset,
    date_map: Dataset,
}

struct Breakpoint {
    date: NaiveDate,
    magnitude: f64,
}

struct ForecastResult {
    predictions: Vec<f64>,
    mae: f64,
    rmse: f64,
}

impl serde::Serialize for TimeRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TimeRange", 2)?;
        state.serialize_field("start", &self.start.to_string())?;
        state.serialize_field("end", &self.end.to_string())?;
        state.end()
    }
}
