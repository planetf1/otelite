//! SQLite backend implementation

use crate::StorageConfig;
use async_trait::async_trait;
use chrono::Timelike;
use otelite_core::filters::GenAiFilters;
use otelite_core::storage::{
    PurgeAllStats, PurgeOptions, QueryParams, Result, StorageBackend, StorageError, StorageStats,
};
use otelite_core::telemetry::{LogRecord, Metric, Span};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;

pub mod pool;
pub mod purge;
pub mod reader;
pub mod schema;
pub mod writer;

/// Number of dedicated read connections in the pool. Sized so a full
/// dashboard refresh (up to ~10 parallel fetches) runs a few queries in
/// parallel without oversubscribing the host; each connection carries a
/// 256 MB page cache.
const READ_POOL_SIZE: usize = 4;

/// SQLite storage backend
pub struct SqliteBackend {
    config: StorageConfig,
    conn: Arc<Mutex<Option<Connection>>>,
    /// Dedicated read connections (file-backed databases only; in-memory
    /// databases share the primary connection).
    read_pool: std::sync::OnceLock<Arc<pool::ReadPool>>,
    purge_lock: Arc<purge::PurgeLock>,
    purge_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl SqliteBackend {
    /// Create a new SQLite backend with the given configuration
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            conn: Arc::new(Mutex::new(None)),
            read_pool: std::sync::OnceLock::new(),
            purge_lock: Arc::new(purge::PurgeLock::new()),
            purge_handle: Arc::new(Mutex::new(None)),
        }
    }

    fn db_path(&self) -> PathBuf {
        if self
            .config
            .data_dir
            .to_string_lossy()
            .starts_with(":memory:")
        {
            self.config.data_dir.clone()
        } else {
            self.config.data_dir.join("otelite.db")
        }
    }

    /// Run a read-only query on a blocking thread.
    ///
    /// Uses a pooled read connection when available (file-backed DB),
    /// otherwise the primary connection. Either way the synchronous SQLite
    /// work happens off the async runtime, and on a pooled connection the
    /// query no longer serialises behind the writer.
    async fn read_query<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.read_pool.get().cloned();
        let shared = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || match pool {
            Some(pool) => {
                let guard = pool::ReadPool::checkout(&pool)?;
                f(&guard)
            },
            None => {
                let guard = shared.lock();
                let conn = guard.as_ref().ok_or_else(|| {
                    StorageError::QueryError("Database not initialized".to_string())
                })?;
                f(conn)
            },
        })
        .await
        .map_err(|e| StorageError::QueryError(format!("Read query worker failed: {e}")))?
    }
}

impl SqliteBackend {
    /// Open a dedicated connection for purge operations.
    ///
    /// A purge must never reuse the writer connection: a long purge
    /// would hold the single writer lock (stalling every ingest write
    /// behind the parking_lot mutex) while doing batched deletes. A
    /// dedicated connection with the standard busy timeout lets the
    /// purge coexist with live ingest and reads under WAL.
    fn open_purge_connection(db_path: &std::path::Path) -> Result<Connection> {
        let conn = Connection::open(db_path)
            .map_err(|e| StorageError::WriteError(format!("Failed to open purge connection: {e}")))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=10000;",
        )
        .map_err(|e| {
            StorageError::WriteError(format!("Failed to configure purge connection: {e}"))
        })?;
        Ok(conn)
    }

    /// Run `op` on the writer connection inside a single transaction, on
    /// a blocking thread so a slow batch cannot stall the async runtime.
    /// All-or-nothing: any error rolls the whole transaction back.
    async fn write_in_transaction(
        &self,
        op: impl FnOnce(&Connection) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let conn_opt = std::sync::Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut conn_guard = conn_opt.lock();
            let conn = conn_guard
                .as_mut()
                .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

            let tx = conn.transaction().map_err(|e| {
                StorageError::WriteError(format!("Failed to start write transaction: {}", e))
            })?;
            let result = op(&tx);
            match result {
                Ok(()) => tx.commit().map_err(|e| {
                    StorageError::WriteError(format!("Failed to commit write transaction: {}", e))
                }),
                Err(e) => {
                    let _ = tx.rollback();
                    Err(e)
                },
            }
        })
        .await
        .map_err(|e| StorageError::WriteError(format!("Write task failed to join: {}", e)))?
    }
}

