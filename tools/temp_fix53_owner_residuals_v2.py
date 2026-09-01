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

# Name the internal callback shape rather than suppressing Clippy. The alias is
# private to control-core and does not enlarge the public API surface.
alias_anchor = "impl WorkerRuntime<()> {\n"
alias = '''type WorkerReaperRunner<K, V, J> = dyn Fn(\n        J,\n        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,\n    ) + Send\n    + Sync\n    + 'static;\n\n'''
if text.count(alias_anchor) != 1:
    raise SystemExit(f"worker runtime impl anchor count={text.count(alias_anchor)}")
text = text.replace(alias_anchor, alias + alias_anchor, 1)

complex_runner = '''std::sync::Arc<\n            dyn Fn(\n                    J,\n                    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<K, V>>>,\n                ) + Send\n                + Sync\n                + 'static,\n        >'''
if text.count(complex_runner) != 2:
    raise SystemExit(f"complex reaper runner count={text.count(complex_runner)}")
text = text.replace(complex_runner, "std::sync::Arc<WorkerReaperRunner<K, V, J>>")

helper.write_text(text)
exec(compile(helper.read_text(), str(helper), "exec"))
