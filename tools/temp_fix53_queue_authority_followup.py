from pathlib import Path
import re

ROOT = Path("tuner_hal2")
QUEUE = ROOT / "demux/src/runtime/queue_runtime.rs"


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, got {count}: {old[:120]!r}")
    path.write_text(text.replace(old, new, 1))


# 8/59 + 9/59: destructor is only an infallible fail-closed backstop.
replace_once(
    QUEUE,
    '''#[derive(Debug)]
pub(crate) struct QueueEpochToken {
''',
    '''impl QueueEpochProtocol {
    fn fail_close_unconsumed_authority(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.state = QueueEpochState::Closed;
        self.drained.notify_all();
    }
}

#[derive(Debug)]
pub(crate) struct QueueEpochToken {
''',
)
replace_once(
    QUEUE,
    '''impl Drop for QueueEpochToken {
    fn drop(&mut self) {
        if self.active && self.release().is_err() {
            if let Ok(mut state) = self.protocol.state.lock() {
                state.state = QueueEpochState::Closed;
                self.protocol.drained.notify_all();
            }
        }
    }
}
''',
    '''impl Drop for QueueEpochToken {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.protocol.fail_close_unconsumed_authority();
        }
    }
}
''',
)
replace_once(
    QUEUE,
    '''impl Drop for QueueEpochDrainTxn {
    fn drop(&mut self) {
        if self.active && self.rollback().is_err() {
            if let Ok(mut state) = self.protocol.state.lock() {
                state.state = QueueEpochState::Closed;
                self.protocol.drained.notify_all();
            }
        }
    }
}
''',
    '''impl Drop for QueueEpochDrainTxn {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            self.protocol.fail_close_unconsumed_authority();
        }
    }
}
''',
)

# S-11: every listed prepared/one-shot authority is compiler-visible as must-use.
authorities = [
    "PreparedAvSyncRegistryMutation",
    "PreparedPcrInvalidation",
    "PreparedDvrFilterRelation",
    "FilterQueueCleanupPlan",
    "DvrQueueCleanupPlan",
    "CommittedFilterQueueCleanup",
    "CommittedDvrQueueCleanup",
    "PreparedCallbackRegistration",
    "PreparedLnbControlTxn",
    "CompletedLnbControlTxn",
    "PreparedFrontendLnbAssignment",
    "ExecutedFrontendLnbAssignment",
    "PreparedLnbDiseqc",
    "ExecutedLnbDiseqc",
    "PreparedLnbLifecycleClose",
    "ExecutedLnbLifecycleClose",
    "ObjectMethodExecutionToken",
    "QueueEpochToken",
    "QueueEpochDrainTxn",
]

rust_files = list(ROOT.rglob("*.rs"))
locations = {}
for name in authorities:
    matches = []
    declaration_re = re.compile(rf"^(?P<indent>\s*)(?P<vis>pub(?:\([^\n)]*\))?\s+)?(?P<kind>struct|enum)\s+{re.escape(name)}\b", re.M)
    for path in rust_files:
        text = path.read_text()
        for match in declaration_re.finditer(text):
            matches.append((path, match.start(), match.group(0)))
    if len(matches) != 1:
        raise SystemExit(f"{name}: expected exactly one declaration, got {len(matches)}: {[str(m[0]) for m in matches]}")
    locations[name] = matches[0][0]

by_path = {}
for name, path in locations.items():
    by_path.setdefault(path, []).append(name)

message = "this prepared/one-shot authority must be consumed by its typed completion entry"
for path, names in by_path.items():
    text = path.read_text()
    for name in names:
        decl_re = re.compile(rf"^(?P<indent>\s*)(?P<decl>(?:pub(?:\([^\n)]*\))?\s+)?(?:struct|enum)\s+{re.escape(name)}\b)", re.M)
        match = decl_re.search(text)
        if not match:
            raise SystemExit(f"{path}: declaration disappeared for {name}")
        prefix_start = max(0, text.rfind("\n", 0, match.start()) + 1)
        preceding = text[max(0, prefix_start - 600):prefix_start]
        # Attribute blocks immediately preceding a declaration are bounded by the previous non-attribute/item line.
        recent_lines = preceding.splitlines()
        attr_lines = []
        for line in reversed(recent_lines):
            stripped = line.strip()
            if stripped.startswith("#[") or stripped.startswith("///") or stripped.startswith("//!") or stripped == "":
                attr_lines.append(stripped)
                continue
            break
        if any(line.startswith("#[must_use") for line in attr_lines):
            continue
        indent = match.group("indent")
        text = text[:match.start()] + f'{indent}#[must_use = "{message}"]\n' + text[match.start():]
    path.write_text(text)

