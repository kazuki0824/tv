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
            java.include(
                "com/maleicacid/tvinput/tis/DirectBootGuardR51FixTest.kt",
                "com/maleicacid/tvinput/tis/RecordingDisabledR51Test.kt",
                "com/maleicacid/tvinput/tis/TvProviderWriterDescriptorSchemaTest.kt",
            )
        }
    }

    testOptions {
        unitTests.isIncludeAndroidResources = true
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    compileOnly("org.robolectric:android-all:15-robolectric-13954326")
    testImplementation("junit:junit:4.13.2")
    testImplementation("androidx.test:core:1.7.0")
    testImplementation("androidx.test.ext:junit:1.3.0")
    testImplementation("org.robolectric:robolectric:4.16.1")
}
