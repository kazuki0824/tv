## r50bi6

- Phase B 完了証跡として、既存の SDT / NIT / BAT scope 実装が ONID+TSID / table-specific scope に閉じていることを静的確認した。
- TIS 側 JNI production path から呼ばれる snapshot を bulk wrapper 経由にし、count + index 型 getter を AribSiEngine public path から直接呼ばないようにした。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bi4

- parental_rating_descriptor の Rust 側出力が ARIB 構造化データと diagnostic JSON に留まり、Android `TvContentRating` domain / ISDB rating string を持たないことを behavior test で固定した。
- malformed length / truncated parental rating descriptor が diagnostic として記録され、Android rating projection 文字列を Rust 側に混入させないことを test で補強した。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。


## r50bi3

- service publishability diagnostic に `pmt_pid_resolved` / `pmt_parsed` / `ca_state_resolved` / `free_ca_mode_resolved` を追加し、TIS が current diagnostic complete を理由文字列だけに依存せず判定できるようにした。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

# CHANGELOG

## r50bh

- Replaced persistent/product-path `r51_live_claimable` naming with `clear_live_playback_supported`.
- Added service publishability fields for `channel_registration_ready`, `epg_publishable`, `requires_cas`, and `unsupported_cas`, and made `registration_ready_snapshot()` depend on the explicit Rust readiness flag.
- Made clear live playback support depend on transport/service publishability, registration readiness, supported video, and clear/no-CA state; scrambled services may be channel/EPG publishable but are not clear live playback supported.
- Android/Soong build, Rust unit test, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bg

- service-local registration-ready snapshot を通常 channel registration 用 snapshot として公開し、clear live claimable と scrambled unsupported registration を分離した。
- EIT section 更新後の event set が空になった場合も update window を保持し、TIS が obsolete Programs delete に使える JNI accessor を追加した。
- `arib_si_engine_rs/DESIGN_JA.md` を r51 の service-local registration-ready 方針と empty EIT update window 方針に合わせて改訂した。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf2

- r50bf のロジック未達を是正し、ARIB content_descriptor の display name を `<majorName>/<middleName>` 形式に変更した。
- JNI の broadcast genre token は `ARIB(0xM/0xN):<majorName>/<middleName>` を返し、supplement text も同じ分類名を保持するようにした。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bf

- PMT の `PCR_PID=0x1fff` を PCR なしとして正規化し、r51 clear-live claimable 判定で `NO_PCR_PID` reason を出すようにした。
- PMT parse / descriptor-loop malformed 判定を PAT で確定した PMT PID に限定し、`table_id=0x02` 単独では PMT と見なさないようにした。
- `SectionAssembler` を test-only に閉じ、production の `arib_si_engine_rs` は assembled section payload の semantic parse だけを担当する境界へ戻した。
- ARIB content_descriptor 由来の broadcast genre token を `ARIB(0xM/0xN):<表示名>` 形式で JNI から返す accessor を追加した。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50be

- CHANGELOG の見出しを `# CHANGELOG` と `## r50be` 形式に統一した。
- arib_si_engine_rs の実装ロジックは r50bd から変更していない。

## r50bd

- r51向け Direct Boot 境界、TvProvider Programs 更新、service scoped CAS、AudioTrack write 診断、PTS fallback 診断、extended event JSON 解析、TIS product integration を更新。

## r50bc4

- r50bc3 完了判定で指摘された証跡不一致を踏まえ、EIT same-version 差分削除の説明コメントを日本語化し、r51 clear-live claimability の静的証跡対象を整理した。
- Android/Soong build、Rust unit test、atest、VTS、CTS、実機確認はこの環境では未実施。

## r50bc3

- Split r51 clear-live claimability from transport-level publishability so a service with PMT/PCR, r51-supported video, `free_ca_mode=false`, and no CA descriptors can appear in the r51 viewable snapshot even while NIT or other transport-level discovery is still incomplete.
- Added Rust regression coverage for service-level r51 claimability when the transport-level NIT completion gate is still missing.
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb7

- Added JNI accessors for structured `parental_rating_descriptor` entries so TIS can project ARIB ratings to Android `TvContentRating`.

## r50bb4

- Added raw discovery PMT PID access for section filter control, independent of r51 viewable service snapshot filtering.
- Added JNI/Kotlin raw CAS discovery service and CA metadata accessors so PMT/CAT CA metadata can be used for diagnostics and ECM/EMM filter setup without publishing scrambled services as clear-viewable channels.
- Added a Rust regression test confirming PAT-derived PMT PIDs are available for section filters before r51 viewable snapshot publication.
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50bb

- Changed `libmaleicacid_arib_si_engine_jni` from `product_available: true` to `product_specific: true` according to the supplied Soong patch.
- Android/Soong build, Rust unit tests, atest, VTS, CTS, and real-device checks were not run in this environment.

## r50ba5

- Added r51 test coverage for clear MPEG-2/AVC video service claimability and rejection of audio-only, data-only, HEVC-only, SDT-scrambled, PMT program-CA, and video ES-CA services.
- Added descriptor parser tests for short_event, content, component, event_group, linkage, unknown descriptor preservation, and diagnostic JSON coverage.
- Added EIT tests for invalid time ranges, undefined MJD, and descriptor-loop overflow diagnostics.
- Added CAT tests for version replacement and same-version multi-section merge.
- Added ARIB string diagnostic entry field coverage.

## r50ba4

- Removed `product_available: true` from `maleicacid_arib_si_engine_rs_test` so the Rust test module does not request a product image variant of Soong `libtest`.
- Added r51 publishability diagnostic fields for `viewable`, `r51LiveClaimable`, and r51 exclusion reasons across Rust JNI and Kotlin models.
- Changed ARIB string lossy decode diagnostics from aggregate-only counters to offset/code-set/reason/replacement entries while preserving summary counters.
- Preserved malformed EIT event descriptor-loop overflow as event diagnostics instead of silently breaking the parse loop.

## r50ba3

- Reworked r51 ARIB SI / TvProvider projection planning implementation.
