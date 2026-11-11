// PIDA – Dateien in die App-Sandbox kopieren (iOS), EUPL-1.2.
//
// Eingehende Dokumente („Öffnen in“, Share, Dateien-App) und Dateien aus dem Dokument-Picker werden als
// **Kopie** in den Zielordner der Rust-Seite gelegt: `<destination>/<n>/<Name>` – ein eigener Unterordner
// je Datei, damit der Dateiname (und damit der Ausgabename `<Name>_signed.pdf`) unverändert bleibt.
// Security-Scoped URLs werden nur für die Dauer der Kopie geöffnet; Dateien eines File Providers
// (iCloud Drive …) liest `NSFileCoordinator`, der sie bei Bedarf erst herunterlädt.

import Foundation

struct CopiedFile: Encodable {
  let name: String
  let path: String
}

struct FilesResponse: Encodable {
  let files: [CopiedFile]
  /// gewählt, aber nicht kopiert (nicht lesbar, zu groß, keine Datei)
  var skipped: Int = 0
}

enum SandboxFiles {
  /// Höchstgröße einer Datei: die Grenze des Kerns (20 MB wie bei A-Trust, `MAX_DOCUMENT_BYTES`). Von einer
  /// größeren Datei kommen nur `maxBytes + 1` Bytes in die Sandbox – genug, damit der Kern sie wie überall als
  /// „größer als 20 MB“ ablehnt, ohne dass hunderte MB kopiert werden.
  static let maxBytes: Int64 = 20_000_000

