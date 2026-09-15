#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlertTopic {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlertMessage {
    pub id: u64,
    pub timestamp_ms: u64,
    pub topic: String,
    pub severity: AlertSeverity,
    pub source: String,
    pub payload: String,
}

#[derive(Clone, Debug, Default)]
pub struct ObservatoryEventBroker {
    pub topics: Vec<AlertTopic>,
    pub events: Vec<AlertMessage>,
    pub next_id: u64,
}

impl ObservatoryEventBroker {
    pub fn new() -> Self {
        let mut broker = Self {
            topics: Vec::new(),
            events: Vec::new(),
            next_id: 1,
        };

        broker.register_topic("mount/meridian_flip", "Meridian flip alerts and state updates");
        broker.register_topic("weather/safety", "Atmospheric and weather limit alarms");
        broker.register_topic("science/transient", "New transient source discoveries (supernovae, asteroids)");
        broker.register_topic("qc/frame_rejection", "Quality control frame rejection notifications");
        broker.register_topic("autofocus/v_curve", "Autofocus completion and optimal step convergence");

        broker
    }

    pub fn register_topic(&mut self, name: &str, description: &str) {
        if !self.topics.iter().any(|t| t.name == name) {
            self.topics.push(AlertTopic {
                name: name.to_string(),
                description: description.to_string(),
            });
        }
    }

    pub fn publish_event(
        &mut self,
        topic: &str,
        severity: AlertSeverity,
        source: &str,
        payload: &str,
        timestamp_ms: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.events.push(AlertMessage {
            id,
            timestamp_ms,
            topic: topic.to_string(),
            severity,
            source: source.to_string(),
            payload: payload.to_string(),
        });

        id
    }

    pub fn query_events_by_topic(&self, topic: &str) -> Vec<&AlertMessage> {
        self.events.iter().filter(|e| e.topic == topic).collect()
    }

    pub fn query_events_by_min_severity(&self, min_severity: AlertSeverity) -> Vec<&AlertMessage> {
        self.events
            .iter()
            .filter(|e| match min_severity {
                AlertSeverity::Info => true,
                AlertSeverity::Warning => e.severity != AlertSeverity::Info,
                AlertSeverity::Critical => e.severity == AlertSeverity::Critical,
            })
            .collect()
    }

    pub fn to_json(&self) -> String {
        let mut json = String::new();
        json.push_str("{\n");
        json.push_str("  \"total_events\": ");
        json.push_str(&self.events.len().to_string());
        json.push_str(",\n");
        json.push_str("  \"topics_count\": ");
        json.push_str(&self.topics.len().to_string());
        json.push_str(",\n");
        json.push_str("  \"events\": [\n");

        for (i, e) in self.events.iter().enumerate() {
            json.push_str("    {\n");
            json.push_str("      \"id\": ");
            json.push_str(&e.id.to_string());
            json.push_str(",\n");
            json.push_str("      \"timestamp_ms\": ");
            json.push_str(&e.timestamp_ms.to_string());
            json.push_str(",\n");
            json.push_str("      \"topic\": \"");
            json.push_str(&e.topic);
            json.push_str("\",\n");
            json.push_str("      \"severity\": \"");
            json.push_str(&format!("{:?}", e.severity));
            json.push_str("\",\n");
            json.push_str("      \"source\": \"");
            json.push_str(&e.source);
            json.push_str("\",\n");
            json.push_str("      \"payload\": \"");
            json.push_str(&e.payload.replace('"', "\\\""));
            json.push_str("\"\n    }");
            if i + 1 < self.events.len() {
                json.push_str(",");
            }
            json.push_str("\n");
        }

        json.push_str("  ]\n");
        json.push_str("}\n");
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_and_queries_observatory_events() {
        let mut broker = ObservatoryEventBroker::new();

        let id1 = broker.publish_event(
            "science/transient",
            AlertSeverity::Critical,
            "transient_pipeline",
            "Supernova candidate detected at RA 10.6847, Dec 41.2687 (SNR=35.0)",
            1000,
        );

        let id2 = broker.publish_event(
            "mount/meridian_flip",
            AlertSeverity::Warning,
            "safety_guard",
            "Target crossing meridian in 180s - pier flip scheduled",
            1020,
        );

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(broker.events.len(), 2);

        let trans = broker.query_events_by_topic("science/transient");
        assert_eq!(trans.len(), 1);
        assert_eq!(trans[0].severity, AlertSeverity::Critical);

        let warnings_and_crit = broker.query_events_by_min_severity(AlertSeverity::Warning);
        assert_eq!(warnings_and_crit.len(), 2);
    }
}
