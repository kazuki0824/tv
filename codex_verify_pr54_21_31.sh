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
python3 "$STAGING_DIR/codex_apply_pr54_31_fixed3.py"
python3 "$STAGING_DIR/codex_apply_pr54_21.py"
git diff --check
git status --short

cargo +"$RUST_TOOLCHAIN" fmt --manifest-path arib_si_engine_rs/host_ci/Cargo.toml -- --check
cargo +"$RUST_TOOLCHAIN" check --locked --manifest-path arib_si_engine_rs/host_ci/Cargo.toml
cargo +"$RUST_TOOLCHAIN" test --locked --manifest-path arib_si_engine_rs/host_ci/Cargo.toml
python3 - <<'PY'
import json
from pathlib import Path
for path in sorted(Path('arib_si_engine_rs/schema').glob('*.json')):
    json.loads(path.read_text())
for path in sorted(Path('arib_si_engine_rs/testdata').rglob('*.json')):
    json.loads(path.read_text())
print('JSON parse OK')
PY

tools_dir="$RUNNER_TEMP/tis-kotlin-test"
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
production_dir="$RUNNER_TEMP/tis-kotlin-production"
test_dir="$RUNNER_TEMP/tis-kotlin-tests"
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
test "${#discovered_classes[@]}" -eq 30
test "${#test_classes[@]}" -eq 26
runtime_cp="$production_dir:$test_dir:$android_jar:$junit_jar:$hamcrest_jar:$kotlin_home/lib/kotlin-stdlib.jar:$kotlin_home/lib/kotlin-stdlib-jdk7.jar:$kotlin_home/lib/kotlin-stdlib-jdk8.jar:$kotlin_home/lib/kotlin-test.jar:$kotlin_home/lib/kotlin-test-junit.jar"
java -Xmx4g -Djava.library.path=arib_si_engine_rs/host_ci/target/debug/deps -cp "$runtime_cp" org.junit.runner.JUnitCore "${test_classes[@]}" | tee "$RUNNER_TEMP/tis-junit.log"
grep -F "OK (131 tests)" "$RUNNER_TEMP/tis-junit.log"

python3 - <<'PY'
from pathlib import Path
from xml.etree import ElementTree
paths=sorted(Path('tis').rglob('*.xml')); assert paths
for p in paths: ElementTree.parse(p)
doc=Path('ARIB_SI_EPG_TvProvider投影方針.md').read_text()
for x in ('TvTrackInfo.Builder','setAudioSampleRate(','setHardOfHearing(','Tuner.scan(','onInputStreamIdsReported'):
    assert x not in doc, x
print('XML/projection scope OK')
PY
git diff --check

git config user.name maleicacid
git config user.email 4982384+kazuki0824@users.noreply.github.com
git add -A
git diff --cached --check
git commit -m 'fix(tis): align BS discovery and ARIB audio metadata'
git push origin HEAD:"$TARGET_BRANCH"
