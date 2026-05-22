//! Arrow Flight server implementation for distributed data transfer.
//!
//! This module implements an Arrow Flight server that streams geospatial data
//! between nodes using zero-copy transfers.

use crate::error::{DistributedError, Result};
use arrow::record_batch::RecordBatch;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightEndpoint, FlightInfo,
    HandshakeRequest, HandshakeResponse, PutResult, SchemaAsIpc, SchemaResult, Ticket,
    flight_service_server::{FlightService, FlightServiceServer},
};
use arrow_ipc::writer::IpcWriteOptions;
use bytes::Bytes;
use futures::{Stream, StreamExt, stream};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tonic::{Request, Response, Streaming};
use tracing::{debug, info};

/// Flight server for serving geospatial data.
pub struct FlightServer {
    /// Stored data partitions (ticket -> RecordBatch).
    data_store: Arc<RwLock<HashMap<String, Arc<RecordBatch>>>>,
    /// Authentication tokens.
    auth_tokens: Arc<RwLock<HashMap<String, String>>>,
    /// Enable authentication.
    enable_auth: bool,
}

impl FlightServer {
    /// Create a new Flight server.
    pub fn new() -> Self {
        Self {
            data_store: Arc::new(RwLock::new(HashMap::new())),
            auth_tokens: Arc::new(RwLock::new(HashMap::new())),
            enable_auth: false,
        }
    }

    /// Enable authentication.
    pub fn with_auth(mut self) -> Self {
        self.enable_auth = true;
        self
    }

    /// Store data with a ticket.
    pub fn store_data(&self, ticket: String, data: Arc<RecordBatch>) -> Result<()> {
        let mut store = self
            .data_store
            .write()
            .map_err(|_| DistributedError::flight_rpc("Failed to acquire data store lock"))?;

        store.insert(ticket, data);
        Ok(())
    }

    /// Retrieve data by ticket.
    pub fn get_data(&self, ticket: &str) -> Result<Option<Arc<RecordBatch>>> {
        let store = self
            .data_store
            .read()
            .map_err(|_| DistributedError::flight_rpc("Failed to acquire data store lock"))?;

        Ok(store.get(ticket).cloned())
    }

    /// Remove data by ticket.
    pub fn remove_data(&self, ticket: &str) -> Result<Option<Arc<RecordBatch>>> {
        let mut store = self
            .data_store
            .write()
            .map_err(|_| DistributedError::flight_rpc("Failed to acquire data store lock"))?;

        Ok(store.remove(ticket))
    }

    /// List all available tickets.
    pub fn list_tickets(&self) -> Result<Vec<String>> {
        let store = self
            .data_store
            .read()
            .map_err(|_| DistributedError::flight_rpc("Failed to acquire data store lock"))?;

        Ok(store.keys().cloned().collect())
    }

    /// Add authentication token.
    pub fn add_auth_token(&self, token: String, user: String) -> Result<()> {
        let mut tokens = self
            .auth_tokens
            .write()
            .map_err(|_| DistributedError::authentication("Failed to acquire auth tokens lock"))?;

        tokens.insert(token, user);
        Ok(())
    }

    /// Convert to tonic service.
    pub fn into_service(self) -> FlightServiceServer<Self> {
        FlightServiceServer::new(self)
    }
}

impl Default for FlightServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl FlightService for FlightServer {
    type HandshakeStream =
        Pin<Box<dyn Stream<Item = std::result::Result<HandshakeResponse, tonic::Status>> + Send>>;
    type ListFlightsStream =
        Pin<Box<dyn Stream<Item = std::result::Result<FlightInfo, tonic::Status>> + Send>>;
    type DoGetStream =
        Pin<Box<dyn Stream<Item = std::result::Result<FlightData, tonic::Status>> + Send>>;
    type DoPutStream =
        Pin<Box<dyn Stream<Item = std::result::Result<PutResult, tonic::Status>> + Send>>;
    type DoActionStream = Pin<
        Box<dyn Stream<Item = std::result::Result<arrow_flight::Result, tonic::Status>> + Send>,
    >;
    type ListActionsStream =
        Pin<Box<dyn Stream<Item = std::result::Result<ActionType, tonic::Status>> + Send>>;
    type DoExchangeStream =
        Pin<Box<dyn Stream<Item = std::result::Result<FlightData, tonic::Status>> + Send>>;

    async fn handshake(
        &self,
        _request: Request<Streaming<HandshakeRequest>>,
    ) -> std::result::Result<Response<Self::HandshakeStream>, tonic::Status> {
        debug!("Handshake request received");

        // Simple handshake - just acknowledge
        let response = HandshakeResponse {
            protocol_version: 0,
            payload: Bytes::new(),
        };

        let stream = stream::once(async { Ok(response) });
        Ok(Response::new(Box::pin(stream)))
    }

