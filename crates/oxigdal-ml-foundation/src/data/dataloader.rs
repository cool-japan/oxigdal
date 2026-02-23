//! Data loader for batching and parallel data loading.
//!
//! Provides efficient batch iteration with optional shuffling and prefetching.

use crate::data::Dataset;
use crate::{Error, Result};
use rayon::prelude::*;
use std::sync::Arc;

/// Data loader for efficient batch iteration.
///
/// The data loader manages batch creation, shuffling, and parallel loading
/// of samples from a dataset.
pub struct DataLoader<D> {
    /// Reference to the dataset
    dataset: Arc<D>,
    /// Batch size
    batch_size: usize,
    /// Whether to shuffle indices
    shuffle: bool,
    /// Number of parallel workers for batch loading
    num_workers: usize,
}

impl<D: Dataset + 'static> DataLoader<D> {
    /// Creates a new data loader.
    ///
    /// # Arguments
    ///
    /// * `dataset` - The dataset to load from
    /// * `batch_size` - Number of samples per batch
    /// * `shuffle` - Whether to shuffle the data
    ///
    /// # Errors
    ///
    /// Returns an error if batch size is 0 or dataset is empty.
    pub fn new(dataset: Arc<D>, batch_size: usize, shuffle: bool) -> Result<Self> {
        if batch_size == 0 {
            return Err(Error::invalid_parameter("batch_size", 0, "must be > 0"));
        }

        if dataset.is_empty() {
            return Err(Error::invalid_parameter(
                "dataset",
                "empty",
                "dataset must contain at least one sample",
            ));
        }

        Ok(Self {
            dataset,
            batch_size,
            shuffle,
            num_workers: rayon::current_num_threads(),
        })
    }

    /// Sets the number of parallel workers.
    pub fn with_num_workers(mut self, num_workers: usize) -> Result<Self> {
        if num_workers == 0 {
            return Err(Error::invalid_parameter("num_workers", 0, "must be > 0"));
        }
        self.num_workers = num_workers;
        Ok(self)
    }

    /// Returns the number of batches in an epoch.
    pub fn num_batches(&self) -> usize {
        self.dataset.len().div_ceil(self.batch_size)
    }

    /// Creates an iterator over batches.
    pub fn iter(&self) -> BatchIter<D> {
        let mut indices: Vec<usize> = (0..self.dataset.len()).collect();

        if self.shuffle {
            shuffle_indices(&mut indices);
        }

        BatchIter {
            dataset: Arc::clone(&self.dataset),
            indices,
            batch_size: self.batch_size,
            current_idx: 0,
        }
    }

    /// Returns a reference to the dataset.
    pub fn dataset(&self) -> &D {
        &self.dataset
    }

    /// Returns the batch size.
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }
}

/// Iterator over batches from a dataset.
pub struct BatchIter<D> {
    dataset: Arc<D>,
    indices: Vec<usize>,
    batch_size: usize,
    current_idx: usize,
}

impl<D: Dataset> Iterator for BatchIter<D> {
    type Item = Result<(Vec<f32>, Vec<f32>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx >= self.indices.len() {
            return None;
        }

        let end_idx = (self.current_idx + self.batch_size).min(self.indices.len());
        let batch_indices = &self.indices[self.current_idx..end_idx];

        let result = self.dataset.get_batch(batch_indices);
        self.current_idx = end_idx;

        Some(result)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.indices.len() - self.current_idx).div_ceil(self.batch_size);
        (remaining, Some(remaining))
    }
}

impl<D: Dataset> ExactSizeIterator for BatchIter<D> {
    fn len(&self) -> usize {
        (self.indices.len() - self.current_idx).div_ceil(self.batch_size)
    }
}

/// Shuffles indices using Fisher-Yates algorithm.
fn shuffle_indices(indices: &mut [usize]) {
    for i in (1..indices.len()).rev() {
        let j = match get_random_usize(i + 1) {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("Failed to generate random index for shuffle, skipping");
                continue;
            }
        };
        indices.swap(i, j);
    }
}

/// Helper to get a random usize in range [0, max).
fn get_random_usize(max: usize) -> Result<usize> {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf)
        .map_err(|e| Error::Numerical(format!("Failed to generate random number: {}", e)))?;
    let value = u64::from_ne_bytes(buf);
    Ok((value % max as u64) as usize)
}

/// Parallel data loader for concurrent batch loading.
///
/// This loader uses rayon to load batches in parallel, which can
/// significantly speed up I/O-bound operations.
pub struct ParallelDataLoader<D> {
    /// Reference to the dataset
    dataset: Arc<D>,
    /// Batch size
    batch_size: usize,
    /// Whether to shuffle indices
    shuffle: bool,
    /// Prefetch factor (number of batches to prefetch)
    prefetch: usize,
}

