#!/usr/bin/env bash
set -euo pipefail
TARGET_DIR=${1:?target directory required}
EXPECTED_TARGET_SHA=${EXPECTED_TARGET_SHA:?}
TARGET_BRANCH=${TARGET_BRANCH:?}
RUST_TOOLCHAIN=${RUST_TOOLCHAIN:-1.81.0}
KOTLIN_VERSION=${KOTLIN_VERSION:-1.9.22}
STAGING_DIR=$(cd "$(dirname "$0")" && pwd)
cd "$TARGET_DIR"
test "$(git rev-parse HEAD)" = "$EXPECTED_TARGET_SHA"
python3 "$STAGING_DIR/codex_apply_pr54_review_core.py"
python3 "$STAGING_DIR/codex_apply_pr54_review_remaining_fixed.py"
cargo +"$RUST_TOOLCHAIN" fmt --manifest-path arib_si_engine_rs/host_ci/Cargo.toml
git diff --check
cargo +"$RUST_TOOLCHAIN" check --locked --manifest-path arib_si_engine_rs/host_ci/Cargo.toml
cargo +"$RUST_TOOLCHAIN" test --locked --manifest-path arib_si_engine_rs/host_ci/Cargo.toml
python3 - <<'PY'
import json
from pathlib import Path
for p in sorted(Path('arib_si_engine_rs/schema').glob('*.json')): json.loads(p.read_text())
for p in sorted(Path('arib_si_engine_rs/testdata').rglob('*.json')): json.loads(p.read_text())
print('JSON parse OK')
PY

tools_dir="$RUNNER_TEMP/tis-kotlin-test-review54"
mkdir -p "$tools_dir"
curl --fail --location --retry 3 --retry-all-errors "https://github.com/JetBrains/kotlin/releases/download/v${KOTLIN_VERSION}/kotlin-compiler-${KOTLIN_VERSION}.zip" -o "$tools_dir/kotlin-compiler.zip"
curl --fail --location --retry 3 --retry-all-errors "https://repo.maven.apache.org/maven2/org/robolectric/android-all/14-robolectric-10818077/android-all-14-robolectric-10818077.jar" -o "$tools_dir/android-all.jar"
curl --fail --location --retry 3 --retry-all-errors "https://repo.maven.apache.org/maven2/junit/junit/4.13.2/junit-4.13.2.jar" -o "$tools_dir/junit.jar"
curl --fail --location --retry 3 --retry-all-errors "https://repo.maven.apache.org/maven2/org/hamcrest/hamcrest-core/1.3/hamcrest-core-1.3.jar" -o "$tools_dir/hamcrest-core.jar"
echo "88b39213506532c816ff56348c07bbeefe0c8d18943bffbad11063cf97cac3e6  $tools_dir/kotlin-compiler.zip" | sha256sum -c
echo "6be2218c6a53fe3c57bc22ebdc723edcb7270a8a6f187545708aa5c0ed813977  $tools_dir/android-all.jar" | sha256sum -c
unzip -q "$tools_dir/kotlin-compiler.zip" -d "$tools_dir/compiler"
kotlin_home="$tools_dir/compiler/kotlinc"
android_jar="$tools_dir/android-all.jar"
junit_jar="$tools_dir/junit.jar"
hamcrest_jar="$tools_dir/hamcrest-core.jar"
production_dir="$RUNNER_TEMP/tis-kotlin-production-review54"
test_dir="$RUNNER_TEMP/tis-kotlin-tests-review54"
mkdir -p "$production_dir" "$test_dir"
mapfile -d '' -t production_sources < <(find tis/src tis/host_ci/kotlin/stubs/android -type f -name '*.kt' -print0 | sort -z)
"$kotlin_home/bin/kotlinc" -J-Xmx4g -jvm-target 17 -classpath "$android_jar" -d "$production_dir" "${production_sources[@]}"
test_cp="$production_dir:$android_jar:$junit_jar:$hamcrest_jar:$kotlin_home/lib/kotlin-test.jar:$kotlin_home/lib/kotlin-test-junit.jar"
mapfile -d '' -t test_sources < <(find tis/tests/src tis/host_ci/kotlin/stubs/androidx -type f -name '*.kt' -print0 | sort -z)
"$kotlin_home/bin/kotlinc" -J-Xmx4g -jvm-target 17 -Xfriend-paths="$production_dir" -classpath "$test_cp" -d "$test_dir" "${test_sources[@]}"
mapfile -t discovered_classes < <(find "$test_dir" -type f -name '*Test.class' -printf '%P\n' | sed 's#/#.#g; s#\.class$##' | sort)
test_classes=()
for c in "${discovered_classes[@]}"; do
  case "$c" in
    com.maleicacid.tvinput.tis.DirectBootGuardR51FixTest|com.maleicacid.tvinput.tis.ProviderDataAssetsR51ContractTest|com.maleicacid.tvinput.tis.RecordingDisabledR51Test|com.maleicacid.tvinput.tis.TvProviderWriterDescriptorSchemaTest) ;;
    *) test_classes+=("$c") ;;
  esac
done
runtime_cp="$production_dir:$test_dir:$android_jar:$junit_jar:$hamcrest_jar:$kotlin_home/lib/kotlin-stdlib.jar:$kotlin_home/lib/kotlin-stdlib-jdk7.jar:$kotlin_home/lib/kotlin-stdlib-jdk8.jar:$kotlin_home/lib/kotlin-test.jar:$kotlin_home/lib/kotlin-test-junit.jar"
java -Xmx4g -Djava.library.path=arib_si_engine_rs/host_ci/target/debug/deps -cp "$runtime_cp" org.junit.runner.JUnitCore "${test_classes[@]}" | tee "$RUNNER_TEMP/tis-junit-review54.log"
grep -F "OK (131 tests)" "$RUNNER_TEMP/tis-junit-review54.log"

python3 - <<'PY'
from pathlib import Path
from xml.etree import ElementTree
for p in sorted(Path('tis').rglob('*.xml')): ElementTree.parse(p)
scan=Path('tis/src/com/maleicacid/tvinput/tis/ScanPlan.kt').read_text()
assert 'TransportStreamId16(18803)' in scan
playback=Path('tis/src/com/maleicacid/tvinput/tis/PlaybackPipeline.kt').read_text()
for token in ('3 -> AudioFormat.CHANNEL_OUT_STEREO or AudioFormat.CHANNEL_OUT_FRONT_CENTER','4 -> AudioFormat.CHANNEL_OUT_QUAD','5 -> AudioFormat.CHANNEL_OUT_QUAD or AudioFormat.CHANNEL_OUT_FRONT_CENTER'):
    assert token in playback
manager=Path('tis/src/com/maleicacid/tvinput/tis/ChannelScanManager.kt').read_text()
assert 'committedServiceKeys.containsAll(requiredTargetKeys)' in manager
service=Path('arib_si_engine_rs/src/service_discovery.rs').read_text()
assert 'caption_timing' in service and 'matches!(timing, 0x00 | 0x02)' in service
session=Path('tis/src/com/maleicacid/tvinput/tis/MaleicacidLiveSession.kt').read_text()
assert 'captionSelectors' in session and 'setDescription' in session
print('review contract guards OK')
PY
git diff --check

git config user.name maleicacid
git config user.email 4982384+kazuki0824@users.noreply.github.com
git add -A
git diff --cached --check
git commit -m 'fix(tis): close ARIB review signaling gaps'
git push origin HEAD:"$TARGET_BRANCH"
