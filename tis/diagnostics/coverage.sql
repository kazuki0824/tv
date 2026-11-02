-- 採取開始からトーストまでの区間と欠落を検証する。原因特定や実presentの保証ではない。
WITH markers AS (
  SELECT
    MIN(CASE WHEN msg = 'capture-begin:CAPTURE_KEY' THEN ts END) AS begin_ns,
    MAX(CASE WHEN msg = 'capture-trigger:CAPTURE_KEY' THEN ts END) AS trigger_ns
  FROM android_logs WHERE tag = 'MaleicacidCapture'
), losses AS (
  SELECT COUNT(*) AS loss_count FROM stats WHERE value != 0 AND (
    severity = 'data_loss' OR
    name IN ('traced_buf_chunks_overwritten', 'traced_buf_chunks_discarded',
             'ftrace_cpu_overrun_end', 'ftrace_cpu_commit_overrun_end')
  )
), native AS (
  SELECT
    COUNT(CASE WHEN msg GLOB 'PLAYBACK_SYNC_CREATE native=*' THEN 1 END) AS created,
    COUNT(CASE WHEN msg GLOB 'PLAYBACK_SYNC_INPUT *' THEN 1 END) AS inputs,
    COUNT(CASE WHEN msg GLOB 'PLAYBACK_SYNC_SCHEDULE *' THEN 1 END) AS scheduled,
    COUNT(CASE WHEN msg GLOB 'PLAYBACK_SYNC_OUTPUT *' THEN 1 END) AS outputs
  FROM android_logs, markers
  WHERE tag = 'MediaSync' AND ts BETWEEN begin_ns AND trigger_ns
)
SELECT
  CASE WHEN begin_ns IS NOT NULL AND trigger_ns > begin_ns
    AND start_ts <= begin_ns AND end_ts >= trigger_ns AND loss_count = 0
    AND created > 0 AND inputs > 0 AND scheduled > 0 AND outputs > 0
    THEN 1 ELSE 0 END AS coverage_ok,
  begin_ns, trigger_ns, start_ts, end_ts, loss_count, created, inputs, scheduled, outputs
FROM markers, trace_bounds, losses, native;
