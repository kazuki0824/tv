from pathlib import Path
import re

path = Path("tuner_hal2/control/src/lib.rs")
text = path.read_text()

runner_type = r'''std::sync::Arc<
            dyn Fn(
                    J,
                    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
                ) + Send
                + Sync
                + 'static,
        >'''

# Both WorkerRuntime::start_reaper_queue and the private subordinate handle factory
# use a generic runner type. This keeps the public lifecycle owner unchanged while
# avoiding a duplicated dyn-Fn type expression that Clippy flags as type_complexity.
text, count = re.subn(
    r"pub fn start_reaper_queue<K, V, J>\(",
    "pub fn start_reaper_queue<K, V, J, F>(",
    text,
    count=1,
)
if count != 1:
    raise SystemExit(f"start_reaper_queue generic anchor count={count}")

text, count = re.subn(
    r"fn start\(\n        capacity: usize,\n        thread_prefix: &'static str,\n        runner: " + runner_type + r",\n    \) -> Result<Self, maleicacid_tuner_hal2_common::HalError>\n    where\n        K: Ord \+ Clone \+ Send \+ 'static,\n        V: Clone \+ Send \+ 'static,\n        J: Send \+ 'static,",
    "fn start<F>(\n        capacity: usize,\n        thread_prefix: &'static str,\n        runner: std::sync::Arc<F>,\n    ) -> Result<Self, maleicacid_tuner_hal2_common::HalError>\n    where\n        K: Ord + Clone + Send + 'static,\n        V: Clone + Send + 'static,\n        J: Send + 'static,\n        F: Fn(\n                J,\n                std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,\n            ) + Send\n            + Sync\n            + 'static,",
    text,
    count=1,
)
if count != 1:
    raise SystemExit(f"private reaper start anchor count={count}")

# Replace the public factory runner argument and append the F bound.
old = """        runner: std::sync::Arc<
            dyn Fn(
                    J,
                    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
                ) + Send
                + Sync
                + 'static,
        >,
    ) -> Result<WorkerRuntimeReaperQueue<K, V, J>, maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Clone + Send + 'static,
        V: Clone + Send + 'static,
        J: Send + 'static,
"""
new = """        runner: std::sync::Arc<F>,
    ) -> Result<WorkerRuntimeReaperQueue<K, V, J>, maleicacid_tuner_hal2_common::HalError>
    where
        K: Ord + Clone + Send + 'static,
        V: Clone + Send + 'static,
        J: Send + 'static,
        F: Fn(
                J,
                std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,
            ) + Send
            + Sync
            + 'static,
"""
if text.count(old) != 1:
    raise SystemExit(f"public runner type anchor count={text.count(old)}")
text = text.replace(old, new, 1)

path.write_text(text)
print("PR53 worker factory type complexity simplified")
