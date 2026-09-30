<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { session } from '$lib/api';
</script>

<svelte:head><title>Chorus — Run the same contract. Agree on the same result.</title></svelte:head>
<div class="landing">
  <section class="hero">
    <p class="eyebrow uppercase">Federated computation</p>
    <h1>Run the same contract.<br />Agree on the same result.</h1>
    <p class="intro">Independent execution. Shared agreement.<br />Results anyone can verify.</p>
    <p class="description">
      Federations independently execute deterministic WASM. When the configured threshold agrees on
      the same output, the result is signed and independently verifiable.
    </p>
    <div class="actions">
      <a href="/federations" class="button primary"
        >Explore federations <Icon name="arrow" size={17} /></a
      ><a href={$session.authenticated ? '/federations' : '/login'} class="button"
        >{$session.authenticated ? 'Open your node' : 'Connect to your node'}</a
      >
    </div>
    <div class="hero-note">
      <span class="dot"></span> Nostr-native. No blockchain. No global consensus.
    </div>
  </section>
  <section class="principles" aria-label="Three parts, one verifiable result">
    <article>
      <span class="number">01 / CONTRACT</span>
      <h2>WASM executes.</h2>
      <p>The same contract. The same inputs.<br />A deliberately deterministic runtime.</p>
    </article>
    <article>
      <span class="number">02 / COMMUNICATION</span>
      <h2>Nostr connects.</h2>
      <p>Signed identities, proposals and results.<br />Across independent relays.</p>
    </article>
    <article>
      <span class="number">03 / AGREEMENT</span>
      <h2>The threshold attests.</h2>
      <p>Your members. Your threshold.<br />Every signature stays auditable.</p>
    </article>
  </section>
  <section class="verification-line">
    <div>
      <p class="eyebrow uppercase">Trust, made explicit</p>
      <h2>Don’t take the result on trust.<br />Run it yourself.</h2>
    </div>
    <div>
      <p>
        Obtain the contract and inputs. Check the hashes. Re-execute locally. Verify the signatures.
      </p>
      <a href="/protocol" class="inline-link"
        >Read how Chorus works <span aria-hidden="true">↗</span></a
      >
    </div>
  </section>
  <footer>
    <span>CHORUS</span>
    <p>Federated deterministic attestations.</p>
    <a href="/protocol">Protocol 01 ↗</a>
  </footer>
</div>

<style>
  .landing {
    max-width: 1530px;
    margin: auto;
    padding: 0 7%;
  }
  .hero {
    padding: 78px 0 65px;
  }
  .hero > .eyebrow {
    margin-bottom: 27px;
  }
  .hero h1 {
    font-size: clamp(38px, 4.2vw, 68px);
    line-height: 1.09;
    letter-spacing: -2.4px;
    margin: 0;
    font-weight: 750;
  }
  .intro {
    font-size: 25px;
    line-height: 1.4;
    letter-spacing: -0.3px;
    margin: 28px 0 17px;
  }
  .description {
    color: var(--muted);
    font-size: 16px;
    line-height: 1.65;
    max-width: 570px;
    margin: 0 0 29px;
  }
  .hero-note {
    margin-top: 26px;
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--muted);
    font-size: 12px;
  }
  .hero-note .dot {
    width: 6px;
    height: 6px;
  }
  .principles {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--panel);
  }
  .principles article {
    padding: 29px 30px 24px;
    border-right: 1px solid var(--border);
  }
  .principles article:last-child {
    border: 0;
  }
  .number {
    font-size: 10px;
    letter-spacing: 1.6px;
    color: var(--muted);
  }
  .principles h2 {
    margin: 26px 0 10px;
    font-size: 24px;
    letter-spacing: -0.5px;
  }
  .principles p {
    font-size: 13px;
    color: var(--muted);
    line-height: 1.7;
    margin-bottom: 0;
  }
  .verification-line {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 40px;
    align-items: center;
    padding: 52px 0;
  }
  .verification-line h2 {
    font-size: 27px;
    line-height: 1.2;
    margin: 14px 0 0;
  }
  .verification-line > div:last-child {
    max-width: 390px;
    justify-self: end;
  }
  .verification-line p:not(.eyebrow) {
    font-size: 14px;
    color: var(--muted);
  }
  .verification-line a {
    font-size: 13px;
  }
  .verification-line a span {
    margin-left: 20px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 24px;
    border-top: 1px solid var(--border);
    padding: 20px 0;
    color: var(--muted);
    font-size: 11px;
  }
  footer > span {
    font-weight: 700;
    letter-spacing: 2px;
  }
  footer p {
    margin: 0;
  }
  footer a {
    margin-left: auto;
  }
  @media (max-width: 760px) {
    .landing {
      padding: 0 22px;
    }
    .hero {
      padding: 56px 0 42px;
    }
    .hero h1 {
      letter-spacing: -1.2px;
      font-size: 40px;
    }
    .intro {
      font-size: 22px;
    }
    .description {
      font-size: 14px;
    }
    .principles {
      grid-template-columns: 1fr;
    }
    .principles article {
      border-right: 0;
      border-bottom: 1px solid var(--border);
      padding: 24px;
    }
    .principles h2 {
      margin-top: 16px;
    }
    .verification-line {
      grid-template-columns: 1fr;
      gap: 15px;
    }
    .verification-line > div:last-child {
      justify-self: start;
    }
    footer {
      gap: 15px;
    }
    footer p {
      display: none;
    }
  }
</style>
