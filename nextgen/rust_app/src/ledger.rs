#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerEventType {
    SessionStart,
    ConfigParsed,
    AstrometrySolved,
    FrameAcquired,
    FrameCalibrated,
    ReportGenerated,
    SessionComplete,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LedgerEntry {
    pub sequence_id: u64,
    pub event_type: LedgerEventType,
    pub target_name: Option<String>,
    pub message: String,
    pub timestamp_ms: u64,
    pub checksum: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionLedger {
    pub session_id: String,
    pub entries: Vec<LedgerEntry>,
}

fn compute_simple_checksum(seq: u64, event_str: &str, msg: &str) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in seq.to_le_bytes() {
        hash = (hash ^ (byte as u32)).wrapping_mul(0x01000193);
    }
    for byte in event_str.bytes() {
        hash = (hash ^ (byte as u32)).wrapping_mul(0x01000193);
    }
    for byte in msg.bytes() {
        hash = (hash ^ (byte as u32)).wrapping_mul(0x01000193);
    }
    hash
}

impl SessionLedger {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            entries: Vec::new(),
        }
    }

    pub fn record_event(
        &mut self,
        event_type: LedgerEventType,
        target_name: Option<&str>,
        message: &str,
        timestamp_ms: u64,
    ) {
        let sequence_id = self.entries.len() as u64 + 1;
        let event_str = format!("{:?}", event_type);
        let checksum = compute_simple_checksum(sequence_id, &event_str, message);

        self.entries.push(LedgerEntry {
            sequence_id,
            event_type,
            target_name: target_name.map(str::to_owned),
            message: message.to_string(),
            timestamp_ms,
            checksum,
        });
    }

    pub fn verify_integrity(&self) -> bool {
        for entry in &self.entries {
            let event_str = format!("{:?}", entry.event_type);
            let expected = compute_simple_checksum(entry.sequence_id, &event_str, &entry.message);
            if entry.checksum != expected {
                return false;
            }
        }
        true
    }

    pub fn to_json(&self) -> String {
        let mut json = String::new();
        json.push_str("{\n");
        json.push_str("  \"session_id\": \"");
        json.push_str(&self.session_id);
        json.push_str("\",\n");
        json.push_str("  \"entry_count\": ");
        json.push_str(&self.entries.len().to_string());
        json.push_str(",\n");
        json.push_str("  \"entries\": [\n");

        for (index, entry) in self.entries.iter().enumerate() {
            json.push_str("    {\n");
            json.push_str("      \"sequence_id\": ");
            json.push_str(&entry.sequence_id.to_string());
            json.push_str(",\n");
            json.push_str("      \"event_type\": \"");
            json.push_str(&format!("{:?}", entry.event_type));
            json.push_str("\",\n");
            json.push_str("      \"target_name\": ");
            if let Some(target) = &entry.target_name {
                json.push_str(&format!("\"{}\"", target));
            } else {
                json.push_str("null");
            }
            json.push_str(",\n");
            json.push_str("      \"message\": \"");
            json.push_str(&entry.message.replace('"', "\\\""));
            json.push_str("\",\n");
            json.push_str("      \"timestamp_ms\": ");
            json.push_str(&entry.timestamp_ms.to_string());
            json.push_str(",\n");
            json.push_str("      \"checksum\": ");
            json.push_str(&entry.checksum.to_string());
            json.push_str("\n    }");
            if index + 1 < self.entries.len() {
                json.push_str(",");
            }
            json.push_str("\n");
        }

        json.push_str("  ]\n");
        json.push_str("}\n");
        json
    }

    pub fn write_to_file(&self, path: &str) -> Result<(), String> {
        std::fs::write(path, self.to_json())
            .map_err(|error| format!("failed to write session ledger '{path}': {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_verifies_session_ledger_integrity() {
        let mut ledger = SessionLedger::new("SES-2026-001");
        ledger.record_event(LedgerEventType::SessionStart, None, "Session initiated", 1000);
        ledger.record_event(LedgerEventType::ConfigParsed, Some("M31"), "Campaign config loaded", 1020);
        ledger.record_event(LedgerEventType::FrameCalibrated, Some("M31"), "Bias and Flat applied", 1080);
        ledger.record_event(LedgerEventType::SessionComplete, None, "Session completed successfully", 1200);

        assert_eq!(ledger.entries.len(), 4);
        assert!(ledger.verify_integrity());

        let json = ledger.to_json();
        assert!(json.contains("\"session_id\": \"SES-2026-001\""));
        assert!(json.contains("\"event_type\": \"SessionStart\""));
        assert!(json.contains("\"target_name\": \"M31\""));
    }
}
