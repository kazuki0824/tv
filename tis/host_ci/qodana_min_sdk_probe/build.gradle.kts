plugins {
    id("com.android.library") version "8.7.0"
    kotlin("android") version "1.9.22"
}

android {
    namespace = "com.maleicacid.tvinput"
    compileSdk = 35

    defaultConfig {
        minSdk = 35
    }
}

kotlin {
    jvmToolchain(17)
}

val androidAll = "org.robolectric:android-all:15-robolectric-13954326"

dependencies {
    compileOnly(androidAll)
}
