use opentelemetry::global;
use std::collections::HashMap;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub fn make_context_propagator() -> HashMap<String, String> {
    let span = Span::current();

    let mut context_propagator = HashMap::<String, String>::default();
    let context = span.context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut context_propagator);
    });

    tracing::debug!("context propagator: {:?}", context_propagator);
    context_propagator
}
