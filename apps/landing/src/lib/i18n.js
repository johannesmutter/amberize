/**
 * OS-specific term sets for each locale.
 * Used as placeholder replacements in translation strings.
 * @type {Record<string, Record<string, Record<string, string>>>}
 */
export const os_terms = {
  en: {
    mac:     { os_name: 'macOS',   os_short: 'Mac',     device: 'your Mac',      tray: 'menu bar',    credential_store: 'macOS Keychain' },
    windows: { os_name: 'Windows', os_short: 'Windows', device: 'your PC',       tray: 'system tray', credential_store: 'Windows Credential Manager' },
    linux:   { os_name: 'Linux',   os_short: 'Linux',   device: 'your computer', tray: 'system tray', credential_store: 'system keyring' },
  },
  de: {
    mac:     { os_name: 'macOS',   os_short: 'Mac',     device: 'Ihrem Mac',     tray: 'Menüleiste',   credential_store: 'macOS-Schlüsselbund' },
    windows: { os_name: 'Windows', os_short: 'Windows', device: 'Ihrem PC',      tray: 'Taskleiste',   credential_store: 'Windows-Tresor' },
    linux:   { os_name: 'Linux',   os_short: 'Linux',   device: 'Ihrem Rechner', tray: 'Systemleiste', credential_store: 'System-Schlüsselbund' },
  }
};

