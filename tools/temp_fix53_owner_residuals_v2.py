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
helper.write_text(text.replace(old, new, 1))
exec(compile(helper.read_text(), str(helper), "exec"))
