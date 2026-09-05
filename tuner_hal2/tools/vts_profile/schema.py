from __future__ import annotations

import os
import shlex
import subprocess
import tempfile
from pathlib import Path
from xml.etree import ElementTree

from .model import ProfileError

XSD_RELATIVE_PATH = Path("tv/tuner/config/tuner_testing_dynamic_configuration.xsd")
SOONG_UI_RELATIVE_PATH = Path("build/soong/soong_ui.bash")
AOSP_XSDC_TARGET = "xsdc"
AOSP_TUNER_PACKAGE = "android.media.tuner.testing.configuration.V1_0"


def _git_commit(root: Path, ref: str) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", f"{ref}^{{commit}}"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ProfileError(f"cannot resolve AOSP VTS ref {ref!r} in {root}") from exc
    return result.stdout.strip()


def selected_xsd(hardware_interfaces_root: Path, source_ref: str) -> Path:
    root = hardware_interfaces_root.resolve()
    if _git_commit(root, "HEAD") != _git_commit(root, source_ref):
        raise ProfileError("hardware/interfaces checkout HEAD does not match profile vts.source_ref")
    xsd = root / XSD_RELATIVE_PATH
    if not xsd.is_file():
        raise ProfileError(f"AOSP Tuner VTS XSD not found: {xsd}")
    return xsd


def _run_checked(command: list[str], *, cwd: Path, label: str) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = ""
        if isinstance(exc, subprocess.CalledProcessError):
            detail = (exc.stderr or exc.stdout or "").strip()
        raise ProfileError(label + (f": {detail}" if detail else "")) from exc
    return result.stdout.strip()


def _soong_ui(aosp_root: Path) -> Path:
    entry = aosp_root / SOONG_UI_RELATIVE_PATH
    if not entry.is_file():
        raise ProfileError(f"AOSP Soong entry point not found: {entry}")
    return entry


def _build_aosp_xsdc(aosp_root: Path) -> Path:
    root = aosp_root.resolve()
    soong_ui = _soong_ui(root)
    _run_checked(
        [str(soong_ui), "--make-mode", AOSP_XSDC_TARGET],
        cwd=root,
        label="failed to build AOSP xsdc",
    )
    host_out_text = _run_checked(
        [str(soong_ui), "--dumpvar-mode", "HOST_OUT_EXECUTABLES"],
        cwd=root,
        label="failed to resolve AOSP HOST_OUT_EXECUTABLES",
    )
    if not host_out_text:
        raise ProfileError("AOSP HOST_OUT_EXECUTABLES is empty")
    host_out = Path(host_out_text)
    if not host_out.is_absolute():
        host_out = root / host_out
    xsdc = host_out / AOSP_XSDC_TARGET
    if not xsdc.is_file():
        raise ProfileError(f"AOSP xsdc executable was not produced: {xsdc}")
    return xsdc


def _pkg_config(flag: str, *, cwd: Path) -> list[str]:
    output = _run_checked(
        ["pkg-config", flag, "libxml-2.0"],
        cwd=cwd,
        label=f"failed to resolve libxml2 {flag} with pkg-config",
    )
    return shlex.split(output)


def _compile_selected_aosp_consumer(aosp_root: Path, xsd: Path, workdir: Path) -> Path:
    root = aosp_root.resolve()
    xsdc = _build_aosp_xsdc(root)
    generated = workdir / "generated"
    generated.mkdir(parents=True)
    _run_checked(
        [
            str(xsdc),
            "-c",
            "-p",
            AOSP_TUNER_PACKAGE,
            "-o",
            str(generated),
            "-r",
            "TunerConfiguration",
            str(xsd),
        ],
        cwd=root,
        label="failed to generate the selected AOSP Tuner xsdc consumer",
    )
    generated_cpp = sorted(generated.rglob("*.cpp"))
    if not generated_cpp:
        raise ProfileError("AOSP xsdc produced no C++ parser sources")

    main_cpp = workdir / "validator_main.cpp"
    main_cpp.write_text(
        "#include <android_media_tuner_testing_configuration_V1_0.h>\n"
        "int main(int argc, char** argv) {\n"
        "    if (argc != 2) return 2;\n"
        "    using namespace android::media::tuner::testing::configuration::V1_0;\n"
        "    return read(argv[1]).has_value() ? 0 : 1;\n"
        "}\n",
        encoding="utf-8",
    )
    output = workdir / "tuner-xsdc-consumer"
    cxx = os.environ.get("CXX", "c++")
    command = [
        cxx,
        "-std=c++17",
        f"-I{generated / 'include'}",
        f"-I{root / 'system/tools/xsdc/utils/include'}",
        *_pkg_config("--cflags", cwd=root),
        str(main_cpp),
        *(str(path) for path in generated_cpp),
        *_pkg_config("--libs", cwd=root),
        "-o",
        str(output),
    ]
    _run_checked(
        command,
        cwd=root,
        label="failed to compile the selected AOSP Tuner xsdc consumer",
    )
    if not output.is_file():
        raise ProfileError(f"AOSP Tuner xsdc consumer was not produced: {output}")
    return output


