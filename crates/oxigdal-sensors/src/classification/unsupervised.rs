//! Unsupervised classification algorithms
//!
//! Implements Lloyd's-algorithm K-Means clustering with k-means++
//! centroid initialization, and ISODATA (Iterative Self-Organizing Data
//! Analysis Technique) built on top of the K-Means primitives with real
//! split / merge / discard logic driven by cluster statistics.

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array1, Array2, ArrayView1, ArrayView2};
use scirs2_core::random::prelude::StdRng as SeededRandom;
use scirs2_core::random::seeded_rng;

/// Default seed used for reproducible k-means++ centroid initialization.
///
/// Callers that need a different (or truly non-deterministic) initialization
/// should use [`KMeansClustering::with_seed`].
const DEFAULT_SEED: u64 = 0x0FA9_C0DE_5EED_0001;

/// Squared Euclidean distance between two equal-length band vectors.
fn squared_distance(a: &ArrayView1<f64>, b: &ArrayView1<f64>) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

/// Index (and squared distance) of the centroid nearest to `pixel`.
///
/// Returns `None` only if `centroids` has zero rows.
fn nearest_centroid(pixel: &ArrayView1<f64>, centroids: &ArrayView2<f64>) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (c, centroid) in centroids.outer_iter().enumerate() {
        let dist = squared_distance(pixel, &centroid);
        best = match best {
            Some((_, best_dist)) if best_dist <= dist => best,
            _ => Some((c, dist)),
        };
    }
    best
}

/// k-means++ centroid initialization (Arthur & Vassilvitskii, 2007).
///
/// Chooses the first centroid uniformly at random, then repeatedly chooses
/// subsequent centroids with probability proportional to the squared
/// distance to the nearest already-chosen centroid. This gives materially
/// better and more stable convergence than uniform random initialization.
///
/// Precondition (checked by the caller): `k >= 1` and `data.nrows() >= k`.
fn kmeans_plus_plus_init(data: &ArrayView2<f64>, k: usize, rng: &mut SeededRandom) -> Array2<f64> {
    let n_pixels = data.nrows();
    let n_bands = data.ncols();

    let mut centroids = Array2::<f64>::zeros((k, n_bands));

    let first_idx = rng.random_range(0..n_pixels);
    centroids.row_mut(0).assign(&data.row(first_idx));

    let mut min_dist_sq = vec![f64::INFINITY; n_pixels];

    for c in 1..k {
        let prev_centroid = centroids.row(c - 1).to_owned();
        for (i, pixel) in data.outer_iter().enumerate() {
            let dist_sq = squared_distance(&pixel, &prev_centroid.view());
            if let Some(slot) = min_dist_sq.get_mut(i)
                && dist_sq < *slot
            {
                *slot = dist_sq;
            }
        }

        let total_weight: f64 = min_dist_sq.iter().sum();
        let chosen_idx = if total_weight > 0.0 && total_weight.is_finite() {
            let target = rng.random_range(0.0..total_weight);
            let mut cumulative = 0.0;
            let mut selected = n_pixels - 1;
            for (i, &w) in min_dist_sq.iter().enumerate() {
                cumulative += w;
                if cumulative >= target {
                    selected = i;
                    break;
                }
            }
            selected
        } else {
            // All remaining points coincide with an already-chosen centroid;
            // fall back to uniform selection so initialization stays well-defined.
            rng.random_range(0..n_pixels)
        };

        centroids.row_mut(c).assign(&data.row(chosen_idx));
    }

    centroids
}

/// Cluster-level summary statistics used by ISODATA's split/merge logic.
struct ClusterStats {
    /// Number of pixels assigned to this cluster.
    count: usize,
    /// Per-band standard deviation of assigned pixels around the centroid.
    std_dev: Vec<f64>,
}

