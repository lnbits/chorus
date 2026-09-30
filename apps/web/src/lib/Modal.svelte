<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  let {
    title,
    onclose,
    children,
    footer
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();
  let dialog: HTMLDivElement;
  onMount(() => {
    const before = document.activeElement as HTMLElement;
    dialog.focus();
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => {
      document.body.style.overflow = overflow;
      before?.focus();
    };
  });
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
    if (e.key === 'Tab') {
      const els = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input, select, textarea, a[href]'
        )
      );
      if (!els.length) return;
      const first = els[0],
        last = els[els.length - 1];
      if (e.shiftKey && (document.activeElement === first || document.activeElement === dialog)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<div class="modal-backdrop">
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    bind:this={dialog}
    onkeydown={key}
  >
    <div class="modal-head">
      <h2>{title}</h2>
      <button class="close" aria-label="Close dialog" onclick={onclose}>×</button>
    </div>
    <div class="modal-body">{@render children()}</div>
    {#if footer}<div class="modal-footer">{@render footer()}</div>{/if}
  </div>
</div>
