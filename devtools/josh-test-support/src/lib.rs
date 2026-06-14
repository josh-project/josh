use std::time::Instant;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

struct Timing(Instant);

/// The span's fields, formatted once at creation for display on close.
struct Fields(Option<String>);

#[derive(Default, Clone, Copy)]
pub struct SpanTimingLayer;

impl SpanTimingLayer {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Default)]
struct EventVisitor(String);

impl EventVisitor {
    fn push(&mut self, part: String) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&part);
    }
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.push(format!("{value:?}"));
        } else {
            self.push(format!("{}={:?}", field.name(), value));
        }
    }
}

impl<S> Layer<S> for SpanTimingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut visitor = EventVisitor::default();
            attrs.record(&mut visitor);

            let mut extensions = span.extensions_mut();
            extensions.insert(Timing(Instant::now()));
            extensions.insert(if attrs.is_empty() {
                Fields(None)
            } else {
                Fields(Some(visitor.0))
            });
        }
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        if !visitor.0.is_empty() {
            eprintln!("{}", visitor.0);
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };

        let elapsed = span
            .extensions()
            .get::<Timing>()
            .map(|timing| timing.0.elapsed());

        let Some(elapsed) = elapsed else {
            return;
        };

        let extensions = span.extensions();
        let fields = extensions.get::<Fields>().and_then(|fields| fields.0.as_deref());

        match fields {
            Some(fields) => eprintln!("[{}] {:.2?} {}", span.name(), elapsed, fields),
            None => eprintln!("[{}] {:.2?}", span.name(), elapsed),
        }
    }
}

pub fn init_tracing(default_directive: &str) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::{EnvFilter, Registry};

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    let subscriber = Registry::default()
        .with(filter)
        .with(SpanTimingLayer::new());

    let _ = tracing::subscriber::set_global_default(subscriber);
}