    async fn list_flights(
        &self,
        _request: Request<Criteria>,
    ) -> std::result::Result<Response<Self::ListFlightsStream>, tonic::Status> {
        debug!("List flights request received");

        // Return empty stream - we don't support flight listing yet
        let stream = stream::empty();
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_flight_info(
        &self,
        request: Request<FlightDescriptor>,
    ) -> std::result::Result<Response<FlightInfo>, tonic::Status> {
        let descriptor = request.into_inner();
        debug!("Get flight info request: {:?}", descriptor);

        // Resolve ticket key from descriptor: prefer first path segment, fall back to cmd bytes.
        let ticket_key = if !descriptor.path.is_empty() {
            descriptor.path[0].clone()
        } else if !descriptor.cmd.is_empty() {
            String::from_utf8(descriptor.cmd.to_vec())
                .map_err(|e| tonic::Status::invalid_argument(format!("Invalid cmd: {}", e)))?
        } else {
            return Err(tonic::Status::invalid_argument(
                "FlightDescriptor must have a path or cmd",
            ));
        };

        let data = self
            .get_data(&ticket_key)
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::not_found(format!("Flight not found: {}", ticket_key)))?;

        let schema = data.schema();
        let ipc_opts = IpcWriteOptions::default();
        let schema_bytes: arrow_flight::IpcMessage = SchemaAsIpc::new(schema.as_ref(), &ipc_opts)
            .try_into()
            .map_err(|e: arrow_schema::ArrowError| {
                tonic::Status::internal(format!("Schema encode error: {}", e))
            })?;

        let endpoint = FlightEndpoint {
            ticket: Some(Ticket {
                ticket: Bytes::from(ticket_key),
            }),
            location: vec![],
            expiration_time: None,
            app_metadata: Bytes::new(),
        };

        let flight_info = FlightInfo {
            schema: schema_bytes.0,
            flight_descriptor: Some(descriptor),
            endpoint: vec![endpoint],
            total_records: data.num_rows() as i64,
            total_bytes: -1,
            ordered: false,
            app_metadata: Bytes::new(),
        };

        Ok(Response::new(flight_info))
    }

    async fn get_schema(
        &self,
        request: Request<FlightDescriptor>,
    ) -> std::result::Result<Response<SchemaResult>, tonic::Status> {
        let descriptor = request.into_inner();
        debug!("Get schema request received");

        let ticket_key = if !descriptor.path.is_empty() {
            descriptor.path[0].clone()
        } else if !descriptor.cmd.is_empty() {
            String::from_utf8(descriptor.cmd.to_vec())
                .map_err(|e| tonic::Status::invalid_argument(format!("Invalid cmd: {}", e)))?
        } else {
            return Err(tonic::Status::invalid_argument(
                "FlightDescriptor must have a path or cmd",
            ));
        };

        let data = self
            .get_data(&ticket_key)
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::not_found(format!("Flight not found: {}", ticket_key)))?;

        let schema = data.schema();
        let ipc_opts = IpcWriteOptions::default();
        let schema_result: SchemaResult = SchemaAsIpc::new(schema.as_ref(), &ipc_opts)
            .try_into()
            .map_err(|e: arrow_schema::ArrowError| {
            tonic::Status::internal(format!("Schema encode error: {}", e))
        })?;

