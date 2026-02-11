<script>
  import { app } from '$lib/app.svelte.js';
</script>

<svelte:head>
  <title>Datenschutzerklärung — Amberize</title>
</svelte:head>

<article class="legal">
  <h1>Datenschutzerklärung</h1>

  <div class="plain-intro">
    <p>
      <strong>Kurzfassung:</strong> Diese Website verwendet keine Cookies, kein Tracking und keine
      Analysetools. Die Desktop-App Amberize speichert alle Daten ausschließlich lokal auf Ihrem
      Computer — wir haben keine Server, an die Daten gesendet werden könnten, und wir wissen nicht
      einmal, dass Sie die App nutzen.
    </p>
  </div>

  <h2>1. Verantwortlicher</h2>
  <p>
    Johannes Mutter<br />
    <!-- TODO: Adresse eintragen -->
    [Straße und Hausnummer]<br />
    [PLZ Ort]<br />
    Deutschland<br />
    E-Mail: [TODO: E-Mail-Adresse]
  </p>

  <h2>2. Übersicht der Verarbeitungen</h2>
  <p>
    Die nachfolgende Übersicht fasst die Arten der verarbeiteten Daten und die Zwecke
    ihrer Verarbeitung zusammen.
  </p>
  <ul>
    <li><strong>Arten der verarbeiteten Daten:</strong> Nutzungsdaten (z.B. Zugriffszeiten, aufgerufene Seiten); Meta- und Kommunikationsdaten (z.B. IP-Adressen, Browserinformationen).</li>
    <li><strong>Betroffene Personen:</strong> Nutzer der Website.</li>
    <li><strong>Zwecke:</strong> Bereitstellung der Website; Sicherstellung des technischen Betriebs.</li>
    <li><strong>Rechtsgrundlagen:</strong> Berechtigte Interessen (Art. 6 Abs. 1 S. 1 lit. f DSGVO).</li>
  </ul>

  <h2>3. Hosting und Server-Log-Dateien</h2>
  <p>
    Diese Website wird bei einem externen Dienstleister gehostet. Beim Aufruf der Website
    erhebt der Hoster automatisch Informationen in Server-Log-Dateien, die Ihr Browser übermittelt:
  </p>
  <ul>
    <li>IP-Adresse</li>
    <li>Browsertyp und -version</li>
    <li>Betriebssystem</li>
    <li>Referrer URL</li>
    <li>Uhrzeit der Anfrage</li>
  </ul>
  <p>
    Diese Daten werden nicht mit anderen Datenquellen zusammengeführt. Die Erfassung erfolgt
    auf Grundlage von Art. 6 Abs. 1 lit. f DSGVO — der Betreiber hat ein berechtigtes Interesse
    an der technisch fehlerfreien Bereitstellung der Website. Server-Log-Dateien werden nach
    spätestens 30 Tagen gelöscht.
  </p>

  <h2>4. Cookies und Tracking</h2>
  <p>
    Diese Website verwendet keine Cookies, keine Analysetools (wie Google Analytics) und keine
    Tracking-Technologien. Es werden keine Nutzungsprofile erstellt.
  </p>

  <h2>5. TLS-Verschlüsselung</h2>
  <p>
    Diese Website nutzt aus Sicherheitsgründen TLS-Verschlüsselung (HTTPS). Eine verschlüsselte
    Verbindung erkennen Sie am Schloss-Symbol in der Adressleiste Ihres Browsers.
  </p>

  <h2>6. Desktop-App Amberize</h2>

  <h3>Lokale Datenverarbeitung</h3>
  <p>
    Die Desktop-Anwendung Amberize verarbeitet alle Daten ausschließlich lokal auf Ihrem Computer.
    Es werden keine Daten an Server des Entwicklers oder an Dritte übermittelt.
  </p>

  <h3>E-Mail-Zugangsdaten</h3>
  <p>
    Ihre E-Mail-Zugangsdaten (Benutzername und Passwort) werden ausschließlich im sicheren
    Anmeldedatenspeicher Ihres Betriebssystems gespeichert (macOS-Schlüsselbund, Windows
    Credential Manager bzw. Linux Secret Service / GNOME Keyring).
  </p>

  <h3>E-Mail-Archivierung</h3>
  <p>
    Archivierte E-Mails werden ausschließlich in einer lokalen Datenbank auf Ihrem Computer
    gespeichert. Die Verbindung zu Ihrem E-Mail-Server erfolgt direkt über verschlüsseltes
    TLS (IMAP/IMAPS). Es findet keine Übertragung an externe Server des Entwicklers statt.
  </p>

  <h3>Keine Telemetrie</h3>
  <p>
    Amberize sammelt keinerlei Nutzungsdaten, Analysen oder Telemetrie. Die Anwendung
    kommuniziert ausschließlich mit dem von Ihnen konfigurierten E-Mail-Server.
  </p>

  <h2>7. Ihre Rechte</h2>
  <p>
    Ihnen stehen als Betroffene nach der DSGVO folgende Rechte zu:
  </p>
  <ul>
    <li><strong>Auskunftsrecht</strong> (Art. 15 DSGVO): Sie können Auskunft über Ihre verarbeiteten Daten verlangen.</li>
    <li><strong>Berichtigung</strong> (Art. 16 DSGVO): Sie können die Berichtigung unrichtiger Daten verlangen.</li>
    <li><strong>Löschung</strong> (Art. 17 DSGVO): Sie können die Löschung Ihrer Daten verlangen.</li>
    <li><strong>Einschränkung</strong> (Art. 18 DSGVO): Sie können die Einschränkung der Verarbeitung verlangen.</li>
    <li><strong>Datenübertragbarkeit</strong> (Art. 20 DSGVO): Sie können Ihre Daten in einem gängigen Format erhalten.</li>
    <li><strong>Widerspruch</strong> (Art. 21 DSGVO): Sie können der Verarbeitung auf Grundlage berechtigter Interessen jederzeit widersprechen.</li>
  </ul>
  <p>
    Da Amberize alle Daten lokal auf Ihrem Gerät speichert, haben Sie jederzeit volle Kontrolle:
    Sie können die Archiv-Datenbank und alle zugehörigen Dateien jederzeit selbst löschen.
  </p>

  <h2>8. Beschwerderecht</h2>
  <p>
    Sie haben das Recht auf Beschwerde bei einer Datenschutz-Aufsichtsbehörde (Art. 77 DSGVO),
    insbesondere in dem Mitgliedstaat Ihres gewöhnlichen Aufenthalts, Ihres Arbeitsplatzes oder
    des Orts des mutmaßlichen Verstoßes.
  </p>

  <h2>9. Änderungen</h2>
  <p>
    Wir behalten uns vor, diese Datenschutzerklärung anzupassen, damit sie stets den aktuellen
    rechtlichen Anforderungen entspricht oder um Änderungen an der Anwendung umzusetzen.
  </p>
</article>

<style>
  .legal {
    max-width: 640px;
    margin: 0 auto;
    padding: 4rem 1.5rem;
  }
  .plain-intro {
    background: var(--color-amber-soft);
    border: 1px solid #f0e5c8;
    border-radius: 10px;
    padding: 1.25rem 1.5rem;
    margin-bottom: 2rem;
  }
  .plain-intro p {
    font-size: var(--text-base);
    color: var(--color-dark);
    line-height: 1.7;
    margin: 0;
  }
  .legal h1 {
    margin-bottom: 1.5rem;
  }
  .legal h2 {
    margin-top: 2rem;
    margin-bottom: 0.5rem;
  }
  .legal h3 {
    margin-top: 1.5rem;
    margin-bottom: 0.35rem;
  }
  .legal p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.7;
    margin-bottom: 0.75rem;
  }
  .legal ul {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.7;
    margin: 0.5rem 0 0.75rem 1.5rem;
  }
  .legal li {
    margin-bottom: 0.25rem;
  }
</style>
