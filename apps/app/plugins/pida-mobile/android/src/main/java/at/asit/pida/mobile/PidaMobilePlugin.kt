// PIDA – native Android-Seite des Plugins `pida-mobile` (EUPL-1.2), Paket M-Android.
//
// Vertrag (Argumente, Antworten, Fehlercodes): apps/app/plugins/pida-mobile/src/lib.rs und
// docs/integration/mobile.md §2; Android-Einzelheiten: docs/integration/mobile-android.md.
//
// - secureStore*: AES-GCM mit Schlüssel im Android Keystore ([SecureStore]).
// - shareFile/openFile: ACTION_SEND bzw. ACTION_VIEW über den eigenen FileProvider
//   (`${applicationId}.pida.files`, nur der Ausgabeordner der App) mit
//   FLAG_GRANT_READ_URI_PERMISSION, im System-Auswahldialog ohne PIDA selbst.
// - exportFile/importFile: Storage Access Framework (ACTION_CREATE_DOCUMENT / ACTION_OPEN_DOCUMENT);
//   keine Speicher-Berechtigungen. Import kopiert die **Bytes** über den ContentResolver nach
//   `destination` (Ordner der Sandbox, von der Shell vorgegeben).
// - takePendingOpenDocuments: Intents VIEW/SEND/SEND_MULTIPLE (content://) vormerken und zusammen mit
//   den übergebenen URIs nach `destination` kopieren ([PendingDocuments]: jede Übergabe genau einmal).
// - Lebenszyklus (onPause/onResume) meldet tao der Rust-Seite (WindowEvent::Suspended/Resumed); hier nur
//   die Vormerkung von Intents. TLS: rustls-platform-verifier initialisiert die Rust-Seite (src/tls.rs),
//   die Kotlin-Komponente kommt als Gradle-Abhängigkeit (build.gradle.kts).
//
// Befehle laufen auf dem UI-Thread (tauri `run_mobile_plugin` → Looper); Dateizugriffe und
// Verschlüsselung laufen auf einem eigenen Thread, die Antwort kommt von dort. Regeln: keine Pfade,
// URIs, Schlüssel oder Werte in Fehlermeldungen oder Logs (ADR 0001 Regel 9).

package at.asit.pida.mobile

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ComponentName
import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.plugin.PluginManager
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

@InvokeArg
class KeyArgs {
    lateinit var key: String
}

@InvokeArg
class SetArgs {
    lateinit var key: String
    lateinit var value: String
}

@InvokeArg
class ShareArgs {
    lateinit var path: String
    lateinit var mimeType: String
}

@InvokeArg
class ExportArgs {
    lateinit var path: String
    lateinit var fileName: String
    lateinit var mimeType: String
}

@InvokeArg
class ImportArgs {
    lateinit var kind: String
    var multiple: Boolean = false
    lateinit var destination: String
}

@InvokeArg
class PendingArgs {
    var urls: Array<String> = arrayOf()
    lateinit var destination: String
}

/** Fehler mit kurzem Code für `invoke.reject` (nie Pfade oder Werte). */
private class CodedException(val code: String) : Exception(code)

@TauriPlugin
class PidaMobilePlugin(private val activity: Activity) : Plugin(activity) {
    private val app: Context = activity.applicationContext
    private val io: ExecutorService = Executors.newSingleThreadExecutor { r -> Thread(r, "pida-mobile-io") }
    private val store by lazy { SecureStore(app) }
    private val pending = PendingDocuments()

    // Laufende Exporte/Importe je Aufruf (SAF-Ergebnis → Callback; nur UI-Thread)
    private val exportSources = HashMap<Long, File>()
    private val importRequests = HashMap<Long, ImportArgs>()

    init {
        // Kaltstart über „Öffnen mit“/„Teilen“: der Intent der Activity (MainActivity hat ihn geprüft und
        // nach einer Neuerstellung durch ACTION_MAIN ersetzt)
        remember(activity.intent)
    }

