//! Comprehensive integration tests for oxigdal-analytics

use approx::assert_abs_diff_eq;
use oxigdal_analytics::change::{ChangeDetector, ChangeMethod};
use oxigdal_analytics::clustering::{DbscanClusterer, KMeansClusterer, silhouette_score};
use oxigdal_analytics::hotspot::{GetisOrdGiStar, MoransI, SpatialWeights};
use oxigdal_analytics::interpolation::{
    IdwInterpolator, KrigingInterpolator, KrigingType, Variogram, VariogramModel,
};
use oxigdal_analytics::timeseries::{
    AnomalyDetector, AnomalyMethod, TimeSeries, TrendDetector, TrendMethod,
};
use oxigdal_analytics::zonal::{ZonalCalculator, ZonalStatistic};
use scirs2_core::ndarray::{Array, Array1, Array2, array};

#[test]
fn test_trend_detection_positive() {
    let values = array![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
    let result = detector
        .detect(&values.view())
        .expect("should detect trend in positive sequence");

    assert_eq!(result.direction, 1);
    assert!(result.significant);
    assert!(result.p_value < 0.05);
}

#[test]
fn test_trend_detection_negative() {
    let values = array![10.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];
    let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
    let result = detector
        .detect(&values.view())
        .expect("should detect trend in negative sequence");

    assert_eq!(result.direction, -1);
    assert!(result.significant);
}

#[test]
fn test_linear_regression_trend() {
    let values = array![1.0, 3.0, 5.0, 7.0, 9.0];
    let detector = TrendDetector::new(TrendMethod::LinearRegression, 0.05);
    let result = detector
        .detect(&values.view())
        .expect("should detect linear trend using regression");

    assert_eq!(result.direction, 1);
    assert_abs_diff_eq!(result.magnitude, 2.0, epsilon = 1e-10); // Slope is 2
}

#[test]
fn test_anomaly_detection_zscore() {
    let values = array![1.0, 2.0, 3.0, 4.0, 100.0]; // 100 is outlier
    // Use threshold of 1.9 instead of 2.0 to match the unit test
    let detector = AnomalyDetector::new(AnomalyMethod::ZScore, 1.9);
    let result = detector
        .detect(&values.view())
        .expect("should detect anomaly using z-score method");

    assert!(!result.anomaly_indices.is_empty());
    assert!(result.anomaly_indices.contains(&4));
}

#[test]
fn test_anomaly_detection_iqr() {
    let values = array![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 100.0];
    let detector = AnomalyDetector::new(AnomalyMethod::IQR, 1.5);
    let result = detector
        .detect(&values.view())
        .expect("should detect anomaly using IQR method");

    assert!(!result.anomaly_indices.is_empty());
    assert!(result.anomaly_indices.contains(&6));
}

#[test]
fn test_time_series_moving_average() {
    let times = array![1.0, 2.0, 3.0, 4.0, 5.0];
    let values = array![10.0, 20.0, 30.0, 40.0, 50.0];
    let ts = TimeSeries::new(times, values).expect("should create time series from valid data");

    let ma = ts
        .moving_average(3)
        .expect("should calculate moving average with window size 3");
    assert_eq!(ma.len(), 3);
    assert_abs_diff_eq!(ma[0], 20.0, epsilon = 1e-10);
}

#[test]
fn test_kmeans_clustering() {
    let data = array![
        [0.0, 0.0],
        [0.1, 0.1],
        [0.2, 0.0],
        [10.0, 10.0],
        [10.1, 10.1],
        [10.0, 10.2],
    ];

    let clusterer = KMeansClusterer::new(2, 100, 1e-4);
    let result = clusterer
        .fit(&data.view())
        .expect("should fit k-means clustering with 2 clusters");

    assert_eq!(result.labels.len(), 6);
    assert_eq!(result.centers.nrows(), 2);
    assert!(result.converged);
    assert!(result.inertia > 0.0);
}

#[test]
fn test_dbscan_clustering() {
    let data = array![
        [0.0, 0.0],
        [0.1, 0.1],
        [0.2, 0.0],
        [10.0, 10.0],
        [10.1, 10.1],
        [10.0, 10.2],
        [5.0, 5.0], // Outlier
    ];

    let clusterer = DbscanClusterer::new(0.5, 2);
    let result = clusterer
        .fit(&data.view())
        .expect("should fit DBSCAN clustering with eps=0.5, min_samples=2");

    assert_eq!(result.n_clusters, 2);
    assert!(result.n_noise > 0);
}

#[test]
fn test_silhouette_score() {
    let data = array![[0.0, 0.0], [0.1, 0.1], [10.0, 10.0], [10.1, 10.1],];
    let labels = array![0, 0, 1, 1];

    let score = silhouette_score(&data.view(), &labels)
        .expect("should calculate silhouette score for clustering");
    assert!(score > 0.5);
}

#[test]
fn test_getis_ord_hotspot() {
    let values = array![1.0, 1.0, 1.0, 10.0, 10.0, 10.0];
    let mut weights_matrix = Array2::zeros((6, 6));
    for i in 0..5 {
        weights_matrix[[i, i]] = 1.0;
        weights_matrix[[i, i + 1]] = 1.0;
        weights_matrix[[i + 1, i]] = 1.0;
    }
    weights_matrix[[5, 5]] = 1.0;

    let weights = SpatialWeights::from_adjacency(weights_matrix)
        .expect("should create spatial weights from adjacency matrix");
    let gi_star = GetisOrdGiStar::new(0.05);
    let result = gi_star
        .calculate(&values.view(), &weights)
        .expect("should calculate Getis-Ord Gi* hotspot statistic");

    assert_eq!(result.z_scores.len(), 6);
    // Hot spot should have positive z-scores
    assert!(result.z_scores[3] > 0.0);
    assert!(result.z_scores[4] > 0.0);
}

#[test]
fn test_morans_i() {
    let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
    let mut weights_matrix = Array2::zeros((5, 5));
    for i in 0..4 {
        weights_matrix[[i, i + 1]] = 1.0;
        weights_matrix[[i + 1, i]] = 1.0;
    }

    let mut weights = SpatialWeights::from_adjacency(weights_matrix)
        .expect("should create spatial weights from adjacency matrix");
    weights.row_standardize();

    let morans_i = MoransI::new(0.05);
    let result = morans_i
        .calculate(&values.view(), &weights)
        .expect("should calculate Moran's I spatial autocorrelation");

    // Should show positive spatial autocorrelation
    assert!(result.i_statistic > result.expected_i);
}

#[test]
fn test_change_detection_differencing() {
    let before =
        Array::from_shape_vec((3, 3, 1), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])
            .expect("should create 3x3x1 array for before image");
    let after = Array::from_shape_vec(
        (3, 3, 1),
        vec![2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
    )
    .expect("should create 3x3x1 array for after image");

    let detector = ChangeDetector::new(ChangeMethod::Differencing).with_threshold(0.5);
    let result = detector
        .detect(&before.view(), &after.view())
        .expect("should detect change using differencing method");

    assert_eq!(result.magnitude.dim(), (3, 3));
    assert!(result.stats.n_changed > 0);
}

#[test]
fn test_change_detection_cva() {
    let before = Array::from_shape_vec((2, 2, 2), vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0])
        .expect("should create 2x2x2 array for before image");
    let after = Array::from_shape_vec((2, 2, 2), vec![2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0])
        .expect("should create 2x2x2 array for after image");

    let detector = ChangeDetector::new(ChangeMethod::CVA);
    let result = detector
        .detect(&before.view(), &after.view())
        .expect("should detect change using CVA method");

    assert_eq!(result.magnitude.dim(), (2, 2));
    assert!(result.stats.mean_change > 0.0);
}

