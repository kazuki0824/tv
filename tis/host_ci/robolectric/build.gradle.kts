plugins {
    id("com.android.library") version "8.7.0"
    kotlin("android") version "1.9.22"
}

val qodanaConsumerSources =
    if (System.getenv("QODANA_INCLUDE_REC_CONSUMERS") == "1") {
        listOf("../../../rec/src")
    } else {
        emptyList()
    }

android {
    namespace = "com.maleicacid.tvinput"
    compileSdk = 35

    defaultConfig {
        minSdk = 35
    }

    sourceSets {
        getByName("main") {
            manifest.srcFile("../../AndroidManifest.xml")
            java.setSrcDirs(listOf("../../src") + qodanaConsumerSources)
            res.setSrcDirs(listOf("../../res"))
            assets.setSrcDirs(listOf("../../tests/assets"))
        }
        getByName("test") {
            java.setSrcDirs(listOf("../../tests/src", "src/test/kotlin"))
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
        unitTests.all {
            it.testLogging.exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
        }
    }
}

kotlin {
    jvmToolchain(17)
}

// 全試験がSDK 35固定。Robolectric独自のMaven取得を避け、Gradleで事前解決する。
val robolectricSdkVersion = "15-robolectric-13954326"
val androidAll = "org.robolectric:android-all:$robolectricSdkVersion"
val robolectricSdk =
    configurations.create("robolectricSdk") {
        isCanBeConsumed = false
        isCanBeResolved = true
    }

dependencies {
    add("robolectricSdk", "org.robolectric:android-all-instrumented:$robolectricSdkVersion-i7")
    compileOnly(androidAll)
    testCompileOnly(androidAll)
    testRuntimeOnly(androidAll)
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit:1.9.22")
    testImplementation("androidx.test:core:1.7.0")
    testImplementation("androidx.test.ext:junit:1.3.0")
    testImplementation("org.robolectric:robolectric:4.16.1")
}

// Robolectric 4.16.1が要求するinstrumented SDKをGradle側で取得し、
// テスト実行中のpom.sha512再取得（Robolectric独自resolver）に依存しない。
val prepareRobolectricSdk by tasks.registering(Sync::class) {
    from(robolectricSdk)
    into(layout.buildDirectory.dir("robolectric-sdk"))
}

tasks.withType<Test>().configureEach {
    dependsOn(prepareRobolectricSdk)
    systemProperty("robolectric.offline", "true")
    systemProperty(
        "robolectric.dependency.dir",
        layout.buildDirectory
            .dir("robolectric-sdk")
            .get()
            .asFile
            .absolutePath,
    )
}