def _aosp_tree_from_selected_xsd(xsd: Path) -> tuple[Path, Path]:
    resolved = xsd.resolve()
    suffix = XSD_RELATIVE_PATH.parts
    if len(resolved.parts) <= len(suffix) or resolved.parts[-len(suffix) :] != suffix:
        raise ProfileError("selected Tuner XSD path is not under hardware/interfaces")
    interfaces = Path(*resolved.parts[: -len(suffix)])
    if interfaces.name != "interfaces" or interfaces.parent.name != "hardware":
        raise ProfileError("selected Tuner XSD path is not under hardware/interfaces")
    return interfaces.parent.parent.resolve(), interfaces.resolve()


def _validate_well_formed(xml: str) -> None:
    try:
        ElementTree.fromstring(xml)
    except ElementTree.ParseError as exc:
        raise ProfileError(f"generated VTS XML is not well-formed: {exc}") from exc


def _validate_with_selected_aosp_consumer(xml: str, *, aosp_root: Path, xsd: Path) -> None:
    _validate_well_formed(xml)
    with tempfile.TemporaryDirectory(prefix="tuner-vts-xsdc-") as directory:
        workdir = Path(directory)
        validator = _compile_selected_aosp_consumer(aosp_root, xsd, workdir)
        xml_path = workdir / "tuner_vts_config.xml"
        xml_path.write_text(xml, encoding="utf-8")
        try:
            result = subprocess.run(
                [str(validator), str(xml_path)],
                cwd=aosp_root,
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            raise ProfileError(f"failed to execute AOSP xsdc Tuner config consumer: {exc}") from exc
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise ProfileError(
                "generated VTS XML is rejected by the selected AOSP xsdc Tuner config consumer"
                + (f": {detail}" if detail else "")
            )


def validate_xml_with_aosp_consumer(
    xml: str,
    *,
    aosp_root: Path,
    hardware_interfaces_root: Path,
    source_ref: str,
) -> None:
    root = aosp_root.resolve()
    interfaces = hardware_interfaces_root.resolve()
    expected_interfaces = (root / "hardware/interfaces").resolve()
    if interfaces != expected_interfaces:
        raise ProfileError(
            "hardware/interfaces root must belong to the same AOSP tree used to build xsdc"
        )
    xsd = selected_xsd(interfaces, source_ref)
    _validate_with_selected_aosp_consumer(xml, aosp_root=root, xsd=xsd)


def _validate_with_external_xsd(xml: str, xsd: Path, executable: str) -> None:
    _validate_well_formed(xml)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".xml", delete=True) as tmp:
        tmp.write(xml)
        tmp.flush()
        try:
            result = subprocess.run(
                [executable, "--noout", "--schema", str(xsd), tmp.name],
                capture_output=True,
                text=True,
            )
        except OSError as exc:
            raise ProfileError(f"failed to execute XSD validator {executable!r}: {exc}") from exc
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise ProfileError(f"generated VTS XML does not satisfy selected XSD: {detail}")


def validate_xml(xml: str, xsd: Path, *, xmllint: str = "xmllint") -> None:
    try:
        aosp_root, _ = _aosp_tree_from_selected_xsd(xsd)
    except ProfileError:
        if xmllint == "xmllint":
            raise
        _validate_with_external_xsd(xml, xsd, xmllint)
        return
    _validate_with_selected_aosp_consumer(xml, aosp_root=aosp_root, xsd=xsd.resolve())
