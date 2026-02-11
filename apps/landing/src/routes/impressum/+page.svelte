<script>
  import { app } from '$lib/app.svelte.js';
</script>

<svelte:head>
  <title>Impressum — Amberize</title>
</svelte:head>

<article class="legal">
  <h1>Impressum</h1>

  <h2>Angaben gemäß § 5 DDG</h2>
  <p>
    Johannes Mutter<br />
    <!-- TODO: Adresse eintragen -->
    [Straße und Hausnummer]<br />
    [PLZ Ort]<br />
    Deutschland
  </p>

  <h2>Kontakt</h2>
  <p>
    <!-- TODO: E-Mail / Telefon eintragen -->
    E-Mail: [TODO: E-Mail-Adresse]
  </p>

  <h2>Umsatzsteuer-ID</h2>
  <p>
    <!-- TODO: Falls vorhanden -->
    Umsatzsteuer-Identifikationsnummer gemäß § 27a Umsatzsteuergesetz:<br />
    [TODO: USt-IdNr. oder „Nicht vorhanden" eintragen]
  </p>

  <h2>Verantwortlich für den Inhalt nach § 18 Abs. 2 MStV</h2>
  <p>
    Johannes Mutter<br />
    [Straße und Hausnummer]<br />
    [PLZ Ort]
  </p>

  <h2>Haftung für Inhalte</h2>
  <p>
    Die Inhalte dieser Seiten wurden mit größter Sorgfalt erstellt.
    Für die Richtigkeit, Vollständigkeit und Aktualität der Inhalte kann jedoch keine Gewähr übernommen werden.
    Als Diensteanbieter bin ich gemäß § 7 Abs. 1 DDG für eigene Inhalte auf diesen Seiten nach den
    allgemeinen Gesetzen verantwortlich. Nach §§ 8 bis 10 DDG bin ich als Diensteanbieter jedoch nicht
    verpflichtet, übermittelte oder gespeicherte fremde Informationen zu überwachen oder nach Umständen
    zu forschen, die auf eine rechtswidrige Tätigkeit hinweisen.
  </p>

  <h2>Haftung für Links</h2>
  <p>
    Diese Website enthält Links zu externen Websites Dritter, auf deren Inhalte ich keinen Einfluss habe.
    Deshalb kann ich für diese fremden Inhalte auch keine Gewähr übernehmen. Für die Inhalte der
    verlinkten Seiten ist stets der jeweilige Anbieter oder Betreiber der Seiten verantwortlich.
    Die verlinkten Seiten wurden zum Zeitpunkt der Verlinkung auf mögliche Rechtsverstöße überprüft.
    Rechtswidrige Inhalte waren zum Zeitpunkt der Verlinkung nicht erkennbar.
  </p>

  <h2>Urheberrecht</h2>
  <p>
    Die durch den Seitenbetreiber erstellten Inhalte und Werke auf diesen Seiten unterliegen dem
    deutschen Urheberrecht. Die Software Amberize steht unter der MIT-Lizenz. Soweit die Inhalte auf
    dieser Seite nicht vom Betreiber erstellt wurden, werden die Urheberrechte Dritter beachtet.
    Sollten Sie trotzdem auf eine Urheberrechtsverletzung aufmerksam werden, bitte ich um einen
    entsprechenden Hinweis. Bei Bekanntwerden von Rechtsverletzungen werde ich derartige Inhalte
    umgehend entfernen.
  </p>
</article>

<style>
  .legal {
    max-width: 640px;
    margin: 0 auto;
    padding: 4rem 1.5rem;
  }
  .legal h1 {
    margin-bottom: 2rem;
  }
  .legal h2 {
    margin-top: 2rem;
    margin-bottom: 0.5rem;
  }
  .legal p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.7;
  }
</style>
