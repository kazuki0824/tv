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

val androidAll = "org.robolectric:android-all:15-robolectric-13954326"

dependencies {
    compileOnly(androidAll)
    testCompileOnly(androidAll)
    testRuntimeOnly(androidAll)
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit:1.9.22")
    testImplementation("androidx.test:core:1.7.0")
    testImplementation("androidx.test.ext:junit:1.3.0")
    testImplementation("org.robolectric:robolectric:4.16.1")
}
