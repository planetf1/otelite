#!/usr/bin/env bash
# Inject test traces, logs, and metrics into Otelite via OTLP HTTP
# Usage: ./scripts/inject_test_data.sh [http://localhost:4318]

set -euo pipefail

ENDPOINT="${1:-http://localhost:4318}"
NOW_NS=$(python3 -c "import time; print(int(time.time() * 1e9))")
TRACE_ID="$(openssl rand -hex 16)"
SPAN_A="$(openssl rand -hex 8)"
SPAN_B="$(openssl rand -hex 8)"
SPAN_C="$(openssl rand -hex 8)"

echo "Injecting test data → $ENDPOINT"
echo "  trace_id: $TRACE_ID"

# ── Traces ────────────────────────────────────────────────────────────────────
curl -s -o /dev/null -w "traces: %{http_code}\n" \
  -X POST "$ENDPOINT/v1/traces" \
  -H "Content-Type: application/json" \
  -d "{
    \"resourceSpans\": [{
      \"resource\": {
        \"attributes\": [
          {\"key\": \"service.name\", \"value\": {\"stringValue\": \"test-service\"}},
          {\"key\": \"service.version\", \"value\": {\"stringValue\": \"1.0.0\"}}
        ]
      },
      \"scopeSpans\": [{
        \"scope\": {\"name\": \"test-instrumentation\"},
        \"spans\": [
          {
            \"traceId\": \"$TRACE_ID\",
            \"spanId\": \"$SPAN_A\",
            \"name\": \"HTTP GET /api/users\",
            \"kind\": 2,
            \"startTimeUnixNano\": \"$((NOW_NS - 150000000))\",
            \"endTimeUnixNano\": \"$NOW_NS\",
            \"status\": {\"code\": 1},
            \"attributes\": [
              {\"key\": \"http.method\", \"value\": {\"stringValue\": \"GET\"}},
              {\"key\": \"http.url\", \"value\": {\"stringValue\": \"/api/users\"}},
              {\"key\": \"http.status_code\", \"value\": {\"intValue\": 200}}
            ]
          },
          {
            \"traceId\": \"$TRACE_ID\",
            \"spanId\": \"$SPAN_B\",
            \"parentSpanId\": \"$SPAN_A\",
            \"name\": \"db.query users\",
            \"kind\": 3,
            \"startTimeUnixNano\": \"$((NOW_NS - 80000000))\",
            \"endTimeUnixNano\": \"$((NOW_NS - 10000000))\",
            \"status\": {\"code\": 1},
            \"attributes\": [
              {\"key\": \"db.system\", \"value\": {\"stringValue\": \"sqlite\"}},
              {\"key\": \"db.statement\", \"value\": {\"stringValue\": \"SELECT * FROM users LIMIT 20\"}}
            ]
          },
          {
            \"traceId\": \"$TRACE_ID\",
            \"spanId\": \"$SPAN_C\",
            \"parentSpanId\": \"$SPAN_A\",
            \"name\": \"cache.get user_list\",
            \"kind\": 3,
            \"startTimeUnixNano\": \"$((NOW_NS - 140000000))\",
            \"endTimeUnixNano\": \"$((NOW_NS - 130000000))\",
            \"status\": {\"code\": 1},
            \"attributes\": [
              {\"key\": \"cache.hit\", \"value\": {\"boolValue\": false}},
              {\"key\": \"cache.key\", \"value\": {\"stringValue\": \"user_list\"}}
            ]
          }
        ]
      }]
    }]
  }"

# ── Logs ──────────────────────────────────────────────────────────────────────
for severity in TRACE DEBUG INFO INFO INFO WARN ERROR; do
  MSG="Test $severity log from inject_test_data.sh"
  SEV_NUM=5
  case $severity in
    TRACE) SEV_NUM=1 ;;
    DEBUG) SEV_NUM=5 ;;
    INFO)  SEV_NUM=9 ;;
    WARN)  SEV_NUM=13 ;;
    ERROR) SEV_NUM=17 ;;
  esac
  TS=$(python3 -c "import time; print(int(time.time() * 1e9))")
  curl -s -o /dev/null -X POST "$ENDPOINT/v1/logs" \
    -H "Content-Type: application/json" \
    -d "{
      \"resourceLogs\": [{
        \"resource\": {\"attributes\": [
          {\"key\": \"service.name\", \"value\": {\"stringValue\": \"test-service\"}}
        ]},
        \"scopeLogs\": [{
          \"scope\": {\"name\": \"test\"},
          \"logRecords\": [{
            \"timeUnixNano\": \"$TS\",
            \"severityNumber\": $SEV_NUM,
            \"severityText\": \"$severity\",
            \"body\": {\"stringValue\": \"$MSG\"},
            \"attributes\": [
              {\"key\": \"component\", \"value\": {\"stringValue\": \"inject-script\"}},
              {\"key\": \"env\", \"value\": {\"stringValue\": \"local\"}}
            ]
          }]
        }]
      }]
    }"
done
echo "logs: 7 records injected"

# ── Metrics ───────────────────────────────────────────────────────────────────
TS=$(python3 -c "import time; print(int(time.time() * 1e9))")
curl -s -o /dev/null -w "metrics: %{http_code}\n" \
  -X POST "$ENDPOINT/v1/metrics" \
  -H "Content-Type: application/json" \
  -d "{
    \"resourceMetrics\": [{
      \"resource\": {\"attributes\": [
        {\"key\": \"service.name\", \"value\": {\"stringValue\": \"test-service\"}}
      ]},
      \"scopeMetrics\": [{
        \"scope\": {\"name\": \"test\"},
        \"metrics\": [
          {
            \"name\": \"http.requests.total\",
            \"description\": \"Total HTTP requests\",
            \"unit\": \"requests\",
            \"sum\": {
              \"dataPoints\": [{
                \"startTimeUnixNano\": \"$((TS - 60000000000))\",
                \"timeUnixNano\": \"$TS\",
                \"asDouble\": 1234,
                \"attributes\": [{\"key\": \"method\", \"value\": {\"stringValue\": \"GET\"}}]
              }],
              \"aggregationTemporality\": 2,
              \"isMonotonic\": true
            }
          },
          {
            \"name\": \"http.request.duration\",
            \"description\": \"HTTP request duration\",
            \"unit\": \"ms\",
            \"histogram\": {
              \"dataPoints\": [{
                \"startTimeUnixNano\": \"$((TS - 60000000000))\",
                \"timeUnixNano\": \"$TS\",
                \"count\": 150,
                \"sum\": 18750.0,
                \"bucketCounts\": [10, 40, 70, 25, 5],
                \"explicitBounds\": [10, 50, 100, 500]
              }],
              \"aggregationTemporality\": 2
            }
          },
          {
            \"name\": \"memory.usage\",
            \"description\": \"Memory usage in bytes\",
            \"unit\": \"bytes\",
            \"gauge\": {
              \"dataPoints\": [{
                \"timeUnixNano\": \"$TS\",
                \"asDouble\": 52428800
              }]
            }
          }
        ]
      }]
    }]
  }"

echo ""
echo "Done. Refresh the dashboard to see the data."
