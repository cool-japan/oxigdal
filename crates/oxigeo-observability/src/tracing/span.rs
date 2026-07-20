//! Span creation and management for distributed tracing.

use opentelemetry::trace::{SpanKind, Status, TraceContextExt, Tracer};
use opentelemetry::{Context, KeyValue};

/// Span builder for creating custom spans with a specific tracer.
pub struct SpanBuilder<T>
where
    T: Tracer,
    T::Span: Send + Sync + 'static,
{
    tracer: T,
    name: String,
    kind: SpanKind,
    attributes: Vec<KeyValue>,
    parent_context: Option<Context>,
}

impl<T> SpanBuilder<T>
where
    T: Tracer,
    T::Span: Send + Sync + 'static,
{
    /// Create a new span builder.
    pub fn new(tracer: T, name: impl Into<String>) -> Self {
        Self {
            tracer,
            name: name.into(),
            kind: SpanKind::Internal,
            attributes: Vec::new(),
            parent_context: None,
        }
    }

    /// Set the span kind.
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add an attribute to the span.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .push(KeyValue::new(key.into(), value.into()));
        self
    }

    /// Set the parent context.
    pub fn with_parent_context(mut self, context: Context) -> Self {
        self.parent_context = Some(context);
        self
    }

    /// Build and start the span, returning only the context.
    pub fn start(self) -> Context {
        let mut builder = self.tracer.span_builder(self.name);
        builder.span_kind = Some(self.kind);
        builder.attributes = Some(self.attributes);

        let span = if let Some(parent) = self.parent_context {
            self.tracer.build_with_context(builder, &parent)
        } else {
            builder.start(&self.tracer)
        };

        Context::current().with_span(span)
    }
}

/// Span recorder for recording events and attributes on active spans.
pub struct SpanRecorder;

impl SpanRecorder {
    /// Record an event on the current span.
    pub fn record_event(name: impl Into<String>, attributes: Vec<KeyValue>) {
        let cx = Context::current();
        let span = cx.span();
        span.add_event(name.into(), attributes);
    }

    /// Record an error on the current span.
    pub fn record_error(error: &dyn std::error::Error) {
        let cx = Context::current();
        let span = cx.span();

        span.set_status(Status::error(error.to_string()));
        span.add_event(
            "exception",
            vec![
                KeyValue::new("exception.type", std::any::type_name_of_val(error)),
                KeyValue::new("exception.message", error.to_string()),
            ],
        );
    }

    /// Record success on the current span.
    pub fn record_success() {
        let cx = Context::current();
        let span = cx.span();
        span.set_status(Status::Ok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_span_builder() {
        let tracer = global::tracer("test");
        let _context = SpanBuilder::new(tracer, "test_span")
            .with_kind(SpanKind::Server)
            .with_attribute("key1", "value1")
            .start();
    }
}
