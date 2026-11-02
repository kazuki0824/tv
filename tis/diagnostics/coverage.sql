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
), native_logs AS (
  SELECT upid,
    SUBSTR(msg, 1, INSTR(msg, ' ') - 1) AS stage,
    SUBSTR(msg, INSTR(msg, ' native=') + 8) AS native_tail
  FROM android_logs LEFT JOIN thread USING (utid), markers
  WHERE tag = 'MediaSync' AND ts BETWEEN begin_ns AND trigger_ns
    AND INSTR(msg, ' native=') > 0
), identified AS (
  SELECT upid, stage,
    SUBSTR(native_tail, 1, INSTR(native_tail || ' ', ' ') - 1) AS native_id
  FROM native_logs
), instances AS (
  SELECT upid, native_id,
    COUNT(CASE WHEN stage = 'PLAYBACK_SYNC_CREATE' THEN 1 END) AS created,
    COUNT(CASE WHEN stage = 'PLAYBACK_SYNC_INPUT' THEN 1 END) AS inputs,
    COUNT(CASE WHEN stage = 'PLAYBACK_SYNC_SCHEDULE' THEN 1 END) AS scheduled,
    COUNT(CASE WHEN stage = 'PLAYBACK_SYNC_OUTPUT' THEN 1 END) AS outputs
  FROM identified
  WHERE upid IS NOT NULL AND native_id GLOB '0x[0-9a-fA-F]*'
    AND SUBSTR(native_id, 3) NOT GLOB '*[^0-9a-fA-F]*' AND native_id != '0x0'
  GROUP BY upid, native_id
), native AS (
  -- 同じpointerが再生成された区間も同一寿命とは認定しない。
  SELECT COUNT(*) AS complete_native_instances FROM instances
  WHERE created = 1 AND inputs > 0 AND scheduled > 0 AND outputs > 0
), interval AS (
  SELECT CASE WHEN begin_ns IS NOT NULL AND trigger_ns > begin_ns
    AND start_ts <= begin_ns AND end_ts >= trigger_ns AND loss_count = 0
    THEN 1 ELSE 0 END AS trace_coverage_ok,
    begin_ns, trigger_ns, start_ts, end_ts, loss_count
  FROM markers, trace_bounds, losses
)
SELECT
  -- 現行ログはTIS session/generationとnative pointerの対応を提供しない。
  -- 同一nativeの4段階があっても対象再生を追跡できたという成功にはしない。
  0 AS coverage_ok,
  trace_coverage_ok,
  CASE WHEN complete_native_instances > 0 THEN 1 ELSE 0 END AS native_instance_coverage_ok,
  0 AS target_playback_correlated,
  'target-session-generation-unverified' AS correlation_status,
  begin_ns, trigger_ns, start_ts, end_ts, loss_count, complete_native_instances
FROM interval, native;