/** @type {Record<string, Record<string, string>>} */
export const translations = {
  en: {
    // Nav
    nav_features: 'Features',
    nav_gobd: 'GoBD',
    nav_download: 'Download',

    // Hero
    hero_eyebrow: 'Free email archiving for {os_short}',
    hero_title: 'Your emails, safely archived\nfor the next tax audit',
    hero_sub: 'Amberize saves your business emails to {device} \u2014 searchable, tamper-proof, and ready when the Finanzamt asks. No cloud. No subscription. No IT department needed.',
    hero_cta_download: 'Download for {os_name}',
    hero_cta_how: 'See how it works',
    mac_apple_silicon: 'Apple Silicon (M1\u2013M4)',
    mac_intel: 'Intel Mac',

    // Trust
    trust: 'Built for freelancers, small businesses, and tax advisors in Germany. Open source and free forever.',

    // Features
    feature_1_title: 'Audit-ready archive',
    feature_1_desc: 'Every email is saved exactly as received and protected by SHA-256 hashes. Any change is detected immediately.',
    feature_2_title: 'Find any email instantly',
    feature_2_desc: "Search through years of emails in seconds. No scrolling through folders \u2014 just type what you're looking for.",
    feature_3_title: 'Multiple accounts, one archive',
    feature_3_desc: 'Connect Gmail, Outlook, or any IMAP mailbox \u2014 all in one place. Your entire archive is a single portable file. Copy it to an external drive, put it in your safe, or hand it to your Steuerberater.',
    feature_4_title: 'Made for German tax law',
    feature_4_desc: 'Generates the Verfahrensdokumentation your auditor needs. Export a ready-made audit package with one click.',
    feature_5_title: 'Private and secure by design',
    feature_5_desc: 'Everything runs on {device} \u2014 no cloud, no account, no tracking. Your email password stays in your {credential_store}. Fully open source on GitHub.',
    feature_6_title: 'Set it and forget it',
    feature_6_desc: "Sits quietly in your {tray} and archives new emails every few minutes. You don't have to do anything.",

    // How it works
    how_title: 'Up and running in 2 minutes',
    step_1_title: 'Pick a location',
    step_1_desc: 'Choose where to save your archive \u2014 your Documents folder, an external drive, wherever you like.',
    step_2_title: 'Connect your email',
    step_2_desc: 'Enter your email account details or sign in with Google. Pick which folders to archive.',
    step_3_title: "That's it",
    step_3_desc: 'Amberize archives new emails automatically. When your auditor asks, export everything in one click.',

    // Compliance
    compliance_title: 'Supports GoBD-compliant archiving',
    compliance_intro: "German tax law (GoBD) requires businesses to archive emails for up to 10 years in an unalterable, searchable format. Here's how Amberize covers the technical requirements.",
    compliance_1_label: 'Emails stored unaltered',
    compliance_1_term: 'Unver\u00e4nderbarkeit',
    compliance_1_detail: 'Original messages are saved exactly as received and protected by SHA-256 hashes. Any tampering is detected.',
    compliance_2_label: 'Automatic archiving',
    compliance_2_term: 'Vollst\u00e4ndigkeit',
    compliance_2_detail: 'All emails in your selected folders are archived automatically every few minutes. Important: emails deleted from the server before the next sync cannot be captured.',
    compliance_3_label: 'Complete audit trail',
    compliance_3_term: 'Nachvollziehbarkeit',
    compliance_3_detail: 'Every action is logged in a tamper-evident, hash-chained event log you can hand to an auditor.',
    compliance_4_label: 'Searchable and exportable',
    compliance_4_term: 'Maschinelle Auswertbarkeit',
    compliance_4_detail: 'Full-text search plus export as .eml files or a complete audit package (ZIP).',

    // Limitations
    limits_title: 'What you need to know',
    limits_intro: "Full GoBD compliance also requires organizational measures \u2014 no software handles that alone. Here's what stays your responsibility.",
    limit_1_title: 'Keep Amberize running',
    limit_1_desc: "Amberize can only archive emails while it's running. Let it start automatically and run in the background \u2014 it uses minimal resources. Emails deleted from your server before the next sync cannot be captured.",
    limit_2_title: 'Back up your archive',
    limit_2_desc: "Amberize doesn't enforce 6- or 10-year retention periods automatically. Keep regular backups of your archive file \u2014 on a separate drive, in a safe, or with your Steuerberater.",

    // About
    about_title: 'Why I built Amberize',
    about_text: "I'm Johannes Mutter \u2014 a freelance designer and engineer in Germany. Like many Selbst\u00e4ndige, I have email accounts with multiple providers and couldn't find a GoBD archiving solution that was simple, reliable, and didn't force me into an enterprise contract or lock me to a single provider. So I built Amberize.",
    about_github: 'GitHub',
    about_website: 'mutter.co',
    about_twitter: 'Twitter',

    // Platform availability
    also_available: 'Also available for',
    platform_and: 'and',

    // CTA
    cta_title: 'Get your email archiving sorted',
    cta_sub: 'Download Amberize, connect your email, and have one less thing to worry about when the Finanzamt calls.',
    cta_button: 'Download for {os_name}',
    cta_note: 'Free and open source. No account needed.',
    cta_downloads: '{count}+ downloads',

    // Footer
    footer_text: 'Amberize is free, open-source software (MIT license).',
    footer_github: 'View on GitHub',
    footer_impressum: 'Impressum',
    footer_datenschutz: 'Privacy Policy',
    footer_disclaimer: 'Amberize supports GoBD-compliant archiving but is not certified by an auditor (IDW PS 880). GoBD compliance requires organizational measures in addition to software. This page does not constitute legal or tax advice. Consult your Steuerberater.'
  },

  de: {
    // Nav
    nav_features: 'Funktionen',
    nav_gobd: 'GoBD',
    nav_download: 'Download',

    // Hero
    hero_eyebrow: 'Kostenlose E-Mail-Archivierung f\u00fcr {os_short}',
    hero_title: 'Ihre E-Mails, sicher archiviert\nf\u00fcr die n\u00e4chste Betriebspr\u00fcfung',
    hero_sub: 'Amberize speichert Ihre gesch\u00e4ftlichen E-Mails auf {device} \u2014 durchsuchbar, manipulationssicher und bereit, wenn das Finanzamt fragt. Keine Cloud. Kein Abo. Keine IT-Abteilung n\u00f6tig.',
    hero_cta_download: 'F\u00fcr {os_name} herunterladen',
    hero_cta_how: 'So funktioniert es',
    mac_apple_silicon: 'Apple Silicon (M1\u2013M4)',
    mac_intel: 'Intel Mac',

    // Trust
    trust: 'Entwickelt f\u00fcr Freiberufler, kleine Unternehmen und Steuerberater in Deutschland. Open Source und dauerhaft kostenlos.',

    // Features
    feature_1_title: 'Pr\u00fcfungsbereites Archiv',
    feature_1_desc: 'Jede E-Mail wird exakt so gespeichert, wie sie empfangen wurde, und durch SHA-256-Hashes gesch\u00fctzt. Jede \u00c4nderung wird sofort erkannt.',
    feature_2_title: 'Jede E-Mail sofort finden',
    feature_2_desc: 'Durchsuchen Sie Jahre an E-Mails in Sekunden. Kein Ordner-Durchklicken \u2014 einfach eintippen.',
    feature_3_title: 'Mehrere Konten, ein Archiv',
    feature_3_desc: 'Verbinden Sie Gmail, Outlook oder jedes IMAP-Postfach \u2014 alles an einem Ort. Ihr gesamtes Archiv ist eine einzige portable Datei. Kopieren Sie sie auf eine externe Festplatte, in den Tresor oder geben Sie sie Ihrem Steuerberater.',
    feature_4_title: 'F\u00fcr deutsches Steuerrecht gemacht',
    feature_4_desc: 'Erstellt automatisch die Verfahrensdokumentation, die Ihr Pr\u00fcfer braucht. Pr\u00fcferpaket per Klick exportieren.',
    feature_5_title: 'Privat und sicher \u2014 von Grund auf',
    feature_5_desc: 'L\u00e4uft komplett auf {device} \u2014 keine Cloud, kein Konto, kein Tracking. Ihr E-Mail-Passwort bleibt im {credential_store}. Vollst\u00e4ndig Open Source auf GitHub.',
    feature_6_title: 'Einrichten und vergessen',
    feature_6_desc: 'Sitzt leise in der {tray} und archiviert neue E-Mails alle paar Minuten. Sie m\u00fcssen nichts weiter tun.',

    // How it works
    how_title: 'In 2 Minuten startklar',
    step_1_title: 'Speicherort w\u00e4hlen',
    step_1_desc: 'W\u00e4hlen Sie, wo Ihr Archiv gespeichert werden soll \u2014 Dokumente-Ordner, externe Festplatte, ganz wie Sie m\u00f6chten.',
    step_2_title: 'E-Mail-Konto verbinden',
    step_2_desc: 'Geben Sie Ihre Kontodaten ein oder melden Sie sich mit Google an. W\u00e4hlen Sie die zu archivierenden Ordner.',
    step_3_title: 'Fertig',
    step_3_desc: 'Amberize archiviert neue E-Mails automatisch. Wenn der Pr\u00fcfer fragt, exportieren Sie alles mit einem Klick.',

    // Compliance
    compliance_title: 'Unterst\u00fctzt GoBD-konforme Archivierung',
    compliance_intro: 'Das deutsche Steuerrecht (GoBD) verlangt, dass Unternehmen E-Mails bis zu 10 Jahre lang unver\u00e4nderbar und durchsuchbar aufbewahren. So deckt Amberize die technischen Anforderungen ab.',
    compliance_1_label: 'E-Mails unver\u00e4ndert gespeichert',
    compliance_1_term: 'Unver\u00e4nderbarkeit',
    compliance_1_detail: 'Originalnachrichten werden exakt wie empfangen gespeichert und durch SHA-256-Hashes gesch\u00fctzt. Manipulationen werden erkannt.',
    compliance_2_label: 'Automatische Archivierung',
    compliance_2_term: 'Vollst\u00e4ndigkeit',
    compliance_2_detail: 'Alle E-Mails in Ihren ausgew\u00e4hlten Ordnern werden automatisch alle paar Minuten archiviert. Wichtig: E-Mails, die vor dem n\u00e4chsten Sync vom Server gel\u00f6scht werden, k\u00f6nnen nicht erfasst werden.',
    compliance_3_label: 'L\u00fcckenloser Pr\u00fcfpfad',
    compliance_3_term: 'Nachvollziehbarkeit',
    compliance_3_detail: 'Jede Aktion wird in einem hash-verketteten Protokoll festgehalten, das Manipulationen erkennt und das Sie dem Pr\u00fcfer \u00fcbergeben k\u00f6nnen.',
    compliance_4_label: 'Durchsuchbar und exportierbar',
    compliance_4_term: 'Maschinelle Auswertbarkeit',
    compliance_4_detail: 'Volltextsuche plus Export als .eml-Dateien oder komplettes Pr\u00fcferpaket (ZIP).',

    // Limitations
    limits_title: 'Was Sie wissen sollten',
    limits_intro: 'Vollst\u00e4ndige GoBD-Konformit\u00e4t erfordert auch organisatorische Ma\u00dfnahmen \u2014 keine Software schafft das allein. Hier sehen Sie, was in Ihrer Verantwortung bleibt.',
    limit_1_title: 'Amberize immer laufen lassen',
    limit_1_desc: 'Amberize kann E-Mails nur archivieren, solange es l\u00e4uft. Lassen Sie es automatisch starten und im Hintergrund laufen \u2014 es verbraucht kaum Ressourcen. E-Mails, die vor dem n\u00e4chsten Sync vom Server gel\u00f6scht werden, k\u00f6nnen nicht erfasst werden.',
    limit_2_title: 'Archiv regelm\u00e4\u00dfig sichern',
    limit_2_desc: 'Amberize erzwingt keine 6- oder 10-j\u00e4hrigen Aufbewahrungsfristen automatisch. Erstellen Sie regelm\u00e4\u00dfig Backups Ihrer Archiv-Datei \u2014 auf einer separaten Festplatte, im Tresor oder bei Ihrem Steuerberater.',

    // About
    about_title: 'Warum ich Amberize entwickelt habe',
    about_text: 'Ich bin Johannes Mutter \u2014 selbst\u00e4ndiger Designer und Entwickler in Deutschland. Wie viele Selbst\u00e4ndige habe ich E-Mail-Konten bei mehreren Anbietern und konnte keine GoBD-Archivierungsl\u00f6sung finden, die einfach, zuverl\u00e4ssig und nicht an einen bestimmten Anbieter oder Enterprise-Vertrag gebunden ist. Also habe ich Amberize entwickelt.',
    about_github: 'GitHub',
    about_website: 'mutter.co',
    about_twitter: 'Twitter',

    // Platform availability
    also_available: 'Auch verf\u00fcgbar f\u00fcr',
    platform_and: 'und',

    // CTA
    cta_title: 'E-Mail-Archivierung einfach erledigen',
    cta_sub: 'Laden Sie Amberize herunter, verbinden Sie Ihr E-Mail-Konto und haben Sie eine Sorge weniger, wenn das Finanzamt anruft.',
    cta_button: 'F\u00fcr {os_name} herunterladen',
    cta_note: 'Kostenlos und Open Source. Kein Konto n\u00f6tig.',
    cta_downloads: '{count}+ Downloads',

    // Footer
    footer_text: 'Amberize ist kostenlose Open-Source-Software (MIT-Lizenz).',
    footer_github: 'Auf GitHub ansehen',
    footer_impressum: 'Impressum',
    footer_datenschutz: 'Datenschutz',
    footer_disclaimer: 'Amberize unterst\u00fctzt GoBD-konforme Archivierung, ist aber nicht durch einen Wirtschaftspr\u00fcfer zertifiziert (IDW PS 880). GoBD-Konformit\u00e4t erfordert neben der Software auch organisatorische Ma\u00dfnahmen. Diese Seite stellt keine Rechts- oder Steuerberatung dar. Sprechen Sie mit Ihrem Steuerberater.'
  }
};

