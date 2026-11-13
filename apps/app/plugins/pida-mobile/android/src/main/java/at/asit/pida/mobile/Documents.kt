// PIDA – Dateinamen und Eingangs-Warteschlange der Android-Seite (EUPL-1.2).
//
// Reine Logik ohne Android-Klassen (JVM-Unit-Tests: src/test). Die Regeln entsprechen der Rust-Shell
// (apps/app/src-tauri/src/mobile.rs: `safe_name`, `unique_target`, `MAX_DOCUMENT_BYTES`).

package at.asit.pida.mobile

import java.io.File

object Documents {
    /**
     * Höchstgröße einer übernommenen Datei: die Grenze des Kerns (20 MB wie bei A-Trust, `MAX_DOCUMENT_BYTES`).
     * Von einer größeren Datei kommen nur [MAX_BYTES] + 1 Bytes in die Sandbox – genug, damit der Kern sie wie
     * überall als „größer als 20 MB“ ablehnt, ohne dass hunderte MB kopiert werden.
     */
    const val MAX_BYTES: Long = 20_000_000L

    /** Dateiname ohne Pfadanteile und Steuerzeichen; leer → `fallback`. */
    fun safeName(name: String?, fallback: String = "Dokument.pdf"): String {
        val cleaned = (name ?: "").map { c ->
            if (c.isISOControl() || c == '/' || c == '\\' || c == ':' || c == '\u0000') '_' else c
        }.joinToString("")
        val trimmed = cleaned.trim().trimStart('.')
        return if (trimmed.isEmpty()) fallback else trimmed
    }

    /** PDF ohne Endung (manche Anbieter liefern nur „document“) → `.pdf` anhängen. */
    fun withPdfExtension(name: String): String =
        if (name.lowercase().endsWith(".pdf")) name else "$name.pdf"

    /**
     * Freier Zielpfad `dir/<n>/<name>` – eigener Unterordner je Datei, damit der Name (und der Ausgabename
     * `<name>_signed.pdf`) unverändert bleibt.
     */
    fun uniqueTarget(dir: File, name: String): File {
        val safe = safeName(name)
        var n = 1
        while (true) {
            val candidate = File(File(dir, n.toString()), safe)
            if (!candidate.exists() && !candidate.parentFile!!.exists()) return candidate
            n++
        }
    }

    /**
     * Mehrfachauswahl (ADR 0004 Nachtrag 17): kopiert alle `sources` in ihrer Reihenfolge; was `copy` nicht
     * übernehmen kann (Ausnahme), zählt als nicht übernommen – die Oberfläche zeigt dazu einen Hinweis.
     */
    fun <S, T> copyEach(sources: List<S>, copy: (S) -> T): Copied<T> {
        val copied = ArrayList<T>(sources.size)
        val failures = ArrayList<Exception>()
        for (source in sources) {
            try {
                copied.add(copy(source))
            } catch (e: Exception) {
                failures.add(e)
            }
        }
        return Copied(copied, failures)
    }

    /** Liegt `file` innerhalb von `dir` (nach Auflösen von `..` und Symlinks)? */
    fun within(dir: File, file: File): Boolean {
        val d = dir.canonicalFile
        val f = file.canonicalFile
        return f.path.startsWith(d.path + File.separator)
    }
}

/** Ergebnis von [Documents.copyEach]: Kopien in der Reihenfolge der Auswahl und die Fehler der nicht übernommenen. */
data class Copied<T>(val files: List<T>, val failures: List<Exception>) {
    val skipped: Int get() = failures.size
}

/**
 * Eingegangene Dokument-URIs (Intents VIEW/SEND/SEND_MULTIPLE).
 *
 * Jede Übergabe soll **genau einmal** ankommen. Dieselbe URI erreicht die native Seite auf zwei Wegen: als
 * vorgemerkter Intent ([remember] aus Konstruktor bzw. `onNewIntent`) und – über tao →
 * `RunEvent::Opened` → Shell – als Argument von `takePendingOpenDocuments`. Die Reihenfolge der Aufrufe
 * (Start, Rückkehr in den Vordergrund, `Opened`) ist nicht festgelegt. Regel: URIs aus Intents, die die
 * native Seite selbst gesehen hat, liefert nur die Warteschlange; übergebene URIs nur, wenn sie hier unbekannt
 * sind. Nicht threadsicher – nur auf dem UI-Thread benutzen (Intents und Plugin-Befehle laufen dort).
 */
class PendingDocuments(private val maxSeen: Int = 256) {
    private val pending = LinkedHashSet<String>()
    private val seen = LinkedHashSet<String>()

    /** URIs eines eingegangenen Intents vormerken. */
    fun remember(uris: List<String>) {
        for (u in uris) {
            pending.add(u)
            markSeen(u)
        }
    }

    /** Was jetzt zu kopieren ist (Warteschlange + unbekannte übergebene URIs); leert die Warteschlange. */
    fun take(passed: List<String>): List<String> {
        val out = LinkedHashSet<String>(pending)
        pending.clear()
        for (u in passed) {
            if (u !in seen) {
                out.add(u)
                markSeen(u)
            }
        }
        return out.toList()
    }

    private fun markSeen(u: String) {
        seen.remove(u)
        seen.add(u)
        while (seen.size > maxSeen) {
            seen.remove(seen.first())
        }
    }
}
