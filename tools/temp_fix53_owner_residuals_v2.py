from pathlib import Path

helper = Path("tools/temp_fix53_owner_residuals.py")
text = helper.read_text()
old = '''replace_once(
    frontend_ops,
    "        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(\\n",
    "        let fixed_power = Self::ensure_frontend_fixed_power_for_object(\\n",
)
replace_once(
    frontend_ops,
    "        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(\\n",
    "        let fixed_power = Self::ensure_frontend_fixed_power_for_object(\\n",
)
'''
new = '''text = frontend_ops.read_text()\nold_fixed_power_call = "        let fixed_power = crate::lnb_ops::ensure_frontend_fixed_power_for_object(\\n"\nnew_fixed_power_call = "        let fixed_power = Self::ensure_frontend_fixed_power_for_object(\\n"\nif text.count(old_fixed_power_call) != 2:\n    raise SystemExit(f"frontend fixed-power call count={text.count(old_fixed_power_call)}")\nfrontend_ops.write_text(text.replace(old_fixed_power_call, new_fixed_power_call))\n'''
if text.count(old) != 1:
    raise SystemExit("expected duplicate fixed-power helper block not found")
text = text.replace(old, new, 1)

# The runner callback is intentionally an internal generic reaper boundary. Naming a
# public alias only to satisfy Clippy would enlarge the API surface, so suppress the
# complexity lint locally at the two owner-controlled factory boundaries.
public_factory = "    pub fn start_reaper_queue<K, V, J>(\\n"
if text.count(public_factory) != 1:
    raise SystemExit(f"public reaper factory count={text.count(public_factory)}")
text = text.replace(
    public_factory,
    "    #[allow(clippy::type_complexity)]\\n" + public_factory,
    1,
)
private_factory = "impl<K, V, J> WorkerRuntimeReaperQueue<K, V, J>\\nwhere"
if text.count(private_factory) != 1:
    raise SystemExit(f"private reaper impl count={text.count(private_factory)}")
idx = text.index(private_factory)
start_idx = text.index("    fn start(\\n", idx)
text = text[:start_idx] + "    #[allow(clippy::type_complexity)]\\n" + text[start_idx:]

helper.write_text(text)
exec(compile(helper.read_text(), str(helper), "exec"))