/** @type {string[]} */
export const supported_locales = ['en', 'de'];

/** @type {string[]} */
export const supported_os = ['mac', 'windows', 'linux'];

/**
 * Detect the visitor's operating system from the user agent string.
 * Defaults to 'mac' during SSR.
 * @returns {'mac' | 'windows' | 'linux'}
 */
export function detect_os() {
  if (typeof navigator === 'undefined') return 'mac';
  const ua = navigator.userAgent?.toLowerCase() ?? '';
  if (ua.includes('win')) return 'windows';
  if (ua.includes('linux') && !ua.includes('android')) return 'linux';
  return 'mac';
}

/**
 * Get OS-specific placeholder variables for a given locale and OS.
 * @param {string} locale
 * @param {string} os
 * @returns {Record<string, string>}
 */
export function get_os_vars(locale, os) {
  return os_terms[locale]?.[os] ?? os_terms.en?.mac ?? {};
}

/**
 * Detect the best locale from the browser.
 * @returns {string}
 */
export function detect_locale() {
  if (typeof navigator === 'undefined') return 'de';
  const browser_lang = navigator.language?.toLowerCase() ?? '';
  if (browser_lang.startsWith('de')) return 'de';
  return 'en';
}

/**
 * Get a translation string, optionally replacing {placeholder} variables.
 * @param {Record<string, string>} strings
 * @param {string} key
 * @param {Record<string, string>} [vars]
 * @returns {string}
 */
export function t(strings, key, vars) {
  let text = strings[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      text = text.replaceAll(`{${k}}`, v);
    }
  }
  return text;
}