        Ok(Response::new(schema_result))
    }

    async fn do_get(
        &self,
        request: Request<Ticket>,
    ) -> std::result::Result<Response<Self::DoGetStream>, tonic::Status> {
        let ticket = request.into_inner();
        let ticket_str = String::from_utf8(ticket.ticket.to_vec())
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid ticket: {}", e)))?;

        info!("DoGet request for ticket: {}", ticket_str);

        // Retrieve data
        let data = self
            .get_data(&ticket_str)
            .map_err(|e| tonic::Status::internal(e.to_string()))?
            .ok_or_else(|| tonic::Status::not_found(format!("Ticket not found: {}", ticket_str)))?;

        // Convert RecordBatch to FlightData stream
        let flight_data_vec = arrow_flight::utils::batches_to_flight_data(
            data.schema().as_ref(),
            vec![(*data).clone()],
        )
        .map_err(|e| tonic::Status::internal(format!("Failed to encode batches: {}", e)))?
        .into_iter()
        .map(Ok)
        .collect::<Vec<_>>();

        let stream = stream::iter(flight_data_vec);
        Ok(Response::new(Box::pin(stream)))
    }

    async fn do_put(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> std::result::Result<Response<Self::DoPutStream>, tonic::Status> {
        debug!("DoPut request received");

        let mut stream = request.into_inner();
        let mut flight_data_vec = Vec::new();

        // Collect all FlightData messages
        while let Some(data_result) = stream.next().await {
            flight_data_vec.push(data_result?);
        }

        // Convert FlightData to RecordBatches
        let batches = arrow_flight::utils::flight_data_to_batches(&flight_data_vec)
            .map_err(|e| tonic::Status::internal(format!("Failed to decode batches: {}", e)))?;

        info!("DoPut received {} batches", batches.len());

        // Store batches (using a generated ticket)
        for (i, batch) in batches.into_iter().enumerate() {
            let ticket = format!("uploaded_{}", i);
            self.store_data(ticket, Arc::new(batch))
                .map_err(|e| tonic::Status::internal(e.to_string()))?;
        }

        // Return success
        let result = PutResult {
            app_metadata: Bytes::new(),
        };

        let stream = stream::once(async { Ok(result) });
        Ok(Response::new(Box::pin(stream)))
    }

    async fn do_action(
        &self,
        request: Request<Action>,
    ) -> std::result::Result<Response<Self::DoActionStream>, tonic::Status> {
        let action = request.into_inner();
        info!("DoAction request: {}", action.r#type);

        match action.r#type.as_str() {
            "list_tickets" => {
                let tickets = self
                    .list_tickets()
                    .map_err(|e| tonic::Status::internal(e.to_string()))?;

                let result = arrow_flight::Result {
                    body: serde_json::to_vec(&tickets)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Serialization error: {}", e))
                        })?
                        .into(),
                };

                let stream = stream::once(async { Ok(result) });
                Ok(Response::new(Box::pin(stream)))
            }
            "remove_ticket" => {
                let ticket = String::from_utf8(action.body.to_vec()).map_err(|e| {
                    tonic::Status::invalid_argument(format!("Invalid ticket: {}", e))
                })?;

                self.remove_data(&ticket)
                    .map_err(|e| tonic::Status::internal(e.to_string()))?;

                let result = arrow_flight::Result {
                    body: Bytes::from("removed"),
                };

                let stream = stream::once(async { Ok(result) });
                Ok(Response::new(Box::pin(stream)))
            }
            "list_actions" => {
                // Reflection action: return JSON array of supported action descriptors.
                let actions = vec![
                    serde_json::json!({"type": "list_tickets", "description": "List all available tickets"}),
                    serde_json::json!({"type": "remove_ticket", "description": "Remove a ticket from the server"}),
                    serde_json::json!({"type": "list_actions", "description": "List all supported actions (reflection)"}),
                    serde_json::json!({"type": "ping", "description": "Health check — returns 'pong'"}),
                ];

                let result = arrow_flight::Result {
                    body: serde_json::to_vec(&actions)
                        .map_err(|e| {
                            tonic::Status::internal(format!("Serialization error: {}", e))
                        })?
                        .into(),
                };

                let stream = stream::once(async { Ok(result) });
                Ok(Response::new(Box::pin(stream)))
            }
            "ping" => {
                let result = arrow_flight::Result {
                    body: Bytes::from_static(b"pong"),
                };
                let stream = stream::once(async { Ok(result) });
                Ok(Response::new(Box::pin(stream)))
            }
            _ => Err(tonic::Status::unimplemented(format!(
                "Action not implemented: {}",
                action.r#type
            ))),
        }
    }

    async fn list_actions(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<Self::ListActionsStream>, tonic::Status> {
        debug!("List actions request received");

        let actions = vec![
            ActionType {
                r#type: "list_tickets".to_string(),
                description: "List all available tickets".to_string(),
            },
            ActionType {
                r#type: "remove_ticket".to_string(),
                description: "Remove a ticket from the server".to_string(),
            },
        ];

        let stream = stream::iter(actions.into_iter().map(Ok));
        Ok(Response::new(Box::pin(stream)))
    }

    async fn do_exchange(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> std::result::Result<Response<Self::DoExchangeStream>, tonic::Status> {
        debug!("DoExchange request received — echo/passthrough mode");

        let mut incoming = request.into_inner();
        let mut echo_items: Vec<std::result::Result<FlightData, tonic::Status>> = Vec::new();

        while let Some(item) = incoming.next().await {
            match item {
                Ok(flight_data) => {
                    echo_items.push(Ok(flight_data));
                }
                Err(status) => {
                    // Propagate the first error as the terminal item.
                    echo_items.push(Err(status));
                    break;
                }
            }
        }

        info!("DoExchange echoing {} items", echo_items.len());
        let stream = stream::iter(echo_items);
        Ok(Response::new(Box::pin(stream)))
    }

    async fn poll_flight_info(
        &self,
        request: Request<FlightDescriptor>,
    ) -> std::result::Result<Response<arrow_flight::PollInfo>, tonic::Status> {
        let descriptor = request.into_inner();
        debug!("Poll flight info request received");

        // Resolve the ticket key from the descriptor (same logic as get_flight_info).
        let ticket_key = if !descriptor.path.is_empty() {
            descriptor.path[0].clone()
        } else if !descriptor.cmd.is_empty() {
            String::from_utf8(descriptor.cmd.to_vec())
                .map_err(|e| tonic::Status::invalid_argument(format!("Invalid cmd: {}", e)))?
        } else {
            return Err(tonic::Status::invalid_argument(
                "FlightDescriptor must have a path or cmd",
            ));
        };

        let data_opt = self
            .get_data(&ticket_key)
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        // If data is ready, return complete FlightInfo (no pending descriptor, progress = 1.0).
        // If data is still pending, echo the descriptor back (client should retry), progress = None.
        if let Some(data) = data_opt {
            let schema = data.schema();
            let ipc_opts = IpcWriteOptions::default();
            let schema_bytes: arrow_flight::IpcMessage =
                SchemaAsIpc::new(schema.as_ref(), &ipc_opts)
                    .try_into()
                    .map_err(|e: arrow_schema::ArrowError| {
                        tonic::Status::internal(format!("Schema encode error: {}", e))
                    })?;

            let endpoint = FlightEndpoint {
                ticket: Some(Ticket {
                    ticket: Bytes::from(ticket_key),
                }),
                location: vec![],
                expiration_time: None,
                app_metadata: Bytes::new(),
            };

            let flight_info = FlightInfo {
                schema: schema_bytes.0,
                flight_descriptor: Some(descriptor),
                endpoint: vec![endpoint],
                total_records: data.num_rows() as i64,
                total_bytes: -1,
                ordered: false,
                app_metadata: Bytes::new(),
            };

            Ok(Response::new(arrow_flight::PollInfo {
                info: Some(flight_info),
                // flight_descriptor is None -> indicates the query is complete.
                flight_descriptor: None,
                progress: Some(1.0),
                expiration_time: None,
            }))
        } else {
            // Data not yet ready; client should poll again using the same descriptor.
            Ok(Response::new(arrow_flight::PollInfo {
                info: None,
                flight_descriptor: Some(descriptor),
                progress: None,
                expiration_time: None,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::Int32Array;
    use arrow::datatypes::{DataType, Field, Schema};

    fn create_test_batch() -> std::result::Result<Arc<RecordBatch>, Box<dyn std::error::Error>> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int32,
            false,
        )]));

        let array = Int32Array::from(vec![1, 2, 3, 4, 5]);

        Ok(Arc::new(RecordBatch::try_new(
            schema,
            vec![Arc::new(array)],
        )?))
    }

    #[test]
    fn test_server_creation() {
        let server = FlightServer::new();
        assert!(!server.enable_auth);
    }

    #[test]
    fn test_store_and_retrieve_data() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server = FlightServer::new();
        let batch = create_test_batch()?;

        server.store_data("test_ticket".to_string(), batch.clone())?;

        let retrieved = server
            .get_data("test_ticket")?
            .ok_or_else(|| Box::<dyn std::error::Error>::from("should exist"))?;

        assert_eq!(retrieved.num_rows(), batch.num_rows());
        Ok(())
    }

    #[test]
    fn test_remove_data() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server = FlightServer::new();
        let batch = create_test_batch()?;

        server.store_data("test_ticket".to_string(), batch)?;

        let removed = server
            .remove_data("test_ticket")?
            .ok_or_else(|| Box::<dyn std::error::Error>::from("should exist"))?;

        assert_eq!(removed.num_rows(), 5);

        let retrieved = server.get_data("test_ticket")?;
        assert!(retrieved.is_none());
        Ok(())
    }

    #[test]
    fn test_list_tickets() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server = FlightServer::new();

        server.store_data("ticket1".to_string(), create_test_batch()?)?;
        server.store_data("ticket2".to_string(), create_test_batch()?)?;

        let tickets = server.list_tickets()?;
        assert_eq!(tickets.len(), 2);
        assert!(tickets.contains(&"ticket1".to_string()));
        assert!(tickets.contains(&"ticket2".to_string()));
        Ok(())
    }

    #[test]
    fn test_authentication() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server = FlightServer::new().with_auth();
        assert!(server.enable_auth);

        server.add_auth_token("token123".to_string(), "user1".to_string())?;

        // Verify token exists via auth_tokens (verify_token method not exposed)
        assert!(
            server
                .auth_tokens
                .read()
                .map_err(|e| Box::<dyn std::error::Error>::from(format!("lock poisoned: {}", e)))?
                .contains_key("token123")
        );
        assert!(
            !server
                .auth_tokens
                .read()
                .map_err(|e| Box::<dyn std::error::Error>::from(format!("lock poisoned: {}", e)))?
                .contains_key("invalid")
        );
        Ok(())
    }
}
