<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    api,
    ensureApiBase,
    resetApiBaseDiscovery,
    type AgentStatusEntry,
    type AgentStatusResponse,
    type BridgeStatus,
    type ConfigureAgentResponse
  } from '$lib/api';

  const AUTO_OPEN_KEY = 'wyrd-diff/auto-open-shown';

  let bridge: BridgeStatus | null = null;
  let agents: AgentStatusEntry[] = [];
  let mcpUrl = '';
  let error = '';
  let panelOpen = false;
  let wiringId = '';
  let timer: ReturnType<typeof setInterval> | undefined;
  let autoOpened = false;

  async function poll() {
    try {
      await ensureApiBase();
      const status = await api<BridgeStatus>('/api/status');
      bridge = status;
      mcpUrl = status.mcp_url;
      const agentResult = await api<AgentStatusResponse>('/api/agent-status');
      agents = agentResult.agents;
      error = '';
    } catch (err) {
      bridge = null;
      agents = [];
      error = err instanceof Error ? err.message : String(err);
    }
  }

  async function rediscover() {
    resetApiBaseDiscovery();
    await poll();
  }

  async function wireAgent(id: string) {
    if (wiringId) return;
    wiringId = id;
    try {
      await api<ConfigureAgentResponse>(`/api/configure-agents/${id}`, { method: 'POST' });
      await poll();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      wiringId = '';
    }
  }

  function summaryOf(list: AgentStatusEntry[]): 'ok' | 'mismatch' | 'missing' {
    const detected = list.filter((a) => a.detected);
    if (detected.length === 0) return 'ok';
    if (detected.every((a) => a.state === 'ok')) return 'ok';
    if (detected.some((a) => a.state === 'mismatch')) return 'mismatch';
    return 'missing';
  }

  function maybeAutoOpen() {
    if (autoOpened || typeof window === 'undefined') return;
    if (!bridge) return;
    if (window.sessionStorage.getItem(AUTO_OPEN_KEY)) {
      autoOpened = true;
      return;
    }
    if (summaryOf(agents) !== 'ok') {
      panelOpen = true;
    }
    window.sessionStorage.setItem(AUTO_OPEN_KEY, '1');
    autoOpened = true;
  }

  onMount(() => {
    poll().then(maybeAutoOpen);
    timer = setInterval(poll, 5000);
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  let copiedId = '';
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copySnippet(agent: AgentStatusEntry) {
    try {
      await navigator.clipboard.writeText(agent.snippet);
      copiedId = agent.id;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = ''), 1500);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  $: bridgeOk = bridge !== null;
  $: agentSummary = summaryOf(agents);
  $: dotState = !bridgeOk ? 'down' : agentSummary === 'ok' ? 'ok' : agentSummary;

  function wireLabel(agent: AgentStatusEntry): string {
    if (wiringId === agent.id) return 'wiring…';
    switch (agent.state) {
      case 'ok':
        return 'wired';
      case 'mismatch':
        return 're-wire';
      default:
        return 'wire';
    }
  }
</script>

<div class="status-pill">
  <button
    type="button"
    class="status-trigger"
    class:down={dotState === 'down'}
    class:ok={dotState === 'ok'}
    class:warn={dotState === 'mismatch' || dotState === 'missing'}
    on:click={() => (panelOpen = !panelOpen)}
    aria-expanded={panelOpen}
  >
    <span class="dot"></span>
    <span class="label">
      {#if !bridgeOk}
        bridge down
      {:else if agentSummary === 'ok'}
        wired
      {:else if agentSummary === 'mismatch'}
        agent url drift
      {:else}
        agents unwired
      {/if}
    </span>
  </button>
  {#if panelOpen}
    <div class="status-panel" role="dialog">
      <header>
        <strong>Wyrd Diff bridge</strong>
        <button type="button" on:click={() => (panelOpen = false)}>×</button>
      </header>
      {#if bridge}
        <p class="row">
          <span class="key">MCP URL</span>
          <code>{mcpUrl}</code>
        </p>
        <p class="row">
          <span class="key">Version</span>
          <code>{bridge.version}</code>
        </p>
      {:else}
        <p class="error">{error || 'Bridge not reachable.'}</p>
        <button type="button" on:click={rediscover}>Re-scan ports</button>
      {/if}
      {#if bridge && agents.length}
        <ul class="agents">
          {#each agents as agent (agent.id)}
            <li>
              <div class="row-head">
                <span class="agent-name">{agent.display}</span>
                {#if agent.detected}
                  <span class="badge badge-detected">detected</span>
                {:else}
                  <span class="badge badge-missing">not installed</span>
                {/if}
                <span class="state state-{agent.state}">{agent.state.replace('_', ' ')}</span>
              </div>
              {#if agent.configured_url && agent.configured_url !== mcpUrl}
                <code class="drift">configured: {agent.configured_url}</code>
              {/if}
              <div class="row-actions">
                <button
                  type="button"
                  class="primary"
                  on:click={() => wireAgent(agent.id)}
                  disabled={wiringId !== '' || agent.state === 'ok'}
                >
                  {wireLabel(agent)}
                </button>
                <button type="button" class="ghost" on:click={() => copySnippet(agent)}>
                  {copiedId === agent.id ? 'copied' : 'copy snippet'}
                </button>
                <code class="agent-path">{agent.path}</code>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<slot />

<style>
  .status-pill {
    position: fixed;
    bottom: 16px;
    right: 16px;
    z-index: 50;
    font-family: var(--wm-mono);
  }

  .status-trigger {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--wm-border-strong);
    background: var(--wm-surface);
    color: var(--wm-ink);
    font: 700 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    cursor: pointer;
  }

  .status-trigger.ok {
    border-color: var(--wm-green);
    box-shadow: 0 0 12px rgba(124, 255, 158, 0.18);
  }

  .status-trigger.warn {
    border-color: var(--wm-amber);
  }

  .status-trigger.down {
    border-color: var(--wm-red, #ff6b6b);
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--wm-muted);
  }

  .status-trigger.ok .dot {
    background: var(--wm-green);
    box-shadow: 0 0 8px rgba(124, 255, 158, 0.7);
  }

  .status-trigger.warn .dot {
    background: var(--wm-amber);
    box-shadow: 0 0 8px rgba(255, 209, 102, 0.7);
  }

  .status-trigger.down .dot {
    background: var(--wm-red, #ff6b6b);
    box-shadow: 0 0 8px rgba(255, 107, 107, 0.7);
  }

  .status-panel {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    min-width: 320px;
    max-width: 420px;
    padding: 12px 14px;
    background: var(--wm-surface);
    border: 1px solid var(--wm-border-strong);
    box-shadow: var(--wm-shadow);
    display: grid;
    gap: 8px;
  }

  .status-panel header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .status-panel header strong {
    color: var(--wm-green);
    font: 700 12px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .status-panel header button {
    background: transparent;
    border: 0;
    color: var(--wm-muted);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
  }

  .row {
    margin: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    font-size: 12px;
  }

  .row .key {
    color: var(--wm-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .row code {
    color: var(--wm-amber);
    overflow-wrap: anywhere;
  }

  .error {
    margin: 0;
    color: var(--wm-red, #ff6b6b);
    font-size: 12px;
  }

  .agents {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }

  .agents li {
    display: grid;
    gap: 4px;
    padding: 6px 0;
    border-top: 1px solid var(--wm-border);
    font-size: 12px;
  }

  .agents li:first-child {
    border-top: 0;
  }

  .row-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .row-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .badge {
    padding: 1px 6px;
    border: 1px solid var(--wm-border);
    font: 700 10px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .badge-detected {
    color: var(--wm-green);
    border-color: var(--wm-green);
  }

  .badge-missing {
    color: var(--wm-muted);
    border-color: var(--wm-border-strong);
  }

  .ghost {
    background: transparent;
    border: 1px solid var(--wm-border-strong);
    color: var(--wm-ink);
  }

  .primary {
    background: var(--wm-green);
    border: 1px solid var(--wm-green);
    color: var(--wm-bg, #0a0a0a);
  }

  .primary:disabled {
    background: transparent;
    border-color: var(--wm-border-strong);
    color: var(--wm-muted);
    cursor: default;
    box-shadow: none;
  }

  .agent-path {
    color: var(--wm-muted);
    font-size: 11px;
    overflow-wrap: anywhere;
  }

  .agent-name {
    color: var(--wm-ink);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 700;
  }

  .state {
    padding: 1px 6px;
    border: 1px solid var(--wm-border);
    font: 700 10px/1.2 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .state-ok {
    color: var(--wm-green);
    border-color: var(--wm-green);
  }

  .state-mismatch {
    color: var(--wm-amber);
    border-color: var(--wm-amber);
  }

  .state-missing,
  .state-no_file {
    color: var(--wm-red, #ff6b6b);
    border-color: var(--wm-red, #ff6b6b);
  }

  .drift {
    color: var(--wm-amber);
    font-size: 11px;
    overflow-wrap: anywhere;
  }

  .status-panel button {
    padding: 6px 10px;
    font: 700 11px/1 var(--wm-mono);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
  }
</style>