#[async_trait]
impl StorageBackend for SqliteBackend {
    async fn initialize(&mut self) -> Result<()> {
        let db_path = self.db_path();

        if !db_path.to_string_lossy().starts_with(":memory:") {
            std::fs::create_dir_all(&self.config.data_dir).map_err(|e| {
                StorageError::InitializationError(format!("Failed to create data directory: {}", e))
            })?;
        }

        let conn = Connection::open(&db_path).map_err(|e| {
            StorageError::InitializationError(format!("Failed to open database: {}", e))
        })?;

        // busy_timeout: the purge scheduler (below) and manual purges open
        // their own writer connections, so this connection can meet an
        // in-flight purge transaction. Without a busy timeout the write
        // would fail instantly with SQLITE_BUSY and the exporter would
        // retry the whole batch, storing duplicates. 10 s matches the
        // read pool's READ_BUSY_TIMEOUT_MS.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=10000;",
        )
        .map_err(|e| {
            StorageError::InitializationError(format!("Failed to configure SQLite: {}", e))
        })?;

        schema::initialize_schema(&conn).map_err(StorageError::from)?;

        *self.conn.lock() = Some(conn);

        if !db_path.to_string_lossy().starts_with(":memory:") {
            self.read_pool
                .set(pool::ReadPool::new(db_path.clone(), READ_POOL_SIZE))
                .ok();
        }

        if self.config.retention_days > 0 {
            self.start_purge_scheduler(db_path);
        }

