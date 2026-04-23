//! Logs signal handler

use crate::{conversion, Result};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for logs signals
#[derive(Clone)]
pub struct LogsHandler {
    storage: Arc<dyn StorageBackend>,
}

impl LogsHandler {
    /// Create a new logs handler
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
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

        let records = conversion::convert_logs(request);
        for record in records {
            self.storage.write_log(&record).await?;
        }

        info!("Stored {} logs", log_count);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

    #[tokio::test]
    async fn test_logs_handler_process() {
        let mut storage = SqliteBackend::new(StorageConfig::default());
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = LogsHandler::new(Arc::new(storage));
        let request = ExportLogsServiceRequest {
            resource_logs: vec![],
        };
        assert!(handler.process(request).await.is_ok());
    }
}