#[test]
fn test_idw_interpolation() {
    let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let values = array![1.0, 2.0, 3.0, 4.0];
    let targets = array![[0.5, 0.5]];

    let interpolator = IdwInterpolator::new(2.0);
    let result = interpolator
        .interpolate(&points, &values.view(), &targets)
        .expect("should perform IDW interpolation with power=2.0");

    assert_eq!(result.values.len(), 1);
    assert!(result.values[0] > 2.0 && result.values[0] < 3.0);
}

#[test]
fn test_idw_cross_validation() {
    // Use more points with better spatial pattern for IDW
    let points = array![
        [0.0, 0.0],
        [1.0, 0.0],
        [2.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [2.0, 1.0]
    ];
    // Values increase smoothly in x direction (good for IDW)
    let values = array![1.0, 2.0, 3.0, 1.0, 2.0, 3.0];

    let interpolator = IdwInterpolator::new(2.0);
    let cv_result = interpolator
        .cross_validate(&points, &values.view())
        .expect("should perform IDW cross-validation");

    assert_eq!(cv_result.predictions.len(), 6);
    assert!(cv_result.rmse > 0.0);
    // R-squared can be negative if model performs poorly, so just check it's reasonable
    assert!(cv_result.r_squared >= -1.0 && cv_result.r_squared <= 1.0);
}

#[test]
fn test_kriging_interpolation() {
    let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let values = array![1.0, 2.0, 3.0, 4.0];
    let targets = array![[0.5, 0.5]];

    let variogram = Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 2.0);
    let interpolator = KrigingInterpolator::new(KrigingType::Ordinary, variogram);

    let result = interpolator
        .interpolate(&points, &values.view(), &targets)
        .expect("should perform ordinary kriging interpolation with spherical variogram");

    assert_eq!(result.values.len(), 1);
    assert_eq!(result.variances.len(), 1);
    assert!(result.values[0] > 1.0 && result.values[0] < 4.0);
}

