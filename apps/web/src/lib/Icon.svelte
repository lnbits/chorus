<script lang="ts">
  let { name, size = 18 }: { name: string; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.6"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === 'federations'}<rect x="3" y="3" width="6" height="6" /><rect
      x="15"
      y="3"
      width="6"
      height="6"
    /><rect x="3" y="15" width="6" height="6" /><rect x="15" y="15" width="6" height="6" />
  {:else if name === 'activity'}<path d="M3 12h4l3-8 4 16 3-8h4" />
  {:else if name === 'settings'}<circle cx="12" cy="12" r="3" /><path
      d="m10 3-1 3-3 1-3 3v4l3 1 1 3 3 3h4l1-3 3-1 3-3v-4l-3-1-1-3-3-3z"
    />
  {:else if name === 'arrow'}<path d="M4 12h16m-6-6 6 6-6 6" />
  {:else if name === 'plus'}<path d="M12 5v14M5 12h14" />
  {:else if name === 'check'}<path d="m5 12 4 4L19 6" />
  {:else if name === 'protocol'}<path d="m8 5-6 7 6 7m8-14 6 7-6 7m-5 0 2-14" />
  {:else if name === 'logout'}<path d="M10 4H4v16h6m4-4 4-4-4-4m-5 4h12" />
  {:else if name === 'lock'}<rect x="5" y="10" width="14" height="11" rx="1" /><path
      d="M8 10V7a4 4 0 0 1 8 0v3"
    />
  {:else}<circle cx="12" cy="12" r="8" /><path d="M12 8v5m0 3h.01" />{/if}
</svg>