impl<D: Dataset + Send + Sync + 'static> ParallelDataLoader<D> {
    /// Creates a new parallel data loader.
    pub fn new(dataset: Arc<D>, batch_size: usize, shuffle: bool) -> Result<Self> {
        if batch_size == 0 {
            return Err(Error::invalid_parameter("batch_size", 0, "must be > 0"));
        }

        Ok(Self {
            dataset,
            batch_size,
            shuffle,
            prefetch: 2, // Default: prefetch 2 batches
        })
    }

    /// Sets the prefetch factor.
    pub fn with_prefetch(mut self, prefetch: usize) -> Self {
        self.prefetch = prefetch;
        self
    }

    /// Loads all batches in parallel and returns them as a vector.
    ///
    /// This is useful for small datasets that fit in memory.
    pub fn load_all(&self) -> Result<Vec<(Vec<f32>, Vec<f32>)>> {
        let mut indices: Vec<usize> = (0..self.dataset.len()).collect();

        if self.shuffle {
            shuffle_indices(&mut indices);
        }

        // Create batch indices
        let batch_indices: Vec<Vec<usize>> = indices
            .chunks(self.batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        // Load batches in parallel
        let results: Vec<Result<(Vec<f32>, Vec<f32>)>> = batch_indices
            .par_iter()
            .map(|batch_idx| self.dataset.get_batch(batch_idx))
            .collect();

        // Collect results, propagating errors
        let mut batches = Vec::with_capacity(results.len());
        for result in results {
            batches.push(result?);
        }

        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Mock dataset for testing
    struct MockDataset {
        size: usize,
        input_dim: usize,
        output_dim: usize,
    }

    impl Dataset for MockDataset {
        fn len(&self) -> usize {
            self.size
        }

        fn get_batch(&self, indices: &[usize]) -> Result<(Vec<f32>, Vec<f32>)> {
            let inputs: Vec<f32> = indices
                .iter()
                .flat_map(|&i| vec![i as f32; self.input_dim])
                .collect();

            let targets: Vec<f32> = indices
                .iter()
                .flat_map(|&i| vec![(i * 2) as f32; self.output_dim])
                .collect();

            Ok((inputs, targets))
        }

        fn shapes(&self) -> (Vec<usize>, Vec<usize>) {
            (vec![1, self.input_dim], vec![1, self.output_dim])
        }
    }

    #[test]
    fn test_dataloader_creation() {
        let dataset = Arc::new(MockDataset {
            size: 100,
            input_dim: 10,
            output_dim: 5,
        });

        let loader = DataLoader::new(dataset, 16, false);
        assert!(loader.is_ok());

        let loader = loader.expect("Failed to create loader");
        assert_eq!(loader.batch_size(), 16);
        assert_eq!(loader.num_batches(), 7); // ceil(100/16) = 7
    }

    #[test]
    fn test_dataloader_iteration() {
        let dataset = Arc::new(MockDataset {
            size: 50,
            input_dim: 4,
            output_dim: 2,
        });

        let loader = DataLoader::new(dataset, 10, false).expect("Failed to create loader");
        let mut count = 0;

        for batch in loader.iter() {
            assert!(batch.is_ok());
            let (inputs, targets) = batch.expect("Failed to get batch");
            assert!(!inputs.is_empty());
            assert!(!targets.is_empty());
            count += 1;
        }

        assert_eq!(count, 5); // 50/10 = 5 batches
    }

    #[test]
    fn test_dataloader_validation() {
        let dataset = Arc::new(MockDataset {
            size: 100,
            input_dim: 10,
            output_dim: 5,
        });

        // Zero batch size
        let result = DataLoader::new(dataset.clone(), 0, false);
        assert!(result.is_err());

        // Empty dataset
        let empty_dataset = Arc::new(MockDataset {
            size: 0,
            input_dim: 10,
            output_dim: 5,
        });
        let result = DataLoader::new(empty_dataset, 16, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_parallel_loader() {
        let dataset = Arc::new(MockDataset {
            size: 100,
            input_dim: 8,
            output_dim: 4,
        });

        let loader = ParallelDataLoader::new(dataset, 10, false);
        assert!(loader.is_ok());

        let loader = loader.expect("Failed to create parallel loader");
        let batches = loader.load_all();
        assert!(batches.is_ok());

        let batches = batches.expect("Failed to load batches");
        assert_eq!(batches.len(), 10); // 100/10 = 10 batches
    }

    #[test]
    fn test_shuffle() {
        let mut indices: Vec<usize> = (0..100).collect();
        let original = indices.clone();

        shuffle_indices(&mut indices);

        // Should be different after shuffling (with very high probability)
        assert_ne!(indices, original);

        // Should contain same elements
        let mut sorted_indices = indices.clone();
        sorted_indices.sort_unstable();
        assert_eq!(sorted_indices, original);
    }
}