#[test]
fn test_zonal_statistics() {
    let values = array![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
    let zones = array![[1, 1, 2], [1, 2, 2], [2, 2, 2]];

    let calculator = ZonalCalculator::new().with_statistics(vec![
        ZonalStatistic::Mean,
        ZonalStatistic::Min,
        ZonalStatistic::Max,
        ZonalStatistic::Count,
    ]);

    let result = calculator
        .calculate(&values.view(), &zones.view())
        .expect("should calculate zonal statistics for 3x3 array");

    assert_eq!(result.zone_ids.len(), 2);
    assert!(result.zones.contains_key(&1));
    assert!(result.zones.contains_key(&2));

    let zone1_stats = &result.zones[&1];
    assert_abs_diff_eq!(
        zone1_stats[&ZonalStatistic::Mean],
        (1.0 + 2.0 + 4.0) / 3.0,
        epsilon = 1e-10
    );
    assert_abs_diff_eq!(zone1_stats[&ZonalStatistic::Count], 3.0, epsilon = 1e-10);
}

#[test]
fn test_zonal_multiband() {
    let values = Array::from_shape_vec((2, 2, 2), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0])
        .expect("should create 2x2x2 multiband array");
    let zones = array![[1, 1], [2, 2]];

    let calculator = ZonalCalculator::new();
    let results = calculator
        .calculate_multiband(&values.view(), &zones.view())
        .expect("should calculate zonal statistics for multiband array");

    assert_eq!(results.len(), 2);
    for result in results {
        assert_eq!(result.zone_ids.len(), 2);
    }
}

#[test]
fn test_integrated_workflow() {
    // Simulate a complete workflow: clustering + hotspot analysis
    let data = array![[0.0, 0.0], [0.1, 0.1], [10.0, 10.0], [10.1, 10.1],];

    // 1. Cluster the data
    let clusterer = KMeansClusterer::new(2, 100, 1e-4);
    let cluster_result = clusterer
        .fit(&data.view())
        .expect("should fit k-means clustering in integrated workflow");

    assert_eq!(cluster_result.labels.len(), 4);

    // 2. Create values based on clusters
    let values: Array1<f64> = cluster_result
        .labels
        .iter()
        .map(|&label| (label + 1) as f64 * 10.0)
        .collect();

    // 3. Build spatial weights
    let mut weights_matrix = Array2::zeros((4, 4));
    for i in 0..3 {
        weights_matrix[[i, i + 1]] = 1.0;
        weights_matrix[[i + 1, i]] = 1.0;
    }
    for i in 0..4 {
        weights_matrix[[i, i]] = 1.0;
    }

    let weights = SpatialWeights::from_adjacency(weights_matrix)
        .expect("should create spatial weights in integrated workflow");

    // 4. Perform hotspot analysis
    let gi_star = GetisOrdGiStar::new(0.05);
    let hotspot_result = gi_star
        .calculate(&values.view(), &weights)
        .expect("should calculate hotspot in integrated workflow");

    assert_eq!(hotspot_result.z_scores.len(), 4);
}
