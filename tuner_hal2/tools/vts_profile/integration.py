from __future__ import annotations
from pathlib import Path
from .render import output_filename


def _write(path: Path, text: str) -> None:
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(text, encoding="utf-8")
    tmp.replace(path)


def write_product_artifacts(profile: dict, xml: str, directory: Path) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    for stale in directory.glob("tuner_vts_config_aidl_V1*.xml"):
        stale.unlink()
    filename = output_filename(profile)
    xml_path = directory / filename
    _write(xml_path, xml)
    mk = (
        "_tuner_hal2_vts_generated_dir := $(dir $(lastword $(MAKEFILE_LIST)))\n"
        f"PRODUCT_COPY_FILES += $(_tuner_hal2_vts_generated_dir){filename}:$(TARGET_COPY_OUT_VENDOR)/etc/{filename}\n"
    )
    variant = profile["vts"].get("variant", "")
    if variant:
        mk += f"PRODUCT_VENDOR_PROPERTIES += ro.vendor.vts_tuner_configuration_variant={variant}\n"
    _write(directory / "vts_product_generated.mk", mk)
    return xml_path
