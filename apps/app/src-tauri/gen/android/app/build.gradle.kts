// PIDA – Android-App (EUPL-1.2). Erzeugt mit `tauri android init` (Tauri 2.12), angepasst für
// Paket M-Android: docs/integration/mobile-android.md.
import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// PDFium (bblanchon/pdfium-binaries chromium/8100, SHA-256 in tools/pdfium.lock): `tools/fetch-pdfium.sh
// --jnilibs` legt libpdfium.so je ABI unter tools/pdfium/android/jniLibs ab. Der Kern lädt sie zur
// Laufzeit mit dlopen("libpdfium.so") aus nativeLibraryDir. Anderer Ordner: -Ppida.pdfiumJniLibs=<pfad>.
val pdfiumJniLibs: File = (findProperty("pida.pdfiumJniLibs") as String?)?.let { file(it) }
    ?: rootDir.resolve("../../../../../tools/pdfium/android/jniLibs")
if (!pdfiumJniLibs.resolve("arm64-v8a/libpdfium.so").isFile) {
    logger.warn(
        "PIDA: PDFium fehlt unter $pdfiumJniLibs (tools/fetch-pdfium.sh --jnilibs) – " +
            "die App startet dann ohne Vorschau (automatische Position)."
    )
}

// Klartext-HTTP (Plan §7.7): nie im Release; im Debug-Build nur für `tauri android dev` (Dev-Server
// im LAN) mit PIDA_ANDROID_DEV_CLEARTEXT=1.
val devCleartext = System.getenv("PIDA_ANDROID_DEV_CLEARTEXT") == "1"

// Release-Signatur (docs/integration/mobile-android.md §4.1): Der Schlüssel liegt nie im Repository.
// PIDA_ANDROID_KEYSTORE_PROPERTIES=<absoluter Pfad> nennt eine Properties-Datei außerhalb mit storeFile
// (absolut oder relativ zu dieser Datei), storePassword, keyAlias, keyPassword und optional storeType;
// damit wird der Release-Build signiert. Ohne Variable bleibt alles wie bisher (Release unsigniert, Debug
// mit dem Debug-Schlüssel). Fehlermeldungen nennen nur Schlüsselnamen, nie Werte.
val releaseSigning: Properties? = System.getenv("PIDA_ANDROID_KEYSTORE_PROPERTIES")
    ?.takeIf { it.isNotBlank() }
    ?.let { path ->
        val propsFile = File(path)
        if (!propsFile.isAbsolute || !propsFile.isFile) {
            throw GradleException(
                "PIDA_ANDROID_KEYSTORE_PROPERTIES: keine Datei unter diesem absoluten Pfad"
            )
        }
        val props = Properties().apply { propsFile.inputStream().use { load(it) } }
        val missing = listOf("storeFile", "storePassword", "keyAlias", "keyPassword")
            .filter { props.getProperty(it).isNullOrBlank() }
        if (missing.isNotEmpty()) {
            throw GradleException(
                "PIDA_ANDROID_KEYSTORE_PROPERTIES: es fehlt ${missing.joinToString()}"
            )
        }
        val store = File(props.getProperty("storeFile").trim())
            .let { if (it.isAbsolute) it else propsFile.absoluteFile.parentFile.resolve(it) }
        if (!store.isFile) {
            throw GradleException("PIDA_ANDROID_KEYSTORE_PROPERTIES: storeFile nicht gefunden")
        }
        props.setProperty("storeFile", store.absolutePath)
        props
    }

android {
    // compileSdk 36: die Tauri-API-Bibliothek (.tauri/tauri-api) verlangt 36.
    compileSdk = 36
    namespace = "at.asit.pida.app"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        // Kennung at.asit.pida.app = Android-Paket = Paketname bei Google Play (seit 0.6.0; vorher at.asit.pida, at.pida).
        applicationId = "at.asit.pida.app"
        // Plan §5.1/E-15: Android 9 (API 28)
        minSdk = 28
        // Google Play verlangt für neue Apps und Updates API 36 (Android 16)
        targetSdk = 36
        // tauri.properties (von der CLI aus `version` in tauri.conf.json erzeugt): 0.8.1 → versionCode
        // 0 · 1000000 + 8 · 1000 + 1 = 8001; die Rückfallwerte entsprechen dem, falls die Datei fehlt
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "8001").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "0.8.1")
    }
    sourceSets {
        getByName("main") {
            jniLibs.srcDir(pdfiumJniLibs)
        }
    }
    signingConfigs {
        releaseSigning?.let { props ->
            create("release") {
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
                props.getProperty("storeType")?.takeIf { it.isNotBlank() }?.let { storeType = it }
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = if (devCleartext) "true" else "false"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (releaseSigning != null) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