        Ok(())
    }

    async fn write_log(&self, log: &LogRecord) -> Result<()> {
        let conn_guard = self.conn.lock();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_log(conn, log).map_err(StorageError::from)
    }

    async fn write_span(&self, span: &Span) -> Result<()> {
        let conn_guard = self.conn.lock();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_span(conn, span).map_err(StorageError::from)
    }

    async fn write_metric(&self, metric: &Metric) -> Result<()> {
        let conn_guard = self.conn.lock();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_metric(conn, metric).map_err(StorageError::from)
    }

    async fn write_log_batch(&self, logs: &[LogRecord]) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }
        let logs = logs.to_vec();
        self.write_in_transaction(move |conn| {
            for log in &logs {
                writer::write_log(conn, log)?;
            }
            Ok(())
        })
        .await
    }

    async fn write_span_batch(&self, spans: &[Span]) -> Result<()> {
        if spans.is_empty() {
            return Ok(());
        }
        let spans = spans.to_vec();
        self.write_in_transaction(move |conn| {
            for span in &spans {
                writer::write_span(conn, span)?;
            }
            Ok(())
        })
        .await
    }

    async fn write_metric_batch(&self, metrics: &[Metric]) -> Result<()> {
        if metrics.is_empty() {
            return Ok(());
        }
        let metrics = metrics.to_vec();
        self.write_in_transaction(move |conn| {
            for metric in &metrics {
                writer::write_metric(conn, metric)?;
            }
            Ok(())
        })
        .await
    }

    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>> {
        let params = params.clone();
        self.read_query(move |conn| reader::query_logs(conn, &params).map_err(StorageError::from))
            .await
    }

    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>> {
        let params = params.clone();
        self.read_query(move |conn| reader::query_spans(conn, &params).map_err(StorageError::from))
            .await
    }

    async fn query_spans_for_trace_list(
        &self,
        params: &QueryParams,
        trace_limit: usize,
    ) -> Result<Vec<Span>> {
        let params = params.clone();
        self.read_query(move |conn| {
            reader::query_spans_for_trace_list(conn, &params, trace_limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        let params = params.clone();
        self.read_query(move |conn| {
            reader::query_metrics(conn, &params).map_err(StorageError::from)
        })
        .await
    }

    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        let params = params.clone();
        self.read_query(move |conn| {
            reader::query_latest_metrics(conn, &params).map_err(StorageError::from)
        })
        .await
    }

    async fn query_distinct_metric_names(&self) -> Result<Vec<String>> {
        self.read_query(|conn| {
            reader::query_distinct_metric_names(conn).map_err(StorageError::from)
        })
        .await
    }

    async fn stats(&self) -> Result<StorageStats> {
        self.read_query(|conn| reader::get_stats(conn).map_err(StorageError::from))
            .await
    }

    async fn purge(&self, options: &PurgeOptions) -> Result<u64> {
        let _guard = self
            .purge_lock
            .try_lock()
            .await
            .map_err(StorageError::from)?;

        // Non-blocking check: if a write is in flight the database is
        // initialised by definition. If it is not initialised at all,
        // the purge will fail with a clear "no such table" error from
        // SQLite.
        if let Some(conn_guard) = self.conn.try_lock() {
            if conn_guard.is_none() {
                return Err(StorageError::WriteError("Database not initialized".to_string()));
            }
        }

        let db_path = self.db_path();
        let cutoff_timestamp = if let Some(older_than) = options.older_than {
            older_than
        } else {
            let cutoff =
                chrono::Utc::now() - chrono::Duration::days(self.config.retention_days as i64);
            cutoff.timestamp_nanos_opt().unwrap_or(0)
        };
        let signal_types = options.signal_types.clone();
        let dry_run = options.dry_run;

        // Batched deletes and the checkpoint are synchronous SQLite
        // work: run them on a blocking thread (so a long purge cannot
        // stall the async runtime) on the dedicated purge connection
        // (so ingest writes keep flowing during the purge).
        let result = tokio::task::spawn_blocking(move || {
            let mut conn = Self::open_purge_connection(&db_path)?;
            let record =
                purge::purge_old_data(&mut conn, cutoff_timestamp, 10000, &signal_types, dry_run)?;
            if !dry_run {
                purge::checkpoint(&mut conn)?;
            }
            Ok::<_, StorageError>(record)
        })
        .await
        .map_err(|e| StorageError::WriteError(format!("Purge task failed to join: {e}")))?;

        let record = result?;
        let total_deleted = record.logs_deleted + record.spans_deleted + record.metrics_deleted;
        Ok(total_deleted as u64)
    }

    async fn purge_all(&self) -> Result<PurgeAllStats> {
        let _guard = self
            .purge_lock
            .try_lock()
            .await
            .map_err(StorageError::from)?;

        // Non-blocking check (see purge()); a busy writer means the
        // database is initialised.
        if let Some(conn_guard) = self.conn.try_lock() {
            if conn_guard.is_none() {
                return Err(StorageError::WriteError("Database not initialized".to_string()));
            }
        }

        let db_path = self.db_path();

        let result = tokio::task::spawn_blocking(move || {
            let mut conn = Self::open_purge_connection(&db_path)?;

            let tx = conn
                .transaction()
                .map_err(|e| StorageError::WriteError(format!("Failed to start transaction: {}", e)))?;

            let logs_deleted = tx
                .execute("DELETE FROM logs", [])
                .map_err(|e| StorageError::WriteError(format!("Failed to delete logs: {}", e)))?
                as u64;
            let spans_deleted = tx
                .execute("DELETE FROM spans", [])
                .map_err(|e| StorageError::WriteError(format!("Failed to delete spans: {}", e)))?
                as u64;
            let metrics_deleted = tx
                .execute("DELETE FROM metrics", [])
                .map_err(|e| StorageError::WriteError(format!("Failed to delete metrics: {}", e)))?
                as u64;

            tx.commit().map_err(|e| {
                StorageError::WriteError(format!("Failed to commit transaction: {}", e))
            })?;

            purge::checkpoint(&mut conn)?;

            Ok::<_, StorageError>(PurgeAllStats {
                logs_deleted,
                spans_deleted,
                metrics_deleted,
            })
        })
        .await
        .map_err(|e| StorageError::WriteError(format!("Purge task failed to join: {e}")))?;

        result
    }

    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>> {
        let signal = signal.to_string();
        self.read_query(move |conn| {
            reader::distinct_resource_keys(conn, &signal).map_err(StorageError::from)
        })
        .await
    }

    async fn query_token_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<(
        otelite_core::api::TokenUsageSummary,
        Vec<otelite_core::api::ModelUsage>,
        Vec<otelite_core::api::SystemUsage>,
    )> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_token_usage(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_cost_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::CostSeriesPoint>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_cost_series(conn, start_time, end_time, bucket_ns, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_top_spans(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
        sort_by: otelite_core::api::TopSpanSort,
        truncated_only: bool,
    ) -> Result<Vec<otelite_core::api::TopSpan>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_top_spans(
                conn,
                start_time,
                end_time,
                &filters,
                limit,
                sort_by,
                truncated_only,
            )
            .map_err(StorageError::from)
        })
        .await
    }

    async fn query_top_sessions(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<otelite_core::api::SessionCostRow>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_top_sessions(conn, start_time, end_time, &filters, limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_top_conversations(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<otelite_core::api::ConversationCostRow>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_top_conversations(conn, start_time, end_time, &filters, limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_finish_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::FinishReasonCount>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_finish_reasons(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_latency_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::LatencyStats>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_latency_stats(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_latency_percentiles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        metrics: &[&str],
        timezone: Option<&str>,
    ) -> Result<otelite_core::api::LatencyPercentilesResponse> {
        let filters = filters.clone();
        let metrics: Vec<String> = metrics.iter().map(|m| m.to_string()).collect();
        let timezone = timezone.map(str::to_string);
        self.read_query(move |conn| {
            let refs: Vec<&str> = metrics.iter().map(String::as_str).collect();
            let tz = timezone.as_deref();
            reader::query_latency_percentiles(
                conn,
                start_time,
                end_time,
                bucket_secs,
                &refs,
                &filters,
                tz,
            )
            .map_err(StorageError::from)
        })
        .await
    }

    async fn query_distribution(
        &self,
        metric: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        buckets: usize,
        scale: &str,
    ) -> Result<otelite_core::api::DistributionResponse> {
        let filters = filters.clone();
        let metric = metric.to_string();
        let scale = scale.to_string();
        self.read_query(move |conn| {
            reader::query_distribution(
                conn, &metric, start_time, end_time, buckets, &scale, &filters,
            )
            .map_err(StorageError::from)
        })
        .await
    }

    async fn query_session_context(
        &self,
        session_id: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: u64,
    ) -> Result<Option<otelite_core::api::SessionContextResponse>> {
        let session_id = session_id.to_string();
        self.read_query(move |conn| {
            reader::query_session_context(conn, &session_id, start_time, end_time, limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_genai_capabilities(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<otelite_core::api::GenAiCapabilityResponse> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_genai_capabilities(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_error_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::ErrorRateByModel>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_error_rate(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_tool_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<otelite_core::api::ToolUsage>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_tool_usage(conn, start_time, end_time, &filters, limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_retry_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<otelite_core::api::RetryStats> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_retry_stats(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_retrieval_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        top_queries_limit: usize,
    ) -> Result<otelite_core::api::RetrievalStats> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_retrieval_stats(conn, start_time, end_time, &filters, top_queries_limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_truncation_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::TruncationRateByModel>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_truncation_rate(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_cache_hit_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::CacheHitRateByModel>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_cache_hit_rate(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_agent_roles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<otelite_core::api::AgentRolesResponse> {
        self.read_query(move |conn| {
            reader::query_agent_roles(conn, start_time, end_time).map_err(StorageError::from)
        })
        .await
    }

    async fn query_provider_mix(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<otelite_core::api::ProviderMixResponse> {
        self.read_query(move |conn| {
            reader::query_provider_mix(conn, start_time, end_time).map_err(StorageError::from)
        })
        .await
    }

    async fn query_cache_economics(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
    ) -> Result<otelite_core::api::CacheEconomicsResponse> {
        self.read_query(move |conn| {
            reader::query_cache_economics(conn, start_time, end_time, bucket_ns)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_reasoning_share(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<otelite_core::api::ReasoningShareResponse> {
        self.read_query(move |conn| {
            reader::query_reasoning_share(conn, start_time, end_time).map_err(StorageError::from)
        })
        .await
    }

    async fn query_agent_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
    ) -> Result<Vec<otelite_core::api::AgentRollupStorage>> {
        self.read_query(move |conn| {
            reader::query_agent_rollup(conn, start_time, end_time, bucket_secs)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_project_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<otelite_core::api::ProjectRollupStorage>> {
        self.read_query(move |conn| {
            reader::query_project_rollup(conn, start_time, end_time).map_err(StorageError::from)
        })
        .await
    }

    async fn query_session_costs(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<otelite_core::api::SessionCostStorage>> {
        self.read_query(move |conn| {
            reader::query_session_costs(conn, start_time, end_time).map_err(StorageError::from)
        })
        .await
    }

    async fn query_request_param_profile(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<otelite_core::api::RequestParamProfile> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_request_param_profile(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_conversation_depth(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<otelite_core::api::ConversationDepthStats> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_conversation_depth(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_latency_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
        filters: &GenAiFilters,
        all_spans: bool,
        timezone: Option<&str>,
    ) -> Result<Vec<otelite_core::api::LatencySeriesPoint>> {
        let filters = filters.clone();
        let tz = timezone.map(str::to_string);
        self.read_query(move |conn| {
            reader::query_latency_series(
                conn,
                start_time,
                end_time,
                bucket_secs,
                &filters,
                all_spans,
                tz.as_deref(),
            )
            .map_err(StorageError::from)
        })
        .await
    }

    async fn query_calls_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        all_spans: bool,
    ) -> Result<Vec<otelite_core::api::CallsSeriesPoint>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_calls_series(conn, start_time, end_time, &filters, bucket_secs, all_spans)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_latency_by_context(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::LatencyByContextBin>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_latency_by_context(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_error_types(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::ErrorTypeBreakdown>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_error_types(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_model_drift(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::ModelDriftPair>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_model_drift(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_tool_approvals(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<otelite_core::api::ToolApprovalStats> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_tool_approvals(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_stop_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::StopReasonCount>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_stop_reasons(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_context_type_split(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::ContextTypeSplit>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_context_type_split(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_tool_errors(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<otelite_core::api::ToolErrorEntry>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_tool_errors(conn, start_time, end_time, &filters, limit)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn query_hour_of_day(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<otelite_core::api::HourOfDayBucket>> {
        let filters = filters.clone();
        self.read_query(move |conn| {
            reader::query_hour_of_day(conn, start_time, end_time, &filters)
                .map_err(StorageError::from)
        })
        .await
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(handle) = self.purge_handle.lock().take() {
            handle.abort();
        }

        if let Some(pool) = self.read_pool.take() {
            pool.close_all();
        }

        let mut conn_guard = self.conn.lock();
        if let Some(conn) = conn_guard.take() {
            conn.close()
                .map_err(|(_, e)| StorageError::DatabaseError(e.to_string()))?;
        }
        Ok(())
    }
}

impl SqliteBackend {
    fn start_purge_scheduler(&self, db_path: PathBuf) {
        let config = self.config.clone();
        let purge_lock = self.purge_lock.clone();

        let handle = tokio::spawn(async move {
            // Dedicated connection: purge never competes with the main conn mutex.
            let mut conn = match Connection::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Purge scheduler: failed to open DB connection: {}", e);
                    return;
                },
            };
            if let Err(e) = conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=10000;",
            ) {
                tracing::warn!("Purge scheduler: failed to set WAL mode: {}", e);
            }

            loop {
                let now = chrono::Local::now();
                let next_purge = if now.hour() < 2 {
                    now.date_naive().and_hms_opt(2, 0, 0).unwrap()
                } else {
                    (now.date_naive() + chrono::Duration::days(1))
                        .and_hms_opt(2, 0, 0)
                        .unwrap()
                };
                let next_purge =
                    chrono::TimeZone::from_local_datetime(&chrono::Local, &next_purge).unwrap();
                let duration = (next_purge - now)
                    .to_std()
                    .unwrap_or(std::time::Duration::from_secs(86400));

                tokio::time::sleep(duration).await;

                if let Ok(_guard) = purge_lock.try_lock().await {
                    let cutoff =
                        chrono::Utc::now() - chrono::Duration::days(config.retention_days as i64);
                    let cutoff_timestamp = cutoff.timestamp_nanos_opt().unwrap_or(0);

                    if let Ok(record) = purge::purge_old_data(
                        &mut conn,
                        cutoff_timestamp,
                        10000,
                        &[
                            crate::SignalType::Logs,
                            crate::SignalType::Traces,
                            crate::SignalType::Metrics,
                        ],
                        false,
                    ) {
                        tracing::info!(
                            "Automatic purge completed: {} logs, {} spans, {} metrics deleted",
                            record.logs_deleted,
                            record.spans_deleted,
                            record.metrics_deleted
                        );
                        // VACUUM requires exclusive access and cannot run while the main
                        // connection is open. Run a passive checkpoint instead to keep
                        // the WAL from growing unboundedly after bulk deletes.
                        if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
                            tracing::warn!("Purge scheduler: WAL checkpoint failed: {}", e);
                        }
                    }
                }
            }
        });

        *self.purge_handle.lock() = Some(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_sqlite_backend_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let backend = SqliteBackend::new(config);
        assert!(backend.conn.lock().is_none());
    }

    #[tokio::test]
    async fn test_sqlite_backend_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let mut backend = SqliteBackend::new(config);
        let result = backend.initialize().await;
        assert!(result.is_ok());
        assert!(backend.conn.lock().is_some());
    }

    #[tokio::test]
    async fn test_stats_returns_counts() {
        use otelite_core::telemetry::log::SeverityLevel;
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let log = LogRecord {
            timestamp: 1000,
            observed_timestamp: Some(1000),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "test log".to_string(),
            trace_id: None,
            span_id: None,
            attributes: HashMap::new(),
            resource: None,
        };
        backend.write_log(&log).await.unwrap();

        let stats = backend.stats().await.unwrap();

        assert_eq!(stats.log_count, 1);
        assert_eq!(stats.span_count, 0);
        assert_eq!(stats.metric_count, 0);
        assert!(stats.storage_size_bytes > 0);
    }

    /// Regression test: a write must wait out a concurrent write lock
    /// (held by the purge path's own connection) instead of failing
    /// instantly with SQLITE_BUSY. Before the fix, the writer connection
    /// had no busy_timeout, so any overlap with a purge made the batch
    /// fail and the exporter retry it — storing duplicates.
    #[tokio::test]
    async fn test_write_waits_for_concurrent_write_lock() {
        use otelite_core::telemetry::log::SeverityLevel;
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let db_path = temp_dir.path().join("otelite.db");
        let lock_held = Arc::new(AtomicBool::new(false));

        // A second writer (as the purge path opens) grabs the WAL write
        // lock, holds it for 500 ms, then releases it. Not awaited until
        // after the write, so the lock is held while the write runs.
        let holder = {
            let lock_held = Arc::clone(&lock_held);
            let db_path = db_path.clone();
            tokio::task::spawn_blocking(move || {
                let conn = Connection::open(&db_path).unwrap();
                // BEGIN IMMEDIATE takes the WAL write lock at start; the
                // dummy write makes it stick. Commit after 500 ms.
                conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS lock_probe (x INTEGER); INSERT INTO lock_probe VALUES (1);",
                    [],
                )
                .unwrap();
                lock_held.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(500));
                conn.execute_batch("COMMIT;").unwrap();
            })
        };

        // Wait until the write lock is actually held.
        while !lock_held.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let log = LogRecord {
            timestamp: 2000,
            observed_timestamp: Some(2000),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "written while a purge lock is held".to_string(),
            trace_id: None,
            span_id: None,
            attributes: HashMap::new(),
            resource: None,
        };

        let started = Instant::now();
        let result = backend.write_log(&log).await;
        let waited = started.elapsed();

        result.expect("write must wait out the concurrent lock, not fail with SQLITE_BUSY");
        assert!(
            waited >= Duration::from_millis(300),
            "write should have blocked on the lock (waited {waited:?})"
        );

        holder.await.unwrap();

        let count: i64 = {
            let conn = Connection::open(&db_path).unwrap();
            conn.query_row("SELECT COUNT(*) FROM logs", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count, 1, "the log must be stored exactly once");
    }

    fn test_span(id: u32) -> Span {
        use otelite_core::telemetry::trace::{SpanKind, SpanStatus, StatusCode};
        Span {
            trace_id: format!("trace{id:032x}"),
            span_id: format!("{id:016x}"),
            parent_span_id: None,
            name: format!("batch-span-{id}"),
            kind: SpanKind::Internal,
            start_time: 1_000_000 + id as i64,
            end_time: 2_000_000 + id as i64,
            attributes: std::collections::HashMap::new(),
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: None,
        }
    }

    #[tokio::test]
    async fn test_write_span_batch_stores_all() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let batch: Vec<Span> = (0..5).map(test_span).collect();
        backend.write_span_batch(&batch).await.unwrap();

        let stats = backend.stats().await.unwrap();
        assert_eq!(stats.span_count, 5);

        // Empty batch is a no-op, not an error.
        backend.write_span_batch(&[]).await.unwrap();
    }

    /// Regression test: a failure partway through a batch must roll back
    /// the whole batch. Before the fix, each span committed individually,
    /// so a mid-batch error left a partial write behind and the
    /// exporter's retry duplicated it.
    #[tokio::test]
    async fn test_write_span_batch_is_atomic() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        // Force the third insert in the batch to fail, via a trigger on
        // the writer connection.
        {
            let conn_guard = backend.conn.lock();
            let conn = conn_guard
                .as_ref()
                .expect("initialised backend has a connection");
            conn.execute(
                "CREATE TRIGGER fail_third_insert BEFORE INSERT ON spans
                 WHEN (SELECT COUNT(*) FROM spans) = 2
                 BEGIN SELECT RAISE(ABORT, 'forced mid-batch failure'); END",
                [],
            )
            .unwrap();
        }

        let batch: Vec<Span> = (0..3).map(test_span).collect();
        let err = backend
            .write_span_batch(&batch)
            .await
            .expect_err("the forced third insert must fail the batch");
        assert!(
            err.to_string().contains("forced mid-batch failure"),
            "unexpected error: {err}"
        );

        // All-or-nothing: the two inserts before the failure are gone.
        let count: i64 = {
            let conn_guard = backend.conn.lock();
            let conn = conn_guard
                .as_ref()
                .expect("initialised backend has a connection");
            conn.query_row("SELECT COUNT(*) FROM spans", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            count, 0,
            "a failed batch must leave no partial write behind"
        );
    }

    fn test_log(ts: i64) -> LogRecord {
        use otelite_core::telemetry::log::SeverityLevel;
        LogRecord {
            timestamp: ts,
            observed_timestamp: Some(ts),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "purge test log".to_string(),
            trace_id: None,
            span_id: None,
            attributes: std::collections::HashMap::new(),
            resource: None,
        }
    }

    /// Regression test: a purge must run on its own connection. While
    /// the writer connection is held by an in-flight write, a dry-run
    /// purge must still complete immediately — before the fix, purge()
    /// locked the writer connection for its whole duration, stalling
    /// every ingest write (and, for a real purge, every read too)
    /// until it finished.
    #[tokio::test]
    async fn test_purge_does_not_hold_writer_lock() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();
        backend.write_log(&test_log(1000)).await.unwrap();

        // Hold the writer connection for 300 ms, as a long write would.
        let conn_opt = std::sync::Arc::clone(&backend.conn);
        let holder = tokio::task::spawn_blocking(move || {
            let _guard = conn_opt.lock();
            std::thread::sleep(std::time::Duration::from_millis(300));
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let options = otelite_core::storage::PurgeOptions {
            older_than: Some(2000),
            signal_types: vec![crate::SignalType::Logs],
            dry_run: true,
        };
        let started = std::time::Instant::now();
        let purged = backend
            .purge(&options)
            .await
            .expect("dry-run purge must not need the writer connection");
        let took = started.elapsed();

        assert_eq!(purged, 1, "dry run must report the matching row");
        assert!(
            took < std::time::Duration::from_millis(150),
            "purge waited {took:?} on the writer lock; it must use its own connection"
        );

        holder.await.unwrap();
    }

    /// Regression test: a real purge must succeed while the read pool
    /// holds open connections. Before the fix the purge finished with
    /// VACUUM, which needs exclusive file access and failed with
    /// SQLITE_BUSY whenever a pooled read connection was still open —
    /// i.e. in every real deployment after the first dashboard query.
    #[tokio::test]
    async fn test_purge_succeeds_with_active_read_pool() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        backend.write_log(&test_log(1000)).await.unwrap();
        backend.write_log(&test_log(5_000)).await.unwrap();

        // Keep a pooled read connection open (a dashboard query would).
        let params = QueryParams {
            start_time: None,
            end_time: None,
            limit: None,
            trace_id: None,
            span_id: None,
            min_severity: None,
            search_text: None,
            predicates: Vec::new(),
        };
        let _ = backend.query_logs(&params).await.expect("read must work");

        let options = otelite_core::storage::PurgeOptions {
            older_than: Some(2000),
            signal_types: vec![crate::SignalType::Logs],
            dry_run: false,
        };
        let purged = backend
            .purge(&options)
            .await
            .expect("purge must succeed while the read pool holds connections");
        assert_eq!(purged, 1);

        let stats = backend.stats().await.unwrap();
        assert_eq!(stats.log_count, 1, "only the new log may remain");
    }
}
