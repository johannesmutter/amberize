# Verfahrensdokumentation — Amberize (GoBD) — Email Archivierung

Diese Verfahrensdokumentation unterstützt die GoBD-konforme Nachvollziehbarkeit.
Sie ersetzt keine steuerliche oder rechtliche Beratung. Für die inhaltliche Richtigkeit
und Vollständigkeit ist der Betreiber verantwortlich.

**Hinweis:** Bitte die markierten Platzhalter ausfüllen. Der Abschnitt
„Technische Systemdokumentation (automatisch)" wird durch die App aktualisiert.

---

## 1. Allgemeine Beschreibung (Allgemeine Verfahrensbeschreibung)

**Unternehmen / Steuerpflichtiger:** [Name / Firma]

**Steuernummer / USt-IdNr.:** [optional]

**Verantwortliche Person:** [Name, Rolle]

**Zweck des Systems:** Lokale, automatische Archivierung geschäftsrelevanter E-Mails
aus IMAP-Postfächern mit Integritätsnachweisen (tamper-evidence) und Exportfunktionen
für Prüfungssituationen.

**Geltungsbereich:** Archivierung eingehender und ausgehender geschäftlicher E-Mails
gemäß interner Richtlinie.

**Aufbewahrungsfristen:** Dokumenttypabhängig (typisch: 6/8/10 Jahre). Die konkrete
Einstufung erfolgt organisatorisch (z. B. nach steuerlicher Relevanz).

---

## 2. Anwenderdokumentation (Bedienung und Prozesse)

### 2.1 Einrichtung

- Archiv-Speicherort wählen (SQLite-Datei).
- IMAP-Konto hinzufügen (Zugangsdaten werden im Betriebssystem-Anmeldedatenspeicher gesichert — z. B. macOS Schlüsselbund, Windows Anmeldeinformationsverwaltung oder Linux Secret Service).
- Zu archivierende Ordner auswählen (mit sinnvollen Standardwerten).
- „Beim Anmelden im Hintergrund ausführen" aktivieren.

### 2.2 Laufender Betrieb

- Die App archiviert automatisch im Hintergrund.
- Im Fehlerfall zeigt die App einen Status an und versucht erneut.
- Bei Änderungen (neues Konto, Ordnerauswahl) wird die technische Dokumentation aktualisiert.

### 2.3 Export für Prüfung / Nachweise

- Einzelne Nachricht als `.eml` exportieren (Originalbytes).
- „Auditor-Paket" exportieren (Index, EML, Event-Log, Proof Snapshot, Verfahrensdokumentation).

---

## 3. Technische Systemdokumentation (automatisch)

<!-- BEGIN AUTO-GENERATED TECHNISCHE_SYSTEMDOKUMENTATION -->
Dieser Abschnitt wird automatisch generiert. Bitte die Marker nicht entfernen.
<!-- END AUTO-GENERATED TECHNISCHE_SYSTEMDOKUMENTATION -->

---

## 4. Betriebsdokumentation (Betrieb, Kontrollen, Backup)

### 4.1 Zugriffs- und Berechtigungskonzept

- Das System läuft im Benutzerkontext.
- Zugangsdaten liegen im Betriebssystem-Anmeldedatenspeicher (macOS Schlüsselbund, Windows Anmeldeinformationsverwaltung oder Linux Secret Service).
- Der Zugriff auf die Archivdatei ist durch Dateirechte geschützt.

### 4.2 Backup- und Wiederherstellungsprozess

**Backup-Strategie (Empfehlung):**

- Regelmäßiges Backup der Archivdatei aktivieren (z. B. Time Machine, Windows-Sicherung, rsync o. Ä.).
- Regelmäßig einen Restore-Test durchführen (Stichprobe).

**Restore-Test (Checkliste):**

1. Kopie der Archivdatei aus Backup wiederherstellen.
2. Integritätsprüfung in der App ausführen.
3. Stichproben: Suche und Export einer Nachricht.

### 4.3 Änderungen / Versionierung

- Änderungen an der Software werden über Releases dokumentiert.
- Die App führt ein tamper-evidentes Event-Log und erstellt Proof Snapshots.
