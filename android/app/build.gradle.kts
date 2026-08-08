plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "tokyo.runo.rssync"
    compileSdk = 35

    defaultConfig {
        applicationId = "tokyo.runo.rssync"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        // 2026-08-07追記: 「本体をこの端末で実行」機能
        // (`LocalServerConfig`)の追加に伴い、`jniLibs/{arm64-v8a,x86_64}/
        // librssync.so`(`cargo ndk`クロスビルド成果物)を同梱するように
        // なったため、ABIフィルタを明示(open-easy-web/open-web-server
        // 版と同じ2 ABI)。
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    // 2026-08-07追記: AGP既定の「APK内から直接実行(run-from-apk)」では
    // `applicationInfo.nativeLibraryDir`配下にファイルが展開されず、
    // `MainActivity.startLocalServerProcess()`が期待する実ファイルパスが
    // 存在しなくなる(open-easy-web/open-web-server側で先に踏んだ実バグと
    // 同一パターン)。旧来通りインストール時に展開される動作を強制する。
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        viewBinding = false
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
}
