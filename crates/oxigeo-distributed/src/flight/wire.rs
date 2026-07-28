//! Wire protocol for the `execute_task` Flight action.
//!
//! This module is the glue that connects the three previously-disconnected
//! distributed components — the [`Coordinator`](crate::coordinator::Coordinator)
//! (task bookkeeping), the [`Worker`](crate::worker::Worker) (task execution),
//! and Arrow Flight (data transport) — into an actual multi-process pipeline.
//!
//! A task-execution request/response is a single self-describing frame:
//!
//! ```text
//! ┌──────────────┬───────────────────────┬──────────────────────────────┐
//! │ u32 LE       │ JSON header           │ Arrow IPC stream (optional)   │
//! │ header_len   │ (Task / result meta)  │ input or output RecordBatch   │
//! └──────────────┴───────────────────────┴──────────────────────────────┘
//! ```
//!
//! Carrying the input batch inline keeps the RPC hermetic: the coordinator does
//! not have to pre-stage data under a separately-negotiated ticket, and the
//! worker returns its result batch in the same envelope.

use crate::error::{DistributedError, Result};
use crate::task::Task;
use arrow::record_batch::RecordBatch;
use arrow_ipc::reader::StreamReader;
use arrow_ipc::writer::StreamWriter;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Flight action type used to submit a task to a worker's Flight server.
pub const EXECUTE_TASK_ACTION: &str = "execute_task";

/// JSON header of an [`EXECUTE_TASK_ACTION`] request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskRequest {
    /// The task to execute.
    pub task: Task,
    /// Whether an input `RecordBatch` (Arrow IPC) follows the header.
    pub has_input: bool,
}

/// JSON header of an [`EXECUTE_TASK_ACTION`] response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteTaskResponse {
    /// Whether the task completed successfully.
    pub success: bool,
    /// Error message when the task failed.
    pub error: Option<String>,
    /// Wall-clock execution time reported by the worker, in milliseconds.
    pub execution_time_ms: u64,
    /// Number of rows in the output batch (0 when there is none).
    pub num_rows: usize,
    /// Whether an output `RecordBatch` (Arrow IPC) follows the header.
    pub has_output: bool,
}

/// Serialize a single [`RecordBatch`] to an Arrow IPC stream byte buffer.
pub fn encode_batch_ipc(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut buffer, batch.schema().as_ref())
            .map_err(|e| DistributedError::arrow(format!("IPC writer init failed: {e}")))?;
        writer
            .write(batch)
            .map_err(|e| DistributedError::arrow(format!("IPC write failed: {e}")))?;
        writer
            .finish()
            .map_err(|e| DistributedError::arrow(format!("IPC finish failed: {e}")))?;
    }
    Ok(buffer)
}

/// Decode a single [`RecordBatch`] from an Arrow IPC stream byte buffer.
pub fn decode_batch_ipc(bytes: &[u8]) -> Result<RecordBatch> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None)
        .map_err(|e| DistributedError::arrow(format!("IPC reader init failed: {e}")))?;
    let mut batch = None;
    for item in reader {
        let b = item.map_err(|e| DistributedError::arrow(format!("IPC read failed: {e}")))?;
        // The envelope always carries exactly one batch; keep the first.
        if batch.is_none() {
            batch = Some(b);
        }
    }
    batch.ok_or_else(|| DistributedError::arrow("IPC stream carried no record batch"))
}

/// Frame a JSON header and an optional batch into a single wire buffer.
fn frame(header_json: Vec<u8>, batch: Option<&RecordBatch>) -> Result<Bytes> {
    let header_len = u32::try_from(header_json.len())
        .map_err(|_| DistributedError::flight_rpc("execute_task header too large"))?;

    let mut out = Vec::with_capacity(4 + header_json.len());
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&header_json);
    if let Some(batch) = batch {
        out.extend_from_slice(&encode_batch_ipc(batch)?);
    }
    Ok(Bytes::from(out))
}

/// Split a wire buffer back into its JSON header bytes and trailing IPC bytes.
fn unframe(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    if bytes.len() < 4 {
        return Err(DistributedError::flight_rpc(
            "execute_task frame shorter than its length prefix",
        ));
    }
    let header_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let header_end = 4usize
        .checked_add(header_len)
        .filter(|&end| end <= bytes.len())
        .ok_or_else(|| DistributedError::flight_rpc("execute_task header length out of range"))?;
    Ok((&bytes[4..header_end], &bytes[header_end..]))
}

