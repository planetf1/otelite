//! Logs signal handler

use crate::Result;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use tracing::{debug, info};

/// Handler for logs signals
#[derive(Debug, Clone)]
pub struct LogsHandler {
    // Future: will contain storage backend reference
}

impl LogsHandler {
    /// Create a new logs handler
    pub fn new() -> Self {
        Self {}
    }

    /// Process logs data from OTLP request
    pub async fn process(&self, request: ExportLogsServiceRequest) -> Result<()> {
        let log_count: usize = request
            .resource_logs
            .iter()
            .map(|rl| {
                rl.scope_logs
                    .iter()
                    .map(|sl| sl.log_records.len())
                    .sum::<usize>()
            })
            .sum();

        debug!(
            "Processing {} logs from {} resource logs",
            log_count,
            request.resource_logs.len()
        );

        // TODO: Convert OTLP logs to internal format
        // TODO: Store logs in backend
        // For now, just log receipt

        info!("Received {} logs", log_count);
        Ok(())
    }

    /// Process raw bytes (for HTTP/protobuf)
    pub fn process_bytes(&self, data: &[u8]) -> Result<()> {
        debug!("Processing logs data: {} bytes", data.len());

        // TODO: Parse OTLP logs from protobuf/JSON
        // TODO: Store logs in backend
        // For now, just log receipt

        info!("Received logs: {} bytes", data.len());
        Ok(())
    }
}

impl Default for LogsHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl super::SignalHandler for LogsHandler {
    fn process(&self, data: &[u8]) -> Result<()> {
        self.process_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logs_handler_creation() {
        let handler = LogsHandler::new();
        assert!(handler.process_bytes(&[1, 2, 3]).is_ok());
    }

    #[tokio::test]
    async fn test_logs_handler_process() {
        let handler = LogsHandler::new();
        let request = ExportLogsServiceRequest {
            resource_logs: vec![],
        };
        assert!(handler.process(request).await.is_ok());
    }
}

// Made with Bob
