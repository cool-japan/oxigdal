//! Arrow Flight client implementation for distributed data transfer.
//!
//! This module implements an Arrow Flight client for fetching and sending
//! geospatial data using zero-copy transfers.

use crate::error::{DistributedError, Result};
use arrow::record_batch::RecordBatch;
use arrow_flight::{Action, HandshakeRequest, Ticket, flight_service_client::FlightServiceClient};
use bytes::Bytes;
use futures::StreamExt;
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, warn};

/// Flight client for fetching geospatial data.
pub struct FlightClient {
    /// gRPC client.
    client: FlightServiceClient<Channel>,
    /// Server address.
    address: String,
}

impl FlightClient {
    /// Create a new Flight client.
    pub async fn new(address: String) -> Result<Self> {
        info!("Connecting to Flight server at {}", address);

        let endpoint = Endpoint::from_shared(address.clone())
            .map_err(|e| DistributedError::worker_connection(format!("Invalid endpoint: {}", e)))?
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10));

        let channel = endpoint.connect().await.map_err(|e| {
            DistributedError::worker_connection(format!("Connection failed: {}", e))
        })?;

        let client = FlightServiceClient::new(channel);

        Ok(Self { client, address })
    }

    /// Perform handshake with the server.
    pub async fn handshake(&mut self) -> Result<()> {
        debug!("Performing handshake with {}", self.address);

        let request = tonic::Request::new(futures::stream::once(async {
            HandshakeRequest {
                protocol_version: 0,
                payload: Bytes::new(),
            }
        }));

        let mut response_stream = self
            .client
            .handshake(request)
            .await
            .map_err(|e| DistributedError::flight_rpc(format!("Handshake failed: {}", e)))?
            .into_inner();

        // Read handshake response
        while let Some(response) = response_stream.next().await {
            let _handshake_response = response
                .map_err(|e| DistributedError::flight_rpc(format!("Handshake error: {}", e)))?;
            debug!("Handshake successful");
        }

        Ok(())
    }

    /// Fetch data from the server using a ticket.
    pub async fn get_data(&mut self, ticket: String) -> Result<Vec<RecordBatch>> {
        info!("Fetching data for ticket: {}", ticket);

        let ticket = Ticket {
            ticket: Bytes::from(ticket),
        };

        let request = tonic::Request::new(ticket);

        let mut stream = self
            .client
            .do_get(request)
            .await
            .map_err(|e| DistributedError::flight_rpc(format!("DoGet failed: {}", e)))?
            .into_inner();

        let mut flight_data_vec = Vec::new();

        while let Some(data_result) = stream.next().await {
            flight_data_vec.push(
                data_result
                    .map_err(|e| DistributedError::flight_rpc(format!("Stream error: {}", e)))?,
            );
        }

        // Convert FlightData to RecordBatches
        let batches = arrow_flight::utils::flight_data_to_batches(&flight_data_vec)
            .map_err(|e| DistributedError::arrow(format!("Failed to decode batches: {}", e)))?;

        info!("Received {} batches", batches.len());
        Ok(batches)
    }

    /// Send data to the server.
    pub async fn put_data(&mut self, batches: Vec<RecordBatch>) -> Result<()> {
        info!("Sending {} batches to server", batches.len());

        if batches.is_empty() {
            return Err(DistributedError::flight_rpc("No batches to send"));
        }

        // Convert batches to FlightData
        let flight_data_vec =
            arrow_flight::utils::batches_to_flight_data(batches[0].schema().as_ref(), batches)
                .map_err(|e| DistributedError::arrow(format!("Failed to encode batches: {}", e)))?;

        let request = tonic::Request::new(futures::stream::iter(flight_data_vec));

        let mut response_stream = self
            .client
            .do_put(request)
            .await
            .map_err(|e| DistributedError::flight_rpc(format!("DoPut failed: {}", e)))?
            .into_inner();

        // Read put results
        while let Some(result) = response_stream.next().await {
            let _put_result =
                result.map_err(|e| DistributedError::flight_rpc(format!("Put error: {}", e)))?;
        }

        info!("Data sent successfully");
        Ok(())
    }

    /// Execute an action on the server.
    pub async fn do_action(&mut self, action_type: String, body: Bytes) -> Result<Vec<Bytes>> {
        debug!("Executing action: {}", action_type);

        let action = Action {
            r#type: action_type.clone(),
            body,
        };

        let request = tonic::Request::new(action);

        let mut stream = self
            .client
            .do_action(request)
            .await
            .map_err(|e| DistributedError::flight_rpc(format!("DoAction failed: {}", e)))?
            .into_inner();

        let mut results = Vec::new();

        while let Some(result) = stream.next().await {
            let action_result =
                result.map_err(|e| DistributedError::flight_rpc(format!("Action error: {}", e)))?;
            results.push(action_result.body);
        }

        debug!(
            "Action {} completed with {} results",
            action_type,
            results.len()
        );
        Ok(results)
    }

    /// List all available tickets.
    pub async fn list_tickets(&mut self) -> Result<Vec<String>> {
        let results = self
            .do_action("list_tickets".to_string(), Bytes::new())
            .await?;

        if results.is_empty() {
            return Ok(Vec::new());
        }

        let tickets: Vec<String> = serde_json::from_slice(&results[0]).map_err(|e| {
            DistributedError::flight_rpc(format!("Failed to deserialize tickets: {}", e))
        })?;

        Ok(tickets)
    }

    /// Remove a ticket from the server.
    pub async fn remove_ticket(&mut self, ticket: String) -> Result<()> {
        let body = Bytes::from(ticket.clone());
        let _results = self.do_action("remove_ticket".to_string(), body).await?;

        info!("Removed ticket: {}", ticket);
        Ok(())
    }

    /// Get the server address.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Check if the client is connected.
    pub async fn health_check(&mut self) -> Result<bool> {
        match self.handshake().await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Health check failed: {}", e);
                Ok(false)
            }
        }
    }
}

