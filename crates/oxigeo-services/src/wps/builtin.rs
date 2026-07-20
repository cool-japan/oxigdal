//! Built-in WPS processes

use crate::error::ServiceResult;
use crate::wps::{
    ComplexDataType, DataType, InputDescription, LiteralDataType, OutputDescription, Process,
    ProcessInputs, ProcessOutputs, WpsState,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Register built-in processes
pub fn register_builtin_processes(state: &WpsState) {
    state.add_process(Arc::new(BufferProcess)).ok();
    state.add_process(Arc::new(ClipProcess)).ok();
    state.add_process(Arc::new(UnionProcess)).ok();
}

/// Buffer process
struct BufferProcess;

#[async_trait]
impl Process for BufferProcess {
    fn identifier(&self) -> &str {
        "buffer"
    }

    fn title(&self) -> &str {
        "Buffer Geometry"
    }

    fn abstract_text(&self) -> Option<&str> {
        Some("Creates a buffer around input geometry")
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![
            InputDescription {
                identifier: "geometry".to_string(),
                title: "Input Geometry".to_string(),
                abstract_text: None,
                data_type: DataType::Complex(ComplexDataType {
                    mime_type: "application/geo+json".to_string(),
                    encoding: None,
                    schema: None,
                }),
                min_occurs: 1,
                max_occurs: Some(1),
            },
            InputDescription {
                identifier: "distance".to_string(),
                title: "Buffer Distance".to_string(),
                abstract_text: None,
                data_type: DataType::Literal(LiteralDataType {
                    data_type: "double".to_string(),
                    allowed_values: None,
                }),
                min_occurs: 1,
                max_occurs: Some(1),
            },
        ]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![OutputDescription {
            identifier: "result".to_string(),
            title: "Buffered Geometry".to_string(),
            abstract_text: None,
            data_type: DataType::Complex(ComplexDataType {
                mime_type: "application/geo+json".to_string(),
                encoding: None,
                schema: None,
            }),
        }]
    }

    async fn execute(&self, _inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        Ok(ProcessOutputs::default())
    }
}

/// Clip process
struct ClipProcess;

#[async_trait]
impl Process for ClipProcess {
    fn identifier(&self) -> &str {
        "clip"
    }

    fn title(&self) -> &str {
        "Clip Geometry"
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![]
    }

    async fn execute(&self, _inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        Ok(ProcessOutputs::default())
    }
}

/// Union process
struct UnionProcess;

#[async_trait]
impl Process for UnionProcess {
    fn identifier(&self) -> &str {
        "union"
    }

    fn title(&self) -> &str {
        "Union Geometries"
    }

    fn inputs(&self) -> Vec<InputDescription> {
        vec![]
    }

    fn outputs(&self) -> Vec<OutputDescription> {
        vec![]
    }

    async fn execute(&self, _inputs: ProcessInputs) -> ServiceResult<ProcessOutputs> {
        Ok(ProcessOutputs::default())
    }
}
