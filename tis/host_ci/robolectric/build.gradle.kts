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
            java.setSrcDirs(listOf("src/test/kotlin"))
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
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16.1")
}
