# tuner_hal2 VTS device agent

`maleicacid_tuner_hal2_vts_agent` is a thin, temporary device-side bridge used only by the host VTS profile CLI.

Its responsibility is limited to operations that must execute on the Android device:

- connect to the public `android.hardware.tv.tuner.ITuner/default` Binder service;
- own the temporary `IFrontend`, `IDemux`, and `IFilter` Binder objects;
- tune and confirm `DEMOD_LOCK` through public AIDL;
- open a TS `SECTION` filter for the PID/table requested by the host;
- import and read the filter FMQ; and
- return the completed section-filter payload to the host.

It must not parse PAT/PMT semantics, select services or elementary PIDs, know the `VtsEnvironmentProfile` schema, or alter HAL capability. PAT/PMT syntax and semantics are evaluated on the host with `maleicacid_arib_si_engine_vts_host`, which reuses the canonical `arib_si_engine_rs` parser implementation.

The agent is deliberately not in the normal product `PRODUCT_PACKAGES`. `resolve-device` receives a locally built agent via `--agent`, adb-pushes it to `/data/local/tmp`, invokes it, and removes it when resolution ends. If a target device policy prevents a pushed shell-domain agent from accessing the public Tuner AIDL service, the defined alternative is to include the same thin agent only in an explicit VTS/test image; it must not become an unconditional production package.

C++ in this directory is restricted to the FMQ descriptor import/read boundary. Binder/AIDL control is implemented in Rust.
