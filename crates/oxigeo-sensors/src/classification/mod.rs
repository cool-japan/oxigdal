//! Image classification algorithms

mod gaussian;
pub mod supervised;
pub mod unsupervised;

pub use supervised::MaximumLikelihood;
pub use unsupervised::{ISODATAClustering, KMeansClustering};
