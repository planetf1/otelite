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
        // One atomic transaction for the whole export: a failure rolls
        // back every record, so the exporter's retry of the rejected
        // export cannot duplicate the records that already committed.
        if !records.is_empty() {
            self.storage.write_log_batch(&records).await?;
        }

        info!("Stored {} logs", log_count);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_logs_handler_process() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut storage = SqliteBackend::new(config);
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