  /// Wurzel der App-Sandbox (Container).
  static var containerRoot: URL {
    URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true).resolvingSymlinksInPath()
  }

  /// `Documents` der App (in der Dateien-App sichtbar, UIFileSharingEnabled).
  static var documents: URL {
    FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
  }

  /// `Documents/Inbox`: hier legt iOS Kopien ab, wenn eine Quelle kein Öffnen am Ort erlaubt (AirDrop,
  /// Mail-Anhänge).
  static var documentsInbox: URL {
    documents.appendingPathComponent("Inbox", isDirectory: true)
  }

  /// Ordner der importierten PKCS#12-Keystores des Kerns (`Paths::data_dir/keystores`; Tauri
  /// `app_data_dir` = `Library/Application Support/<Bundle-ID>`, `Paths::sandbox` → `data/`).
  static var importedKeystores: URL {
    let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    return support.appendingPathComponent(Bundle.main.bundleIdentifier ?? "at.asit.pida.app", isDirectory: true)
      .appendingPathComponent("data", isDirectory: true)
      .appendingPathComponent("keystores", isDirectory: true)
  }

  /// Importierte Keystores (privater Schlüssel, durch das Passwort geschützt) nicht in iCloud-/Geräte-
  /// Sicherungen: wie unter Android (`allowBackup=false`) und wie ihr Passwort in der Keychain
  /// (`…ThisDeviceOnly`) bleiben sie auf diesem Gerät. Der Ordner wird dafür vorab angelegt (0700); der
  /// Kern schreibt später hinein. Fehler nur im Log (ohne Pfad).
  static func excludeImportedKeystoresFromBackup() {
    var dir = importedKeystores
    do {
      try FileManager.default.createDirectory(
        at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
      var values = URLResourceValues()
      values.isExcludedFromBackup = true
      try dir.setResourceValues(values)
    } catch {
      NSLog("pida-mobile: Keystore-Ordner nicht von der Sicherung ausgenommen")
    }
  }

  /// Liegt `url` innerhalb von `dir` (nach Auflösen von Symlinks, ohne `..`)?
  static func isInside(_ url: URL, _ dir: URL) -> Bool {
    let path = url.standardizedFileURL.resolvingSymlinksInPath().path
    let root = dir.standardizedFileURL.resolvingSymlinksInPath().path
    return path != root && path.hasPrefix(root.hasSuffix("/") ? root : root + "/")
  }

  /// Zielordner der Rust-Seite: absolut und innerhalb der Sandbox; wird bei Bedarf angelegt.
  static func destination(_ path: String) throws -> URL {
    guard path.hasPrefix("/"), !path.split(separator: "/").contains("..") else {
      throw NativeError(code: "bad-destination")
    }
    let url = URL(fileURLWithPath: path, isDirectory: true)
    guard isInside(url, containerRoot) else { throw NativeError(code: "bad-destination") }
    do {
      try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    } catch {
      throw NativeError(code: "io-error")
    }
    return url
  }

  /// Dateiname ohne Pfadanteile und Steuerzeichen (wie `mobile::safe_name` der Shell).
  static func safeName(_ name: String, fallback: String = "Dokument.pdf") -> String {
    let mapped = String(
      name.unicodeScalars.map { scalar -> Character in
        if CharacterSet.controlCharacters.contains(scalar) || "/\\:\0".unicodeScalars.contains(scalar) {
          return "_"
        }
        return Character(scalar)
      })
    var trimmed = mapped.trimmingCharacters(in: .whitespacesAndNewlines)
    while trimmed.hasPrefix(".") { trimmed.removeFirst() }
    return trimmed.isEmpty ? fallback : trimmed
  }

  /// Freier Zielpfad `<dir>/<n>/<name>`.
  static func uniqueTarget(in dir: URL, name: String) -> URL {
    var n = 1
    while true {
      let candidate = dir.appendingPathComponent(String(n), isDirectory: true)
        .appendingPathComponent(name, isDirectory: false)
      if !FileManager.default.fileExists(atPath: candidate.path) { return candidate }
      n += 1
    }
  }

  /// Kopiert alle `sources` in ihrer Reihenfolge nach `destination` (je Datei ein eigener Unterordner, gleiche
  /// Namen überschreiben sich nicht); was sich nicht kopieren lässt, zählt als `skipped`. `after` läuft je Quelle
  /// nach dem Kopieren (z. B. Aufräumen der Picker-Kopie in tmp).
  static func copyAll(_ sources: [URL], to destination: URL, after: (URL) -> Void = { _ in }) -> FilesResponse {
    var files: [CopiedFile] = []
    var skipped = 0
    for url in sources {
      if let file = try? copyIn(url, to: destination) { files.append(file) } else { skipped += 1 }
      after(url)
    }
    if skipped > 0 { NSLog("pida-mobile: %d gewählte Datei(en) nicht übernommen", skipped) }
    return FilesResponse(files: files, skipped: skipped)
  }

  /// Kopiert `source` (auch Security-Scoped bzw. aus einem File Provider) nach `<destination>/<n>/<Name>`.
  static func copyIn(_ source: URL, to destination: URL) throws -> CopiedFile {
    let scoped = source.startAccessingSecurityScopedResource()
    defer { if scoped { source.stopAccessingSecurityScopedResource() } }

    var result: Result<CopiedFile, Error> = .failure(NativeError(code: "io-error"))
    var coordinationError: NSError?
    NSFileCoordinator(filePresenter: nil).coordinate(
      readingItemAt: source, options: [.withoutChanges], error: &coordinationError
    ) { readable in
      result = Result { try copyRegularFile(readable, displayName: source.lastPathComponent, to: destination) }
    }
    if coordinationError != nil { throw NativeError(code: "io-error") }
    return try result.get()
  }

  private static func copyRegularFile(_ source: URL, displayName: String, to destination: URL) throws -> CopiedFile {
    let values = try? source.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
    guard values?.isRegularFile == true else { throw NativeError(code: "not-a-file") }
    let tooLarge = values?.fileSize.map { Int64($0) > maxBytes } ?? false
    let name = safeName(displayName)
    let target = uniqueTarget(in: destination, name: name)
    let fm = FileManager.default
    do {
      try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
      if tooLarge {
        try copyPrefix(of: source, to: target, bytes: maxBytes + 1)
      } else {
        try fm.copyItem(at: source, to: target)
      }
    } catch {
      try? fm.removeItem(at: target.deletingLastPathComponent())
      throw NativeError(code: "io-error")
    }
    return CopiedFile(name: name, path: target.path)
  }

  /// Kopiert nur die ersten `bytes` Bytes (zu große Datei: der Kern lehnt sie an der Größe ab).
  private static func copyPrefix(of source: URL, to target: URL, bytes: Int64) throws {
    let input = try FileHandle(forReadingFrom: source)
    defer { try? input.close() }
    guard FileManager.default.createFile(atPath: target.path, contents: nil) else {
      throw NativeError(code: "io-error")
    }
    let output = try FileHandle(forWritingTo: target)
    defer { try? output.close() }
    var left = bytes
    while left > 0 {
      guard let chunk = try input.read(upToCount: Int(min(left, 1 << 20))), !chunk.isEmpty else { break }
      try output.write(contentsOf: chunk)
      left -= Int64(chunk.count)
    }
  }

  /// Entfernt Kopien in `Documents/Inbox`, die älter als `age` sind (die Rust-Seite liest solche Dateien
  /// direkt; ohne Aufräumen blieben sie in der Dateien-App sichtbar).
  static func purgeInbox(olderThan age: TimeInterval) {
    let fm = FileManager.default
    let keys: [URLResourceKey] = [.contentModificationDateKey, .creationDateKey]
    guard
      let items = try? fm.contentsOfDirectory(
        at: documentsInbox, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles])
    else { return }
    let limit = Date().addingTimeInterval(-age)
    for item in items {
      let values = try? item.resourceValues(forKeys: Set(keys))
      let date = values?.creationDate ?? values?.contentModificationDate ?? Date.distantPast
      if date < limit { try? fm.removeItem(at: item) }
    }
  }
}