    /** Eingehende Dokumente (Intents VIEW/SEND/SEND_MULTIPLE application/pdf) vormerken. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        remember(intent)
    }

    private fun remember(intent: Intent?) {
        val uris = documentUris(intent ?: return)
        if (uris.isNotEmpty()) {
            Log.i(TAG, "Dokumente vorgemerkt: ${uris.size}")
            pending.remember(uris.map(Uri::toString))
        }
    }

    // --- sicherer Speicher -------------------------------------------------------------------------

    /** `{ key }` → `{ value: string | null }` */
    @Command
    fun secureStoreGet(invoke: Invoke) {
        val args = invoke.parseArgs(KeyArgs::class.java)
        background(invoke) {
            val value = store.get(args.key)
            invoke.resolve(JSObject().put("value", value ?: JSONObject.NULL))
        }
    }

    /** `{ key, value }` → `{}` */
    @Command
    fun secureStoreSet(invoke: Invoke) {
        val args = invoke.parseArgs(SetArgs::class.java)
        background(invoke) {
            store.set(args.key, args.value)
            invoke.resolve()
        }
    }

    /** `{ key }` → `{}` (fehlender Eintrag ist kein Fehler) */
    @Command
    fun secureStoreDelete(invoke: Invoke) {
        val args = invoke.parseArgs(KeyArgs::class.java)
        background(invoke) {
            store.delete(args.key)
            invoke.resolve()
        }
    }

    // --- Teilen, Öffnen, Exportieren ---------------------------------------------------------------

    /** `{ path, mimeType }` → `{}` sobald der Auswahldialog gezeigt wird (ACTION_SEND) */
    @Command
    fun shareFile(invoke: Invoke) {
        val args = invoke.parseArgs(ShareArgs::class.java)
        val uri = providerUri(invoke, args.path) ?: return
        val send = Intent(Intent.ACTION_SEND).apply {
            type = args.mimeType
            putExtra(Intent.EXTRA_STREAM, uri)
            clipData = ClipData.newRawUri(File(args.path).name, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startChooser(invoke, send)
    }

    /**
     * `{ path, mimeType }` → `{}` sobald die Anzeige-App startet (ACTION_VIEW, F-87 „Öffnen“). Android
     * öffnet keine file://-Pfade fremder Apps (FileUriExposedException) – deshalb über den FileProvider.
     */
    @Command
    fun openFile(invoke: Invoke) {
        val args = invoke.parseArgs(ShareArgs::class.java)
        val uri = providerUri(invoke, args.path) ?: return
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, args.mimeType)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        // nur fremde Apps (PIDA selbst nimmt PDFs per VIEW an); <queries> im Manifest des Plugins
        val others = app.packageManager.queryIntentActivities(view, 0).filter { it.activityInfo.packageName != app.packageName }
        if (others.isEmpty()) {
            invoke.reject("keine App zum Anzeigen", "no-app")
            return
        }
        startChooser(invoke, view)
    }

    /** FileProvider-URI einer Datei im freigegebenen Ausgabeordner, sonst `invoke.reject` und `null`. */
    private fun providerUri(invoke: Invoke, path: String): Uri? {
        val file = File(path)
        if (!file.isFile) {
            invoke.reject("Datei fehlt", "not-found")
            return null
        }
        return try {
            FileProvider.getUriForFile(app, shareAuthority(app), file)
        } catch (e: IllegalArgumentException) {
            // nicht im freigegebenen Ordner (res/xml/pida_share_paths.xml)
            invoke.reject("Datei nicht freigegeben", "not-shareable")
            null
        }
    }

    /** System-Auswahldialog ohne PIDA selbst (die eigene Ausgabe-URI würde MainActivity verwerfen). */
    private fun startChooser(invoke: Invoke, target: Intent) {
        val own = app.packageManager.queryIntentActivities(target, 0)
            .filter { it.activityInfo.packageName == app.packageName }
            .map { ComponentName(it.activityInfo.packageName, it.activityInfo.name) }
        val chooser = Intent.createChooser(target, null).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            if (own.isNotEmpty()) putExtra(Intent.EXTRA_EXCLUDE_COMPONENTS, own.toTypedArray())
        }
        try {
            currentActivity().startActivity(chooser)
            invoke.resolve()
        } catch (e: ActivityNotFoundException) {
            invoke.reject("keine passende App", "no-app")
        }
    }

