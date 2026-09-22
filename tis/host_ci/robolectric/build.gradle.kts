plugins {
    id("com.android.library") version "8.7.0"
    kotlin("android") version "1.9.22"
}

android {
    namespace = "com.maleicacid.tvinput"
    compileSdk = 35

    defaultConfig {
        minSdk = 31
    }

    sourceSets {
        getByName("main") {
            manifest.srcFile("../../AndroidManifest.xml")
            java.setSrcDirs(listOf("../../src"))
            res.setSrcDirs(listOf("../../res"))
            assets.setSrcDirs(listOf("../../tests/assets"))
        }
        getByName("test") {
            java.setSrcDirs(listOf("../../tests/src"))
            java.srcDir("src/test/kotlin")
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
    }
}

kotlin {
    jvmToolchain(17)
}

val androidAll = "org.robolectric:android-all:15-robolectric-13954326"

dependencies {
    compileOnly(androidAll)
    testCompileOnly(androidAll)
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit:1.9.22")
    testImplementation("androidx.test:core:1.7.0")
    testImplementation("androidx.test.ext:junit:1.3.0")
    testImplementation("org.robolectric:robolectric:4.16.1")
}


tasks.withType<Test>().configureEach {
    systemProperty("realTs.fixtureDirectory", file("../../tests/fixtures/real_ts").absolutePath)
    systemProperty("realTs.executable", file("../../../tuner_hal2/host_ci/target/debug/real_ts_sections").absolutePath)
    systemProperty("realTs.diagnosticsDirectory", layout.buildDirectory.dir("real-ts-diagnostics").get().asFile.absolutePath)
    systemProperty("java.library.path", file("../../../arib_si_engine_rs/host_ci/target/debug").absolutePath)
}