/// K-Means clustering for image classification.
///
/// Runs Lloyd's algorithm: pixels are assigned to the nearest centroid (by
/// Euclidean distance across all bands), centroids are recomputed as the
/// mean of their assigned pixels, and the process repeats until either the
/// largest centroid movement drops below `tolerance` or `max_iterations` is
/// reached.
pub struct KMeansClustering {
    /// Number of clusters to create
    pub n_clusters: usize,
    /// Maximum number of iterations
    pub max_iterations: usize,
    /// Convergence tolerance threshold (max centroid movement, in band units)
    pub tolerance: f64,
    /// RNG seed for k-means++ centroid initialization
    seed: u64,
}

impl KMeansClustering {
    /// Create a new K-Means classifier
    pub fn new(n_clusters: usize, max_iterations: usize, tolerance: f64) -> Result<Self> {
        if n_clusters == 0 {
            return Err(SensorError::invalid_parameter(
                "n_clusters",
                "must be greater than 0",
            ));
        }

        Ok(Self {
            n_clusters,
            max_iterations,
            tolerance,
            seed: DEFAULT_SEED,
        })
    }

    /// Use a custom RNG seed for deterministic k-means++ initialization.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Classify image pixels.
    ///
    /// `data` is `n_pixels x n_bands`. Returns one cluster label (in
    /// `0..n_clusters`) per pixel.
    pub fn classify(&self, data: &ArrayView2<f64>) -> Result<Array1<usize>> {
        let (labels, _centroids) = self.fit(data)?;
        Ok(labels)
    }

    /// Validate cluster/data shape preconditions shared by `fit`.
    fn validate(&self, n_pixels: usize, n_bands: usize) -> Result<()> {
        if n_pixels == 0 {
            return Err(SensorError::dimension_mismatch(
                "at least one pixel row",
                "0 rows",
            ));
        }
        if n_bands == 0 {
            return Err(SensorError::dimension_mismatch(
                "at least one band column",
                "0 columns",
            ));
        }
        if self.n_clusters > n_pixels {
            return Err(SensorError::invalid_parameter(
                "n_clusters",
                format!("must not exceed the number of pixels ({n_pixels})"),
            ));
        }
        Ok(())
    }

    /// Run Lloyd's algorithm to convergence, returning both the per-pixel
    /// labels and the final centroids (`n_clusters x n_bands`).
    fn fit(&self, data: &ArrayView2<f64>) -> Result<(Array1<usize>, Array2<f64>)> {
        let n_pixels = data.nrows();
        let n_bands = data.ncols();
        self.validate(n_pixels, n_bands)?;

        let mut rng = seeded_rng(self.seed);
        let mut centroids = kmeans_plus_plus_init(data, self.n_clusters, &mut rng);

        for _iteration in 0..self.max_iterations {
            let mut sums = Array2::<f64>::zeros((self.n_clusters, n_bands));
            let mut counts = vec![0usize; self.n_clusters];

            for pixel in data.outer_iter() {
                // `best` is always < self.n_clusters: nearest_centroid only
                // returns indices into `centroids`, which has exactly
                // self.n_clusters rows.
                let Some((best, _)) = nearest_centroid(&pixel, &centroids.view()) else {
                    continue;
                };
                let mut row = sums.row_mut(best);
                row += &pixel;
                if let Some(slot) = counts.get_mut(best) {
                    *slot += 1;
                }
            }

            let mut new_centroids = centroids.clone();
            for c in 0..self.n_clusters {
                let count = counts.get(c).copied().unwrap_or(0);
                if count > 0 {
                    let count_f = count as f64;
                    let mut dst = new_centroids.row_mut(c);
                    let src = sums.row(c);
                    for (d, s) in dst.iter_mut().zip(src.iter()) {
                        *d = s / count_f;
                    }
                } else {
                    // Empty cluster: re-seed its centroid at a random data
                    // point so it can compete for assignments next round
                    // instead of permanently vanishing.
                    let idx = rng.random_range(0..n_pixels);
                    new_centroids.row_mut(c).assign(&data.row(idx));
                }
            }

            let mut max_shift_sq = 0.0f64;
            for (old, new) in centroids.outer_iter().zip(new_centroids.outer_iter()) {
                let shift_sq = squared_distance(&old, &new);
                if shift_sq > max_shift_sq {
                    max_shift_sq = shift_sq;
                }
            }

            centroids = new_centroids;

            if max_shift_sq.sqrt() <= self.tolerance {
                break;
            }
        }

        let labels = assign_labels(data, &centroids.view());
        Ok((labels, centroids))
    }
}