    /** `{ path, fileName, mimeType }` → `{ saved: bool, name? }` nach der Wahl (SAF ACTION_CREATE_DOCUMENT) */
    @Command
    fun exportFile(invoke: Invoke) {
        val args = invoke.parseArgs(ExportArgs::class.java)
        val file = File(args.path)
        if (!file.isFile) {
            invoke.reject("Datei fehlt", "not-found")
            return
        }
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = args.mimeType
            putExtra(Intent.EXTRA_TITLE, Documents.safeName(args.fileName))
        }
        exportSources[invoke.id] = file
        try {
            startActivityForResult(invoke, intent, "exportResult")
        } catch (e: RuntimeException) {
            exportSources.remove(invoke.id)
            invoke.reject("Dokumentauswahl nicht verfügbar", "no-picker")
        }
    }

    @ActivityCallback
    fun exportResult(invoke: Invoke, result: ActivityResult) {
        val source = exportSources.remove(invoke.id)
        val target = result.data?.data
        if (result.resultCode != Activity.RESULT_OK || target == null || source == null) {
            invoke.reject("abgebrochen", "cancelled")
            return
        }
        background(invoke) {
            try {
                val out = app.contentResolver.openOutputStream(target, "wt") ?: throw CodedException("export-failed")
                out.use { o -> source.inputStream().use { i -> copyLimited(i, o) } }
            } catch (e: Exception) {
                // halb geschriebenes Dokument entfernen
                try {
                    DocumentsContract.deleteDocument(app.contentResolver, target)
                } catch (_: Exception) {
                }
                throw if (e is CodedException) e else CodedException("export-failed")
            }
            // tatsächlicher Name am Ziel (die Dateiauswahl nummeriert z. B. „Vertrag_signed (3).pdf“)
            val response = JSObject().put("saved", true)
            displayName(target)?.let { response.put("name", it) }
            invoke.resolve(response)
        }
    }

    /** `DISPLAY_NAME` eines Dokuments; `null`, wenn der Anbieter ihn nicht liefert. */
    private fun displayName(uri: Uri): String? =
        try {
            app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (c.moveToFirst() && n >= 0 && !c.isNull(n)) c.getString(n) else null
            }
        } catch (_: Exception) {
            null
        }

    // --- Import (Dateiauswahl) --------------------------------------------------------------------

    /** `{ kind: "pdf" | "pkcs12", multiple, destination }` → `{ files: [{ name, path }], skipped }` (Reihenfolge der Auswahl) */
    @Command
    fun importFile(invoke: Invoke) {
        val args = invoke.parseArgs(ImportArgs::class.java)
        val mimeTypes = when (args.kind) {
            "pdf" -> arrayOf("application/pdf")
            // .p12/.pfx: je nach Anbieter x-pkcs12, pkcs12 oder nur octet-stream; der Kern prüft das Format
            "pkcs12" -> arrayOf("application/x-pkcs12", "application/pkcs12", "application/octet-stream")
            else -> {
                invoke.reject("unbekannte Art", "bad-kind")
                return
            }
        }
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = if (mimeTypes.size == 1) mimeTypes[0] else "*/*"
            if (mimeTypes.size > 1) putExtra(Intent.EXTRA_MIME_TYPES, mimeTypes)
            putExtra(Intent.EXTRA_ALLOW_MULTIPLE, args.multiple)
        }
        importRequests[invoke.id] = args
        try {
            startActivityForResult(invoke, intent, "importResult")
        } catch (e: RuntimeException) {
            importRequests.remove(invoke.id)
            invoke.reject("Dateiauswahl nicht verfügbar", "no-picker")
        }
    }

    @ActivityCallback
    fun importResult(invoke: Invoke, result: ActivityResult) {
        val request = importRequests.remove(invoke.id)
        val data = result.data
        if (result.resultCode != Activity.RESULT_OK || data == null || request == null) {
            invoke.reject("abgebrochen", "cancelled")
            return
        }
        val uris = ArrayList<Uri>()
        data.clipData?.let { clip -> for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let(uris::add) }
        if (uris.isEmpty()) data.data?.let(uris::add)
        val limited = if (request.multiple) uris else uris.take(1)
        val isPdf = request.kind == "pdf"
        background(invoke) {
            // alle gewählten Dateien in der Reihenfolge der Auswahl; nicht kopierbare zählen als `skipped` (die
            // Oberfläche zeigt dazu einen Hinweis, ADR 0004 Nachtrag 17)
            val copied = copyAll(limited, File(request.destination), isPdf)
            invoke.resolve(JSObject().put("files", copied.first).put("skipped", copied.second))
        }
    }

    // --- eingegangene Dokumente -------------------------------------------------------------------

    /** `{ urls, destination }` → `{ files: [{ name, path }] }` */
    @Command
    fun takePendingOpenDocuments(invoke: Invoke) {
        val args = invoke.parseArgs(PendingArgs::class.java)
        // UI-Thread: Warteschlange und übergebene URIs abgleichen, dann im Hintergrund kopieren
        val uris = pending.take(args.urls.toList())
            .mapNotNull { runCatching { Uri.parse(it) }.getOrNull() }
            .filter(::acceptableSource)
        if (uris.isEmpty()) {
            invoke.resolve(JSObject().put("files", JSArray()))
            return
        }
        background(invoke) {
            val copied = copyAll(uris, File(args.destination), pdf = true)
            invoke.resolve(JSObject().put("files", copied.first).put("skipped", copied.second))
        }
    }

    // --- Hilfsfunktionen --------------------------------------------------------------------------

    /** Aktuelle Activity (nach einer Neuerstellung nicht mehr die beim Laden übergebene). */
    private fun currentActivity(): Activity = PluginManager.activity ?: activity

    /** Nur content://-URIs fremder Anbieter (nie die eigenen Provider, nie file://). */
    private fun acceptableSource(uri: Uri): Boolean {
        val authority = uri.authority ?: return false
        return uri.scheme == ContentResolver.SCHEME_CONTENT && !authority.startsWith(app.packageName)
    }

    /**
     * Kopiert alle lesbaren URIs in ihrer Reihenfolge nach `dest/<n>/<Name>` (je Datei ein eigener Unterordner, gleiche
     * Namen überschreiben sich nicht); nicht lesbare werden übersprungen und gezählt. → (Dateien, Zahl übersprungen)
     */
    private fun copyAll(uris: List<Uri>, dest: File, pdf: Boolean): Pair<JSArray, Int> {
        if (!dest.isDirectory && !dest.mkdirs()) throw CodedException("destination")
        val copied = Documents.copyEach(uris) { uri -> copyIn(uri, dest, pdf) }
        for (e in copied.failures) {
            Log.w(TAG, "Dokument nicht übernommen (${(e as? CodedException)?.code ?: e.javaClass.simpleName})")
        }
        val files = JSArray()
        for (copy in copied.files) files.put(JSObject().put("name", copy.name).put("path", copy.absolutePath))
        return Pair(files, copied.skipped)
    }

    private fun copyIn(uri: Uri, dest: File, pdf: Boolean): File {
        val resolver = app.contentResolver
        var name: String? = null
        try {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (n >= 0 && !c.isNull(n)) name = c.getString(n)
                }
            }
        } catch (_: Exception) {
            // Anbieter ohne OpenableColumns: Name aus der URI
        }
        var safe = Documents.safeName(name ?: uri.lastPathSegment, if (pdf) "Dokument.pdf" else "Datei")
        if (pdf) safe = Documents.withPdfExtension(safe)
        val target = Documents.uniqueTarget(dest, safe)
        if (!Documents.within(dest, target)) throw CodedException("bad-name")
        target.parentFile!!.mkdirs()
        try {
            val input = resolver.openInputStream(uri) ?: throw CodedException("unreadable")
            input.use { i -> target.outputStream().use { o -> copyLimited(i, o) } }
        } catch (e: Exception) {
            target.delete()
            target.parentFile?.delete()
            throw if (e is CodedException) e else CodedException("unreadable")
        }
        return target
    }

    /** Kopiert höchstens [Documents.MAX_BYTES] + 1 Bytes: Den Rest einer zu großen Datei braucht niemand. */
    private fun copyLimited(input: InputStream, output: OutputStream) {
        val buf = ByteArray(64 * 1024)
        var left = Documents.MAX_BYTES + 1
        while (left > 0) {
            val n = input.read(buf, 0, minOf(buf.size.toLong(), left).toInt())
            if (n < 0) break
            output.write(buf, 0, n)
            left -= n
        }
        output.flush()
    }

    /** Im Hintergrund ausführen; Ausnahmen werden zu `invoke.reject` mit kurzem Code. */
    private fun background(invoke: Invoke, work: () -> Unit) {
        io.execute {
            try {
                work()
            } catch (e: CodedException) {
                invoke.reject("pida-mobile: ${e.code}", e.code)
            } catch (e: SecureStoreException) {
                invoke.reject("pida-mobile: ${e.code}", e.code)
            } catch (e: IOException) {
                invoke.reject("pida-mobile: io", "io")
            } catch (e: Exception) {
                Log.w(TAG, "Fehler der nativen Seite (${e.javaClass.simpleName})")
                invoke.reject("pida-mobile: native", "native")
            }
        }
    }

    companion object {
        private const val TAG = "PIDA/Mobile"

        /** Authority des FileProviders für „Teilen“ (Manifest des Plugins). */
        fun shareAuthority(context: Context): String = "${context.packageName}.pida.files"

        /** Dokument-URIs eines Intents (VIEW: Daten; SEND/SEND_MULTIPLE: EXTRA_STREAM und ClipData). */
        @Suppress("DEPRECATION")
        fun documentUris(intent: Intent): List<Uri> {
            val action = intent.action
            if (action != Intent.ACTION_VIEW && action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) {
                return emptyList()
            }
            val out = LinkedHashSet<Uri>()
            intent.data?.let(out::add)
            intent.clipData?.let { clip -> for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let(out::add) }
            try {
                if (action == Intent.ACTION_SEND) {
                    val stream = if (Build.VERSION.SDK_INT >= 33) {
                        intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                    } else {
                        intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
                    }
                    stream?.let(out::add)
                } else if (action == Intent.ACTION_SEND_MULTIPLE) {
                    val streams = if (Build.VERSION.SDK_INT >= 33) {
                        intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
                    } else {
                        intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
                    }
                    streams?.let(out::addAll)
                }
            } catch (e: RuntimeException) {
                Log.w(TAG, "Intent-Extras nicht lesbar (${e.javaClass.simpleName})")
            }
            return out.filter { it.scheme == ContentResolver.SCHEME_CONTENT }
        }
    }
}

/**
 * FileProvider des Plugins für „Teilen“ (eigene Klasse, damit er neben dem FileProvider der App im
 * Manifest stehen kann); Pfade: res/xml/pida_share_paths.xml.
 */
class PidaFileProvider : FileProvider()
