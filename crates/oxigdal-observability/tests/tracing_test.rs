//! Tests for distributed tracing.

use oxigdal_observability::tracing::context::{extract_context, inject_context};
use std::collections::HashMap;

#[test]
fn test_context_propagation() {
    let mut headers = HashMap::new();
    let context = opentelemetry::Context::current();

    inject_context(&context, &mut headers);
    let _extracted = extract_context(&headers);

    // Basic test - context propagation should not panic
}
