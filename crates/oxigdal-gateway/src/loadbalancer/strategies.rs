//! Load balancing strategies.

use super::Backend;
use crate::error::{GatewayError, Result};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Load balancing strategy type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalancingStrategy {
    /// Round-robin selection
    RoundRobin,
    /// Least connections
    LeastConnections,
    /// Weighted round-robin
    Weighted,
    /// IP hash
    IpHash,
}

/// Load balancing strategy trait.
pub trait LoadBalancingStrategy: Send + Sync {
    /// Selects a backend from the available backends.
    fn select(&self, backends: &[&Backend], client_ip: Option<&str>) -> Result<Backend>;
}

/// Round-robin load balancing strategy.
pub struct RoundRobinStrategy {
    counter: AtomicUsize,
}

impl RoundRobinStrategy {
    /// Creates a new round-robin strategy.
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }
}

impl Default for RoundRobinStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBalancingStrategy for RoundRobinStrategy {
    fn select(&self, backends: &[&Backend], _client_ip: Option<&str>) -> Result<Backend> {
        if backends.is_empty() {
            return Err(GatewayError::LoadBalancerError(
                "No backends available".to_string(),
            ));
        }

        let index = self.counter.fetch_add(1, Ordering::Relaxed) % backends.len();
        Ok(backends[index].clone())
    }
}

/// Least connections load balancing strategy.
pub struct LeastConnectionsStrategy {
    connections: Arc<dashmap::DashMap<String, AtomicUsize>>,
}

use std::sync::Arc;

impl LeastConnectionsStrategy {
    /// Creates a new least connections strategy.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Gets connection count for a backend.
    pub fn get_connections(&self, backend_id: &str) -> usize {
        self.connections
            .get(backend_id)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Increments connection count for a backend.
    pub fn increment(&self, backend_id: &str) {
        self.connections
            .entry(backend_id.to_string())
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Decrements connection count for a backend.
    pub fn decrement(&self, backend_id: &str) {
        if let Some(counter) = self.connections.get(backend_id) {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

impl Default for LeastConnectionsStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBalancingStrategy for LeastConnectionsStrategy {
    fn select(&self, backends: &[&Backend], _client_ip: Option<&str>) -> Result<Backend> {
        if backends.is_empty() {
            return Err(GatewayError::LoadBalancerError(
                "No backends available".to_string(),
            ));
        }

        let selected = backends
            .iter()
            .min_by_key(|b| self.get_connections(&b.id))
            .ok_or_else(|| {
                GatewayError::LoadBalancerError("Failed to select backend".to_string())
            })?;

        Ok((*selected).clone())
    }
}

/// Weighted load balancing strategy.
pub struct WeightedStrategy {
    counter: AtomicUsize,
}

impl WeightedStrategy {
    /// Creates a new weighted strategy.
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }
}

impl Default for WeightedStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBalancingStrategy for WeightedStrategy {
    fn select(&self, backends: &[&Backend], _client_ip: Option<&str>) -> Result<Backend> {
        if backends.is_empty() {
            return Err(GatewayError::LoadBalancerError(
                "No backends available".to_string(),
            ));
        }

        let total_weight: u32 = backends.iter().map(|b| b.weight).sum();
        let mut target = (self.counter.fetch_add(1, Ordering::Relaxed) as u32) % total_weight;

        for backend in backends {
            if target < backend.weight {
                return Ok((*backend).clone());
            }
            target -= backend.weight;
        }

        Ok(backends[0].clone())
    }
}

/// IP hash load balancing strategy.
pub struct IpHashStrategy;

impl IpHashStrategy {
    /// Creates a new IP hash strategy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for IpHashStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBalancingStrategy for IpHashStrategy {
    fn select(&self, backends: &[&Backend], client_ip: Option<&str>) -> Result<Backend> {
        if backends.is_empty() {
            return Err(GatewayError::LoadBalancerError(
                "No backends available".to_string(),
            ));
        }

        let hash = if let Some(ip) = client_ip {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            ip.hash(&mut hasher);
            hasher.finish() as usize
        } else {
            0
        };

        let index = hash % backends.len();
        Ok(backends[index].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_backends() -> Vec<Backend> {
        vec![
            Backend::new("b1".to_string(), "http://localhost:8001".to_string(), 1),
            Backend::new("b2".to_string(), "http://localhost:8002".to_string(), 2),
            Backend::new("b3".to_string(), "http://localhost:8003".to_string(), 1),
        ]
    }

    #[test]
    fn test_round_robin() {
        let strategy = RoundRobinStrategy::new();
        let backends = create_test_backends();
        let backend_refs: Vec<&Backend> = backends.iter().collect();

        let b1 = strategy.select(&backend_refs, None);
        assert!(b1.is_ok());

        let b2 = strategy.select(&backend_refs, None);
        assert!(b2.is_ok());

        assert_ne!(b1.ok().map(|b| b.id), b2.ok().map(|b| b.id));
    }

    #[test]
    fn test_least_connections() {
        let strategy = LeastConnectionsStrategy::new();
        let backends = create_test_backends();
        let backend_refs: Vec<&Backend> = backends.iter().collect();

        let backend = strategy.select(&backend_refs, None);
        assert!(backend.is_ok());
    }

    #[test]
    fn test_weighted() {
        let strategy = WeightedStrategy::new();
        let backends = create_test_backends();
        let backend_refs: Vec<&Backend> = backends.iter().collect();

        let backend = strategy.select(&backend_refs, None);
        assert!(backend.is_ok());
    }

    #[test]
    fn test_ip_hash() {
        let strategy = IpHashStrategy::new();
        let backends = create_test_backends();
        let backend_refs: Vec<&Backend> = backends.iter().collect();

        let b1 = strategy.select(&backend_refs, Some("192.168.1.1"));
        let b2 = strategy.select(&backend_refs, Some("192.168.1.1"));

        assert!(b1.is_ok());
        assert!(b2.is_ok());

        // Same IP should always get same backend
        assert_eq!(b1.ok().map(|b| b.id), b2.ok().map(|b| b.id));
    }
}