/// Encode an execute-task request (header + optional input batch) into a Flight
/// action body.
pub fn encode_execute_request(task: &Task, input: Option<&RecordBatch>) -> Result<Bytes> {
    let header = ExecuteTaskRequest {
        task: task.clone(),
        has_input: input.is_some(),
    };
    let header_json = serde_json::to_vec(&header)
        .map_err(|e| DistributedError::task_serialization(format!("encode request: {e}")))?;
    frame(header_json, input)
}

/// Decode an execute-task request body into its task and optional input batch.
pub fn decode_execute_request(bytes: &[u8]) -> Result<(Task, Option<RecordBatch>)> {
    let (header_bytes, tail) = unframe(bytes)?;
    let header: ExecuteTaskRequest = serde_json::from_slice(header_bytes)
        .map_err(|e| DistributedError::task_serialization(format!("decode request: {e}")))?;
    let input = if header.has_input {
        Some(decode_batch_ipc(tail)?)
    } else {
        None
    };
    Ok((header.task, input))
}

/// Encode an execute-task response (header + optional output batch) into a
/// Flight action result body.
pub fn encode_execute_response(
    response: &ExecuteTaskResponse,
    output: Option<&RecordBatch>,
) -> Result<Bytes> {
    let header_json = serde_json::to_vec(response)
        .map_err(|e| DistributedError::task_serialization(format!("encode response: {e}")))?;
    frame(header_json, output)
}

/// Decode an execute-task response body into its metadata and optional output
/// batch.
pub fn decode_execute_response(bytes: &[u8]) -> Result<(ExecuteTaskResponse, Option<RecordBatch>)> {
    let (header_bytes, tail) = unframe(bytes)?;
    let header: ExecuteTaskResponse = serde_json::from_slice(header_bytes)
        .map_err(|e| DistributedError::task_serialization(format!("decode response: {e}")))?;
    let output = if header.has_output {
        Some(decode_batch_ipc(tail)?)
    } else {
        None
    };
    Ok((header, output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{PartitionId, TaskId, TaskOperation};
    use arrow::array::Int32Array;
    use arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    fn sample_batch() -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int32,
            false,
        )]));
        RecordBatch::try_new(
            schema,
            vec![Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5]))],
        )
        .expect("batch")
    }

    fn sample_task() -> Task {
        Task::new(
            TaskId(7),
            PartitionId(2),
            TaskOperation::Filter {
                expression: "value > 2".to_string(),
            },
        )
    }

    #[test]
    fn batch_ipc_round_trips() {
        let batch = sample_batch();
        let bytes = encode_batch_ipc(&batch).expect("encode");
        let restored = decode_batch_ipc(&bytes).expect("decode");
        assert_eq!(restored.num_rows(), 5);
        assert_eq!(restored.num_columns(), 1);
    }

    #[test]
    fn request_round_trips_with_input() {
        let task = sample_task();
        let batch = sample_batch();
        let body = encode_execute_request(&task, Some(&batch)).expect("encode");
        let (decoded_task, input) = decode_execute_request(&body).expect("decode");
        assert_eq!(decoded_task.id, TaskId(7));
        let input = input.expect("input present");
        assert_eq!(input.num_rows(), 5);
    }

    #[test]
    fn request_round_trips_without_input() {
        let task = sample_task();
        let body = encode_execute_request(&task, None).expect("encode");
        let (decoded_task, input) = decode_execute_request(&body).expect("decode");
        assert_eq!(decoded_task.partition_id, PartitionId(2));
        assert!(input.is_none());
    }

    #[test]
    fn response_round_trips_with_output() {
        let batch = sample_batch();
        let resp = ExecuteTaskResponse {
            success: true,
            error: None,
            execution_time_ms: 42,
            num_rows: 5,
            has_output: true,
        };
        let body = encode_execute_response(&resp, Some(&batch)).expect("encode");
        let (decoded, output) = decode_execute_response(&body).expect("decode");
        assert!(decoded.success);
        assert_eq!(decoded.execution_time_ms, 42);
        assert_eq!(output.expect("output").num_rows(), 5);
    }

    #[test]
    fn unframe_rejects_truncated_frame() {
        assert!(unframe(&[0, 1]).is_err());
        // header_len claims more bytes than present.
        assert!(unframe(&[255, 255, 255, 255, 0]).is_err());
    }
}