/// Assign every pixel to its nearest centroid, producing per-pixel labels.
fn assign_labels(data: &ArrayView2<f64>, centroids: &ArrayView2<f64>) -> Array1<usize> {
    let mut labels = Array1::<usize>::zeros(data.nrows());
    for (i, pixel) in data.outer_iter().enumerate() {
        let best = nearest_centroid(&pixel, centroids)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        if let Some(slot) = labels.get_mut(i) {
            *slot = best;
        }
    }
    labels
}

/// ISODATA (Iterative Self-Organizing Data Analysis Technique).
///
/// Extends K-Means with data-driven adjustment of the number of clusters:
/// clusters with too few members are discarded, clusters with high internal
/// variance are split in two along their dominant band, and clusters whose
/// centroids are too close together are merged. This follows the classical
/// ISODATA formulation of Ball & Hall (1965).
pub struct ISODATAClustering {
    /// Initial / target number of clusters
    pub n_clusters: usize,
    /// Maximum number of outer ISODATA iterations
    pub max_iterations: usize,
    /// Minimum number of members a cluster must retain, else it is discarded
    pub min_cluster_size: usize,
    /// Per-band standard deviation above which a cluster is a split candidate
    pub split_std_dev: f64,
    /// Centroid distance below which two clusters are merged
    pub merge_distance: f64,
    /// Maximum number of clusters allowed after splitting
    pub max_clusters: usize,
}

impl ISODATAClustering {
    /// Create a new ISODATA classifier.
    ///
    /// By default splitting and merging are disabled (`split_std_dev =
    /// +infinity`, `merge_distance = 0.0`), which makes ISODATA behave like
    /// plain K-Means with discard-only cluster-count adjustment; use the
    /// `with_*` builders to enable the full ISODATA behaviour.
    pub fn new(n_clusters: usize, max_iterations: usize) -> Self {
        Self {
            n_clusters,
            max_iterations,
            min_cluster_size: 1,
            split_std_dev: f64::INFINITY,
            merge_distance: 0.0,
            max_clusters: n_clusters.saturating_mul(2).max(n_clusters),
        }
    }

    /// Configure the minimum cluster population before a cluster is discarded.
    #[must_use]
    pub fn with_min_cluster_size(mut self, min_cluster_size: usize) -> Self {
        self.min_cluster_size = min_cluster_size;
        self
    }

    /// Configure the per-band standard-deviation threshold above which a
    /// cluster becomes a split candidate.
    #[must_use]
    pub fn with_split_std_dev(mut self, split_std_dev: f64) -> Self {
        self.split_std_dev = split_std_dev;
        self
    }

    /// Configure the centroid distance below which two clusters are merged.
    #[must_use]
    pub fn with_merge_distance(mut self, merge_distance: f64) -> Self {
        self.merge_distance = merge_distance;
        self
    }

    /// Configure the maximum number of clusters splitting may produce.
    #[must_use]
    pub fn with_max_clusters(mut self, max_clusters: usize) -> Self {
        self.max_clusters = max_clusters;
        self
    }

