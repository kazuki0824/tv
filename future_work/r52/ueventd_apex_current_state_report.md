# r51 調査: 問題点 1 と 4 で現在何が起こっているのか

## 対象

このレポートは次の 2 点だけを扱う。

```text
1. ueventd rc が install されるだけで import されない問題
4. APEX 化 template と Soong/module/install 経路の固定不足
```

## 1. ueventd rc で現在起こっていること

### 現状

r50x APEX 修正版では、Tuner HAL 用 device node permission は次の file にまとまっている。

```text
vendor/maleicacid/tv/tuner_hal/config/ueventd.tuner_hal.rc
```

この file は Soong module として vendor に install される。

```text
prebuilt_etc name: maleicacid_tuner_hal_ueventd_rc
install path: /vendor/etc/ueventd.tuner_hal.rc
```

README でも、通常方式・APEX 方式の両方で `maleicacid_tuner_hal_ueventd_rc` を product package に入れるように書かれている。

### 問題

Android の root `ueventd.rc` は `/vendor/etc/ueventd.rc` と `/odm/etc/ueventd.rc` を import する。任意名の `/vendor/etc/ueventd.tuner_hal.rc` を自動的に読むわけではない。

したがって、現在の r50x では次の状態になる。

```text
/vendor/etc/ueventd.tuner_hal.rc は image に存在する
しかし /vendor/etc/ueventd.rc から import されない
結果として ueventd が tuner 用 node permission を適用しない
```

### 影響

次の node が期待通りにならない可能性が高い。

```text
/dev/dvb/adapter*/frontend*
/dev/dvb/adapter*/demux*
/dev/dvb/adapter*/dvr*
/dev/dvb/adapter*/net*
/dev/px4video*
/dev/pxmlt5video*
/dev/pxmlt8video*
/dev/isdb6014video*
/dev/isdb2056video*
/dev/pxm1urvideo*
/dev/pxs1urvideo*
/dev/isdbt2071video*
```

Tuner HAL service は `user media`, `group media system` で動くため、node permission が適用されないと open できない。

### 修正方針

`config/ueventd.tuner_hal.rc` を SSOT として維持する。そのうえで、製品の `/vendor/etc/ueventd.rc` に次の 1 行を入れることを正式手順にする。

```text
import /vendor/etc/ueventd.tuner_hal.rc
```

r51 の README では、通常方式・APEX 方式の両方でこの import が必須であると固定する。`ueventd.vendor.import.example.rc` は example ではなく、製品の vendor ueventd rc に取り込む正式 fragment として扱う。

### APEX 方式での注意

ueventd は boot 時の device node policy であり、APEX payload 内に閉じない。APEX 方式でも `/vendor/etc/ueventd.tuner_hal.rc` と `/vendor/etc/ueventd.rc` import は vendor image 側に残す。

## 4. APEX 化 template で現在起こっていること

### 現状

r50x APEX 修正版には次の template がある。

```text
tuner_hal/apex_template/Android.bp.fragment
tuner_hal/apex_template/apex_manifest.json
tuner_hal/apex_template/file_contexts
tuner_hal/apex_template/tuner-hal-service.apex.rc
```

APEX module は次を package する設計になっている。

```text
binaries:
  maleicacid.tv.tuner_hal-service
prebuilts:
  maleicacid_tuner_hal_apex_vintf_fragment
  maleicacid_tuner_hal_apex_init_rc
```

APEX service path は次に固定されている。

```text
/apex/com.maleicacid.tv.tuner_hal/bin/hw/maleicacid.tv.tuner_hal-service
```

### 問題

現行の `maleicacid.tv.tuner_hal-service` binary module は通常 vendor install 用 module として定義されている。

```text
rust_binary {
  name: "maleicacid.tv.tuner_hal-service"
  vendor: true
  relative_install_path: "hw"
  init_rc: ["tuner-hal-service.rc"]
  vintf_fragments: ["tuner-hal-service.xml"]
}
```

この module を APEX の `binaries` に入れる設計自体は方向性として妥当だが、r50x template では次がまだ固定不足である。

```text
- APEX payload に入る binary と通常 vendor install binary の二重 install 防止
- APEX 内 VINTF fragment と通常 module property の VINTF fragment の二重登録防止
- APEX 内 init rc と通常 module property の init rc の二重登録防止
- APEX 方式で必要な apex_available / dependency visibility の固定
- APEX payload に native shared dependency が解決されることの固定
- APEX service を update する場合の updatable / override / bootstrap 方針
```

### 影響

通常方式では問題になりにくいが、APEX 方式では次が起こり得る。

```text
- service binary が /vendor/bin/hw と /apex/... の両方に入る
- 同じ ITuner/default VINTF fragment が vendor 側と APEX 側に重複する
- 同じ service name が vendor rc と APEX rc の両方で定義される
- Soong が APEX payload に binary を入れられない
- binary は入るが dependency が vendor APEX namespace で解決できない
```

### 修正方針

r51 では通常方式と APEX 方式を Soong module level で分ける。

```text
通常方式:
  maleicacid.tv.tuner_hal-service.vendor
  init_rc: tuner-hal-service.rc
  vintf_fragments: tuner-hal-service.xml

APEX方式:
  maleicacid.tv.tuner_hal-service.apex
  init_rc: none on binary module
  vintf_fragments: none on binary module
  apex_available: ["com.maleicacid.tv.tuner_hal"]
  APEX prebuilts: VINTF fragment + APEX rc
```

APEX 方式の product package には次だけを入れる。

```text
com.maleicacid.tv.tuner_hal
maleicacid_tuner_hal_vts_config_aidl_v2
maleicacid_tuner_hal_ueventd_rc
```

通常方式の service package は入れない。

### r51 で固定すべき完了条件

```text
- m com.maleicacid.tv.tuner_hal が通る
- APEX payload に bin/hw/maleicacid.tv.tuner_hal-service が入る
- APEX payload に etc/vintf/tuner-hal-service.xml が入る
- APEX payload に init rc が入る
- /vendor/bin/hw/maleicacid.tv.tuner_hal-service が同時 install されない
- /vendor/etc/vintf 以下に同じ Tuner HAL fragment が同時 install されない
- /vendor/etc/init 以下に同じ service rc が同時 install されない
- /vendor/etc/ueventd.rc が /vendor/etc/ueventd.tuner_hal.rc を import する
```

## 参照元

- r50x APEX 修正版: `tuner_hal/README_JA.md`
- r50x APEX 修正版: `tuner_hal/Android.bp`
- r50x APEX 修正版: `tuner_hal/apex_template/*`
- r50x APEX 修正版: `tuner_hal/config/ueventd*.rc`
- AOSP root `ueventd.rc`
- AOSP vendor APEX / APEX service documentation
