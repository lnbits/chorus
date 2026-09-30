<script lang="ts">
  import Icon from '$lib/Icon.svelte';
</script>

<svelte:head><title>How it works — Chorus</title></svelte:head>
<div class="protocol-page">
  <p class="eyebrow uppercase">Chorus / Protocol 01</p>
  <h1>Independent work.<br />Verifiable agreement.</h1>
  <p class="lede">
    A federation is a group of Nostr identities that independently run the same WASM contract and
    attest to the same result.
  </p>
  <section>
    <span>01</span>
    <div>
      <h2>Define the federation.</h2>
      <p>
        A signed federation state names the members, contract hash, VM version and threshold. Three
        members or a hundred: the rule belongs to the federation.
      </p>
      <div class="example">
        <code>6/10 = 6 of 10 members must agree</code>
        <p>The quorum is a whole number greater than half the member set.</p>
      </div>
    </div>
  </section>
  <section>
    <span>02</span>
    <div>
      <h2>Execute independently.</h2>
      <p>
        Each member verifies the exact contract and canonical input, then runs it in a restricted
        Wasmtime environment. No network, filesystem, clock, randomness or general WASI. Memory,
        fuel and output are bounded.
      </p>
    </div>
  </section>
  <section>
    <span>03</span>
    <div>
      <h2>Agree on the complete commitment.</h2>
      <p>
        Signed commitments bind the federation, epoch, job, contract, VM, input, result and state
        hashes. Signatures count only when they agree on the exact same commitment. Conflicts and
        equivocation remain visible.
      </p>
      <p>
        FROST can compact an accepted proof after agreement. Individual member attestations remain
        available for auditing.
      </p>
    </div>
  </section>
  <section>
    <span>04</span>
    <div>
      <h2>Govern with the current rule.</h2>
      <p>
        Membership, threshold and contract changes create new epochs. The current threshold approves
        the transition before the new rule takes effect. There is no permanent owner or admin
        override.
      </p>
      <div class="example">
        <code>8/10 → 8 current approvals → new epoch · 6/10</code>
        <p>
          Quorum changes are signed governance events. The new quorum activates only after the
          current quorum approves it.
        </p>
      </div>
    </div>
  </section>
  <section>
    <span>05</span>
    <div>
      <h2>Verify it yourself.</h2>
      <p>
        Pin the federation’s genesis hash through a trusted channel. Download the proof bundle with
        the exact contract, canonical inputs, output, governance history and signatures.
      </p>
      <div class="example">
        <code>fedwasm verify result.json --genesis-hash &lt;hash&gt; --rerun</code>
      </div>
      <p>
        Verification proves that the configured threshold attested to the same result. It does not
        prove that a contract’s business logic is economically correct.
      </p>
    </div>
  </section>
  <div class="end">
    <a href="/federations" class="button primary">Explore federations <Icon name="arrow" /></a>
    <p>No mining. No blocks. No tokens. No global consensus.</p>
  </div>
</div>

<style>
  .protocol-page {
    max-width: 900px;
    margin: 65px auto;
    padding: 0 30px 60px;
  }
  .protocol-page h1 {
    font-size: 56px;
    line-height: 1.1;
    letter-spacing: -2px;
    margin: 23px 0;
  }
  .lede {
    font-size: 19px;
    color: var(--muted);
    line-height: 1.6;
    max-width: 680px;
    margin-bottom: 48px;
  }
  section {
    display: grid;
    grid-template-columns: 65px 1fr;
    padding: 35px 0;
    border-top: 1px solid var(--border);
  }
  section > span {
    font-size: 12px;
    color: var(--muted);
    padding-top: 6px;
  }
  section p {
    color: var(--muted);
    font-size: 15px;
    line-height: 1.7;
  }
  .example {
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--panel);
    margin: 22px 0 0;
  }
  .example p {
    font-size: 12px;
    margin: 10px 0 0;
  }
  .example code {
    overflow-wrap: anywhere;
  }
  .end {
    border-top: 1px solid var(--border);
    padding-top: 30px;
  }
  .end p {
    font-size: 12px;
    color: var(--muted);
  }
  @media (max-width: 760px) {
    .protocol-page {
      margin-top: 40px;
      padding: 0 22px 40px;
    }
    .protocol-page h1 {
      font-size: 40px;
    }
    .lede {
      font-size: 16px;
    }
    section {
      grid-template-columns: 35px 1fr;
    }
  }
</style>