/// Connection pool for managing multiple Flight clients.
pub struct FlightClientPool {
    /// Available clients.
    clients: Vec<FlightClient>,
    /// Maximum pool size.
    max_size: usize,
}

impl FlightClientPool {
    /// Create a new client pool.
    pub fn new(max_size: usize) -> Self {
        Self {
            clients: Vec::new(),
            max_size,
        }
    }

    /// Add a client to the pool.
    pub async fn add_client(&mut self, address: String) -> Result<()> {
        if self.clients.len() >= self.max_size {
            return Err(DistributedError::worker_connection(
                "Pool is at maximum capacity",
            ));
        }

        let client = FlightClient::new(address).await?;
        self.clients.push(client);
        Ok(())
    }

    /// Get a client from the pool (round-robin).
    pub fn get_client(&mut self) -> Result<&mut FlightClient> {
        if self.clients.is_empty() {
            return Err(DistributedError::worker_connection("No clients available"));
        }

        // Simple round-robin: rotate the first client to the back
        self.clients.rotate_left(1);
        let idx = self.clients.len() - 1;
        Ok(&mut self.clients[idx])
    }

    /// Get the number of clients in the pool.
    pub fn size(&self) -> usize {
        self.clients.len()
    }

    /// Check health of all clients.
    pub async fn health_check_all(&mut self) -> Result<Vec<bool>> {
        let mut results = Vec::new();

        for client in &mut self.clients {
            let is_healthy = client.health_check().await.unwrap_or(false);
            results.push(is_healthy);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_pool() {
        let pool = FlightClientPool::new(5);
        assert_eq!(pool.size(), 0);
        assert_eq!(pool.max_size, 5);
    }

    #[tokio::test]
    async fn test_client_creation_fails_for_invalid_address() {
        let result = FlightClient::new("invalid://address".to_string()).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_pool_get_client_empty() {
        let mut pool = FlightClientPool::new(5);
        let result = pool.get_client();
        assert!(result.is_err());
    }
}