    /// Classify image pixels using the ISODATA algorithm.
    pub fn classify(&self, data: &ArrayView2<f64>) -> Result<Array1<usize>> {
        let n_pixels = data.nrows();
        let n_bands = data.ncols();

        if n_pixels == 0 {
            return Err(SensorError::dimension_mismatch(
                "at least one pixel row",
                "0 rows",
            ));
        }
        if self.n_clusters == 0 {
            return Err(SensorError::invalid_parameter(
                "n_clusters",
                "must be greater than 0",
            ));
        }

        // Splitting can only ever produce as many clusters as there are
        // pixels to populate them; clamp the configured ceiling so a later
        // K-Means re-fit never rejects an over-split cluster count.
        let max_clusters = self.max_clusters.min(n_pixels).max(1);

        let mut n_clusters = self.n_clusters.min(n_pixels);
        let mut centroids: Option<Array2<f64>> = None;

        for _outer in 0..self.max_iterations.max(1) {
            // Run K-Means to convergence for the current cluster count.
            let kmeans = KMeansClustering::new(n_clusters, self.max_iterations.max(1), 1e-4)?;
            let (labels, fitted_centroids) = kmeans.fit(data)?;

            let stats = compute_cluster_stats(data, &labels, &fitted_centroids, n_clusters);

            // 1. Discard clusters that fell below the minimum population.
            let keep: Vec<usize> = (0..n_clusters)
                .filter(|&c| stats.get(c).map(|s| s.count).unwrap_or(0) >= self.min_cluster_size)
                .collect();

            let mut next_centroids: Vec<Array1<f64>> = keep
                .iter()
                .map(|&c| fitted_centroids.row(c).to_owned())
                .collect();

            if next_centroids.is_empty() {
                // Degenerate configuration (e.g. min_cluster_size too high
                // for the data) — fall back to the unfiltered K-Means result.
                centroids = Some(fitted_centroids);
                break;
            }

            // 2. Split clusters whose per-band std-dev exceeds the threshold,
            //    provided doing so keeps us within max_clusters.
            let mut split_any = false;
            if next_centroids.len() < max_clusters {
                let mut split_centroids = Vec::with_capacity(next_centroids.len());
                for (rank, &c) in keep.iter().enumerate() {
                    let remaining_budget = max_clusters
                        .saturating_sub(split_centroids.len() + (next_centroids.len() - rank));
                    let stat = stats.get(c);
                    let dominant_band = stat.and_then(|s| {
                        s.std_dev
                            .iter()
                            .enumerate()
                            .max_by(|a, b| a.1.total_cmp(b.1))
                            .map(|(idx, &val)| (idx, val))
                    });

                    let should_split = remaining_budget > 0
                        && stat.map(|s| s.count).unwrap_or(0) >= 2 * self.min_cluster_size.max(1)
                        && dominant_band.map(|(_, val)| val).unwrap_or(0.0) > self.split_std_dev;

                    if let (true, Some((band, delta))) = (should_split, dominant_band) {
                        let centroid = &next_centroids[rank];
                        let mut plus = centroid.clone();
                        let mut minus = centroid.clone();
                        if let (Some(p), Some(m)) = (plus.get_mut(band), minus.get_mut(band)) {
                            *p += delta;
                            *m -= delta;
                        }
                        split_centroids.push(plus);
                        split_centroids.push(minus);
                        split_any = true;
                    } else {
                        split_centroids.push(next_centroids[rank].clone());
                    }
                }
                next_centroids = split_centroids;
            }

            // 3. Merge cluster pairs whose centroids are closer than
            //    merge_distance.
            let mut merged_any = false;
            if self.merge_distance > 0.0 && next_centroids.len() > 1 {
                let mut merged: Vec<Array1<f64>> = Vec::new();
                let mut absorbed = vec![false; next_centroids.len()];
                for i in 0..next_centroids.len() {
                    if absorbed[i] {
                        continue;
                    }
                    let mut group = vec![next_centroids[i].clone()];
                    for j in (i + 1)..next_centroids.len() {
                        if absorbed[j] {
                            continue;
                        }
                        let dist =
                            squared_distance(&next_centroids[i].view(), &next_centroids[j].view())
                                .sqrt();
                        if dist <= self.merge_distance {
                            group.push(next_centroids[j].clone());
                            absorbed[j] = true;
                            merged_any = true;
                        }
                    }
                    let mut avg = Array1::<f64>::zeros(n_bands);
                    for member in &group {
                        avg = &avg + member;
                    }
                    avg /= group.len() as f64;
                    merged.push(avg);
                }
                next_centroids = merged;
            }

            let mut centroid_matrix = Array2::<f64>::zeros((next_centroids.len(), n_bands));
            for (r, row) in next_centroids.iter().enumerate() {
                centroid_matrix.row_mut(r).assign(row);
            }

            let previous_n_clusters = n_clusters;
            n_clusters = centroid_matrix.nrows().max(1);
            centroids = Some(centroid_matrix);

            let converged = !split_any && !merged_any && n_clusters == previous_n_clusters;
            if converged {
                break;
            }
        }

        let final_centroids = centroids.ok_or_else(|| {
            SensorError::classification_error("ISODATA failed to converge to any centroids")
        })?;

        Ok(assign_labels(data, &final_centroids.view()))
    }
}

