#!/usr/bin/env bash
set -euo pipefail
out=tools/temp_fix53_scan.txt
{
  echo '=== frontend operation notifier call sites ==='
  git grep -n -E 'Frontend(Tune|Scan)Notifier|accept_operation_event|deliver_committed_(tune|scan)_notification' -- tuner_hal2 || true
  echo '=== frontend callback owner mappings ==='
  git grep -n -E 'frontend.*object|FrontendEvent|frontend_event\(' -- tuner_hal2/service_runtime tuner_hal2/aidl_service | head -n 300 || true
  echo '=== worker terminal raw matches ==='
  git grep -n -E 'WorkerTerminalResult|PanicOrJoinFailure|RuntimeFailure|join\(' -- tuner_hal2/service_runtime tuner_hal2/aidl_service | head -n 500 || true
  echo '=== reaper supervisors/state ==='
  git grep -n -E 'Reaper|reaping|reaper|pending.*worker|SyncSender|sync_channel' -- tuner_hal2/service_runtime/src/worker_runtime.rs tuner_hal2/service_runtime/src/frontend_worker_txn.rs tuner_hal2/aidl_service/src/dvr_callback_delivery.rs | head -n 600 || true
  echo '=== queue token/drain uses ==='
  git grep -n -E 'begin_dvr_(read|write|drain)|QueueEpoch(Token|DrainTxn)|\.commit\(\)|commit_dvr_drain_with_queue_clear' -- tuner_hal2/demux/src | head -n 500 || true
  echo '=== child open and finish paths ==='
  git grep -n -E 'open_(filter|dvr)_child_runtime|finish_(filter|dvr)_child_open|rollback_(filter|dvr)_child_open|register_aidl_object_for_runtime_auto_generation' -- tuner_hal2/service_runtime tuner_hal2/aidl_service | head -n 500 || true
  echo '=== runtime object table reads/lifecycle ==='
  git grep -n -E 'RuntimeObjectLifecycle::|\.is_live\(\)|entry_checked\(' -- tuner_hal2/service_runtime/src | head -n 500 || true
} > "$out"
