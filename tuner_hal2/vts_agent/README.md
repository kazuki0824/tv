# tuner_hal2 VTS device agent

`maleicacid_tuner_hal2_vts_agent` is a thin, temporary device-side bridge used only by the host VTS profile CLI.

Its responsibility is limited to operations that must execute on the Android device:

- connect to the public `android.hardware.tv.tuner.ITuner/default` Binder service;
- own the temporary `IFrontend`, `IDemux`, and `IFilter` Binder objects;
- tune once and confirm `DEMOD_LOCK` through public AIDL;
- keep that frontend/demux/tune session alive while the host requests SI sections;
- open a TS `SECTION` filter for each PID/table requested by the host;
- import and read the filter FMQ; and
- return the completed section-filter payload to the host.

The agent must not parse PAT/PMT/SDT semantics, select services or elementary PIDs, know the `VtsEnvironmentProfile` schema, or alter HAL capability. C++ in this directory is restricted to the FMQ descriptor import/read boundary; Binder/AIDL control and session lifetime are Rust.

## Resolution boundary

One frequency candidate is resolved in one device session and one tune generation:

```text
host CLI                    device agent                 host arib_si_engine_rs core
   |                            |                                   |
   |---- start/tune ----------->|                                   |
   |<------- ready -------------|                                   |
   |---- PAT section ---------->|                                   |
   |<------- PAT bytes ---------|---- no SI interpretation -------->|
   |---------------------------------------------------- PAT meaning |
   |<----------------------------------------------- PMT PID list ---|
   |---- PMT section(s) ------->|                                   |
   |<------- PMT bytes ---------|                                   |
   |---- SDT actual ----------->|                                   |
   |<------- SDT bytes ---------|                                   |
   |------------------------------------------------ PAT+PMT+SDT --->|
   |<-------------------------------- canonical service/ES facts ----|
   |---- close ---------------->|                                   |
```

PAT determines the PMT PIDs. PMT carries the elementary streams. SDT actual is also collected in the same tune session because the canonical `ServiceDiscoveryCollector` binds the PAT/PMT result to the full ARIB service identity through its existing public SI path. This avoids exposing private pending-PMT state merely for VTS tooling and avoids creating a VTS-specific PAT/PMT semantic API. The additional SDT read is therefore host-parser input, not device-side semantic processing.

`arib_si_engine_rs` syntax/semantic code is compiled once as `libmaleicacid_arib_si_engine_core`. The Android JNI adapter and `maleicacid_arib_si_engine_vts_host` both depend on that library. The host adapter must not `include!` the parser source or carry an independent PAT/PMT implementation.

## Product placement

The agent is deliberately not in the normal product `PRODUCT_PACKAGES`. `resolve-device` receives a locally built agent via `--agent`, adb-pushes it to `/data/local/tmp`, invokes it, and removes it when resolution ends.

If target device SELinux/linker policy prevents a pushed shell-domain agent from accessing the public Tuner AIDL service, the defined alternative is to include the same thin agent only in an explicit VTS/test image via `config/vts_test_agent_integration.mk`; normal product integration remains unchanged.
