use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

/// Where a domain record came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvenanceSource {
    Api,
    WebSocket,
    Database,
    Manual,
    Computed,
    External(String),
}

/// Minimal provenance metadata that can travel with observations, events,
/// and derived signals without coupling to a storage backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceMetadata {
    pub source: ProvenanceSource,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rfc3339::option"
    )]
    pub received_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Default for ProvenanceMetadata {
    fn default() -> Self {
        Self::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
    }
}

impl ProvenanceMetadata {
    pub fn new(source: ProvenanceSource, observed_at: OffsetDateTime) -> Self {
        Self {
            source,
            observed_at,
            received_at: None,
            confidence: None,
            trace_id: None,
            inputs: Vec::new(),
            note: None,
        }
    }

    pub fn with_confidence(mut self, confidence: Decimal) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    pub fn with_inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

/// A raw fact captured from the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation<T> {
    pub value: T,
    pub provenance: ProvenanceMetadata,
}

impl<T> Observation<T> {
    pub fn new(value: T, provenance: ProvenanceMetadata) -> Self {
        Self { value, provenance }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Observation<U> {
        Observation {
            value: f(self.value),
            provenance: self.provenance,
        }
    }
}

/// A discrete occurrence recorded by the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event<T> {
    pub value: T,
    pub provenance: ProvenanceMetadata,
}

impl<T> Event<T> {
    pub fn new(value: T, provenance: ProvenanceMetadata) -> Self {
        Self { value, provenance }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Event<U> {
        Event {
            value: f(self.value),
            provenance: self.provenance,
        }
    }
}

/// A computed signal derived from one or more observations or events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedSignal<T> {
    pub value: T,
    pub provenance: ProvenanceMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
}

impl<T> DerivedSignal<T> {
    pub fn new(value: T, provenance: ProvenanceMetadata) -> Self {
        Self {
            value,
            provenance,
            derived_from: Vec::new(),
        }
    }

    pub fn with_derivation<I, S>(mut self, derived_from: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.derived_from = derived_from.into_iter().map(Into::into).collect();
        self
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> DerivedSignal<U> {
        DerivedSignal {
            value: f(self.value),
            provenance: self.provenance,
            derived_from: self.derived_from,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_metadata_defaults_are_empty() {
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);

        assert!(provenance.received_at.is_none());
        assert!(provenance.confidence.is_none());
        assert!(provenance.trace_id.is_none());
        assert!(provenance.inputs.is_empty());
        assert!(provenance.note.is_none());
    }

    #[test]
    fn wrappers_preserve_provenance_when_mapped() {
        let provenance = ProvenanceMetadata::new(ProvenanceSource::Api, OffsetDateTime::UNIX_EPOCH);
        let observation = Observation::new(10_u32, provenance.clone());
        let event = Event::new("tick", provenance.clone());
        let signal = DerivedSignal::new(1.5_f64, provenance.clone())
            .with_derivation(vec!["Observation:price", "Event:trade"]);

        let mapped_observation = observation.map(|value| value.to_string());
        let mapped_event = event.map(|value| value.len());
        let mapped_signal = signal.map(|value| value.to_string());

        assert_eq!(mapped_observation.provenance, provenance);
        assert_eq!(mapped_event.provenance, provenance);
        assert_eq!(mapped_signal.provenance, provenance);
        assert_eq!(mapped_signal.derived_from.len(), 2);
    }
}