# Queue-specific behavior tests: dropping unconsumed authority must only fail-close;
# explicit abort remains the typed rollback path.
queue_text = QUEUE.read_text()
if "fn dropping_unconsumed_queue_epoch_authority_fail_closes_without_rollback()" not in queue_text:
    queue_text += r'''

#[cfg(test)]
mod queue_epoch_authority_drop_contract_tests {
    use super::*;

    fn protocol(state: QueueEpochState, epoch: u64, admitted: usize) -> Arc<QueueEpochProtocol> {
        Arc::new(QueueEpochProtocol {
            state: Mutex::new(QueueEpochProtocolState {
                state,
                epoch,
                admitted_transaction_count: admitted,
            }),
            drained: Condvar::new(),
            queue_identity: Some(77),
        })
    }

    #[test]
    fn dropping_unconsumed_queue_epoch_authority_fail_closes_without_rollback() {
        let protocol = protocol(QueueEpochState::Open, 4, 1);
        let token = QueueEpochToken {
            protocol: Arc::clone(&protocol),
            queue_identity: Some(77),
            epoch: 4,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        };

        drop(token);

        let state = protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Closed);
        assert_eq!(state.epoch, 4);
        assert_eq!(state.admitted_transaction_count, 1);
    }

    #[test]
    fn dropping_unconsumed_queue_drain_authority_fail_closes_without_rollback() {
        let protocol = protocol(QueueEpochState::Draining, 9, 0);
        let txn = QueueEpochDrainTxn {
            protocol: Arc::clone(&protocol),
            epoch: 9,
            next_epoch: 10,
            active: true,
        };

        drop(txn);

        let state = protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Closed);
        assert_eq!(state.epoch, 9);
    }

    #[test]
    fn explicit_queue_authority_abort_is_the_only_normal_rollback_path() {
        let transaction_protocol = protocol(QueueEpochState::Open, 11, 1);
        QueueEpochToken {
            protocol: Arc::clone(&transaction_protocol),
            queue_identity: Some(77),
            epoch: 11,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        }
        .abort()
        .unwrap();
        {
            let state = transaction_protocol.state.lock().unwrap();
            assert_eq!(state.state, QueueEpochState::Open);
            assert_eq!(state.admitted_transaction_count, 0);
        }

        let drain_protocol = protocol(QueueEpochState::Draining, 12, 0);
        QueueEpochDrainTxn {
            protocol: Arc::clone(&drain_protocol),
            epoch: 12,
            next_epoch: 13,
            active: true,
        }
        .abort()
        .unwrap();
        let state = drain_protocol.state.lock().unwrap();
        assert_eq!(state.state, QueueEpochState::Open);
        assert_eq!(state.epoch, 12);
    }

    #[test]
    fn fail_closed_drop_recovers_a_poisoned_protocol_lock_without_panicking() {
        let protocol = protocol(QueueEpochState::Open, 13, 1);
        let poison_target = Arc::clone(&protocol);
        assert!(std::thread::spawn(move || {
            let _guard = poison_target.state.lock().unwrap();
            panic!("poison queue epoch lock for drop-backstop test");
        })
        .join()
        .is_err());

        let token = QueueEpochToken {
            protocol: Arc::clone(&protocol),
            queue_identity: Some(77),
            epoch: 13,
            direction: QueueTransactionDirection::Read,
            reserved_bytes: 188,
            active: true,
        };
        drop(token);

        let state = protocol.state.lock().unwrap_err().into_inner();
        assert_eq!(state.state, QueueEpochState::Closed);
        assert_eq!(state.epoch, 13);
    }
}
'''
    QUEUE.write_text(queue_text)

# Validate the reviewed structural contracts after all edits.
queue_text = QUEUE.read_text()
for forbidden in [
    "if self.active && self.release().is_err()",
    "if self.active && self.rollback().is_err()",
]:
    if forbidden in queue_text:
        raise SystemExit(f"queue Drop still contains fallible cleanup: {forbidden}")
if "fn fail_close_unconsumed_authority(&self)" not in queue_text:
    raise SystemExit("infallible queue authority fail-close helper missing")

for name in authorities:
    path = locations[name]
    text = path.read_text()
    decl_match = re.search(rf"^(?:pub(?:\([^\n)]*\))?\s+)?(?:struct|enum)\s+{re.escape(name)}\b", text, re.M)
    if not decl_match:
        raise SystemExit(f"{name}: final declaration missing")
    before = text[max(0, decl_match.start() - 700):decl_match.start()]
    if "#[must_use" not in before.split("\n\n")[-1]:
        # Fall back to local line scan across derive/doc attributes.
        lines = text[:decl_match.start()].splitlines()
        seen = False
        for line in reversed(lines[-20:]):
            stripped = line.strip()
            if stripped.startswith("#[must_use"):
                seen = True
                break
            if stripped and not (stripped.startswith("#[") or stripped.startswith("///") or stripped.startswith("//!")):
                break
        if not seen:
            raise SystemExit(f"{path}: {name} lacks #[must_use]")

print("updated authority declarations:")
for name in authorities:
    print(f"  {name}: {locations[name]}")