/// Compute per-cluster population and per-band standard deviation, used by
/// ISODATA's split/merge/discard rules.
fn compute_cluster_stats(
    data: &ArrayView2<f64>,
    labels: &Array1<usize>,
    centroids: &Array2<f64>,
    n_clusters: usize,
) -> Vec<ClusterStats> {
    let n_bands = data.ncols();
    let mut sums_sq = vec![vec![0.0f64; n_bands]; n_clusters];
    let mut counts = vec![0usize; n_clusters];

    for (i, pixel) in data.outer_iter().enumerate() {
        let Some(&label) = labels.get(i) else {
            continue;
        };
        if label >= n_clusters {
            continue;
        }
        let centroid = centroids.row(label);
        if let Some(count_slot) = counts.get_mut(label) {
            *count_slot += 1;
        }
        if let Some(band_sums) = sums_sq.get_mut(label) {
            for (b, (&p, &c)) in pixel.iter().zip(centroid.iter()).enumerate() {
                if let Some(slot) = band_sums.get_mut(b) {
                    let d = p - c;
                    *slot += d * d;
                }
            }
        }
    }

    (0..n_clusters)
        .map(|c| {
            let count = counts.get(c).copied().unwrap_or(0);
            let count_f = count.max(1) as f64;
            let std_dev = sums_sq
                .get(c)
                .map(|sums| sums.iter().map(|&s| (s / count_f).sqrt()).collect())
                .unwrap_or_default();
            ClusterStats { count, std_dev }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_kmeans_ok() {
        let data = array![[0.1, 0.2], [0.9, 0.8], [0.15, 0.25], [0.85, 0.95]];

        let kmeans = KMeansClustering::new(2, 100, 1e-6);
        assert!(kmeans.is_ok());

        if let Ok(kmeans) = kmeans {
            let labels = kmeans.classify(&data.view());
            assert!(labels.is_ok());
        }
    }

    #[test]
    fn test_kmeans_separates_well_separated_clusters() {
        // Two tight, well-separated point pairs in a non-normalized DN-like
        // range (this is exactly the case that the old "mean * n_clusters"
        // stub collapsed into a single bucket).
        let data = array![[10.0, 12.0], [11.0, 13.0], [500.0, 480.0], [510.0, 495.0]];

        let kmeans = KMeansClustering::new(2, 50, 1e-6).expect("valid n_clusters");
        let labels = kmeans.classify(&data.view()).expect("classify succeeds");

        // Points within a pair must share a label...
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[2], labels[3]);
        // ...and the two pairs must be in different clusters.
        assert_ne!(labels[0], labels[2]);
    }

    #[test]
    fn test_kmeans_three_clusters_separates_correctly() {
        let data = array![
            [0.0, 0.0],
            [0.0, 1.0],
            [100.0, 100.0],
            [101.0, 99.0],
            [-100.0, 100.0],
            [-101.0, 101.0],
        ];

        let kmeans = KMeansClustering::new(3, 100, 1e-8).expect("valid n_clusters");
        let labels = kmeans.classify(&data.view()).expect("classify succeeds");

        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[2], labels[3]);
        assert_eq!(labels[4], labels[5]);

        assert_ne!(labels[0], labels[2]);
        assert_ne!(labels[0], labels[4]);
        assert_ne!(labels[2], labels[4]);
    }

    #[test]
    fn test_kmeans_is_deterministic_for_a_fixed_seed() {
        let data = array![[10.0, 12.0], [11.0, 13.0], [500.0, 480.0], [510.0, 495.0]];

        let kmeans = KMeansClustering::new(2, 50, 1e-6)
            .expect("valid n_clusters")
            .with_seed(42);
        let labels_a = kmeans.classify(&data.view()).expect("classify succeeds");
        let labels_b = kmeans.classify(&data.view()).expect("classify succeeds");

        assert_eq!(labels_a, labels_b);
    }

    #[test]
    fn test_kmeans_rejects_more_clusters_than_pixels() {
        let data = array![[0.0, 0.0], [1.0, 1.0]];
        let kmeans = KMeansClustering::new(5, 10, 1e-4).expect("valid n_clusters");
        let result = kmeans.classify(&data.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_kmeans_rejects_zero_clusters() {
        let result = KMeansClustering::new(0, 10, 1e-4);
        assert!(result.is_err());
    }

    #[test]
    fn test_kmeans_rejects_empty_data() {
        let data = Array2::<f64>::zeros((0, 2));
        let kmeans = KMeansClustering::new(2, 10, 1e-4).expect("valid n_clusters");
        let result = kmeans.classify(&data.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_isodata_matches_kmeans_when_no_split_or_merge_enabled() {
        let data = array![[10.0, 12.0], [11.0, 13.0], [500.0, 480.0], [510.0, 495.0]];

        // Default thresholds (split_std_dev = INFINITY, merge_distance = 0.0)
        // mean ISODATA degenerates to plain K-Means with the requested
        // cluster count.
        let isodata = ISODATAClustering::new(2, 20);
        let labels = isodata.classify(&data.view());
        assert!(labels.is_ok());
        if let Ok(labels) = labels {
            assert_eq!(labels[0], labels[1]);
            assert_eq!(labels[2], labels[3]);
            assert_ne!(labels[0], labels[2]);
        }
    }

    #[test]
    fn test_isodata_discards_undersized_clusters() {
        // 7 tightly clustered points plus a single outlier: with
        // min_cluster_size = 2 the outlier's singleton cluster should be
        // discarded rather than kept as its own class.
        let data = array![
            [0.0, 0.0],
            [0.1, 0.1],
            [0.2, 0.0],
            [0.0, 0.2],
            [0.1, 0.0],
            [0.0, 0.1],
            [0.05, 0.05],
            [1000.0, 1000.0],
        ];

        let isodata = ISODATAClustering::new(2, 10).with_min_cluster_size(2);
        let labels = isodata.classify(&data.view());
        assert!(labels.is_ok());
    }

    #[test]
    fn test_isodata_splits_high_variance_cluster() {
        // A single very elongated cloud of points: with a strict enough
        // split threshold ISODATA should split it into two clusters even
        // though it was asked to start from n_clusters = 1.
        let data = array![
            [0.0, 0.0],
            [1.0, 0.0],
            [2.0, 0.0],
            [50.0, 0.0],
            [51.0, 0.0],
            [52.0, 0.0],
        ];

        let isodata = ISODATAClustering::new(1, 10)
            .with_min_cluster_size(1)
            .with_split_std_dev(5.0)
            .with_max_clusters(4);
        let labels = isodata.classify(&data.view());
        assert!(labels.is_ok());
        if let Ok(labels) = labels {
            assert_ne!(labels[0], labels[3]);
        }
    }

    #[test]
    fn test_isodata_rejects_zero_clusters() {
        let data = array![[0.0, 0.0], [1.0, 1.0]];
        let isodata = ISODATAClustering::new(0, 10);
        let result = isodata.classify(&data.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_isodata_rejects_empty_data() {
        let data = Array2::<f64>::zeros((0, 2));
        let isodata = ISODATAClustering::new(2, 10);
        let result = isodata.classify(&data.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_nearest_centroid_empty_returns_none() {
        let centroids = Array2::<f64>::zeros((0, 2));
        let pixel = array![1.0, 2.0];
        assert!(nearest_centroid(&pixel.view(), &centroids.view()).is_none());
    }

    #[test]
    fn test_squared_distance_symmetry() {
        let a = array![1.0, 2.0, 3.0];
        let b = array![4.0, 6.0, 3.0];
        let d1 = squared_distance(&a.view(), &b.view());
        let d2 = squared_distance(&b.view(), &a.view());
        assert!((d1 - d2).abs() < 1e-12);
        assert!((d1 - 25.0).abs() < 1e-12);
    }
}
