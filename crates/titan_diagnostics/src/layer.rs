use crate::{active_sink, sink::unix_ms, FailureContext, RecentLog};
use alloc::collections::BTreeMap;
use core::{cell::RefCell, fmt};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

thread_local! {
    static SYSTEMS: RefCell<Vec<(u64, FailureContext)>> = const { RefCell::new(Vec::new()) };
    static SCHEDULES: RefCell<Vec<(u64, String)>> = const { RefCell::new(Vec::new()) };
}

/// Tracing layer capturing WARN/ERROR events and Bevy's INFO-level schedule and
/// system spans. Install exactly once, e.g. via `LogPlugin::custom_layer`.
/// It can be installed before the plugin; events outside a live diagnostics app
/// are ignored. Subscriber filters must permit INFO spans and WARN/ERROR events.
#[derive(Debug, Default)]
pub struct DiagnosticsLayer;

#[derive(Clone)]
struct SpanInfo {
    kind: &'static str,
    name: String,
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for DiagnosticsLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let kind = match attrs.metadata().name() {
            "schedule" => "schedule",
            "system" => "system",
            "system_commands" => "command",
            _ => return,
        };
        // Avoid attributing similarly named user spans to Bevy.
        if !attrs.metadata().target().starts_with("bevy_ecs::") {
            return;
        }
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(name) = fields.0.remove("name")
            && let Some(span) = ctx.span(id)
        {
            span.extensions_mut().insert(SpanInfo { kind, name });
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let extensions = span.extensions();
        let Some(info) = extensions.get::<SpanInfo>() else {
            return;
        };
        let Some(sink) = active_sink() else {
            return;
        };
        if info.kind == "schedule" {
            SCHEDULES.with(|schedules| {
                schedules
                    .borrow_mut()
                    .push((id.into_u64(), info.name.clone()));
            });
            sink.lock().schedules.push((
                id.into_u64(),
                std::thread::current().id(),
                info.name.clone(),
            ));
        } else {
            SYSTEMS.with(|systems| {
                systems.borrow_mut().push((
                    id.into_u64(),
                    FailureContext {
                        kind: info.kind.into(),
                        name: info.name.clone(),
                        last_run: None,
                        system: None,
                        on_set: None,
                    },
                ));
            });
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let extensions = span.extensions();
        let Some(info) = extensions.get::<SpanInfo>() else {
            return;
        };
        if info.kind == "schedule" {
            SCHEDULES.with(|schedules| {
                let mut schedules = schedules.borrow_mut();
                if let Some(index) = schedules
                    .iter()
                    .rposition(|(span_id, _)| *span_id == id.into_u64())
                {
                    schedules.remove(index);
                }
            });
            if let Some(sink) = active_sink() {
                let mut state = sink.lock();
                if let Some(index) = state.schedules.iter().rposition(|(span_id, owner, _)| {
                    *span_id == id.into_u64() && *owner == std::thread::current().id()
                }) {
                    state.schedules.remove(index);
                }
            }
        } else {
            SYSTEMS.with(|systems| {
                let mut systems = systems.borrow_mut();
                if let Some(index) = systems
                    .iter()
                    .rposition(|(span_id, _)| *span_id == id.into_u64())
                {
                    systems.remove(index);
                }
            });
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if !matches!(
            *event.metadata().level(),
            tracing::Level::WARN | tracing::Level::ERROR
        ) {
            return;
        }
        if let Some(sink) = active_sink() {
            let mut fields = Fields::default();
            event.record(&mut fields);
            sink.log(RecentLog {
                unix_ms: unix_ms(),
                level: event.metadata().level().to_string(),
                target: event.metadata().target().into(),
                fields: fields.0,
            });
        }
    }
}

pub(crate) fn current_schedule() -> Option<String> {
    SCHEDULES.with(|schedules| schedules.borrow().last().map(|(_, name)| name.clone()))
}

pub(crate) fn panic_context() -> Option<FailureContext> {
    SYSTEMS.with(|systems| systems.borrow().last().map(|(_, context)| context.clone()))
}
