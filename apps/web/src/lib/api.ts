import { writable, get } from 'svelte/store';
export type Session = { authenticated: boolean; username?: string; csrf?: string };
export const session = writable<Session>({ authenticated: false });
export const connection = writable<'connecting' | 'online' | 'offline'>('connecting');
export async function api<T = unknown>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (options.body) headers.set('content-type', 'application/json');
  const csrf = get(session).csrf;
  if (csrf) headers.set('x-csrf-token', csrf);
  let res: Response;
  try {
    res = await fetch(`/api/v1${path}`, { ...options, headers, credentials: 'same-origin' });
  } catch {
    connection.set('offline');
    throw new Error('Cannot reach the node. Check that Chorus is running.');
  }
  const text = await res.text();
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    connection.set('offline');
    throw new Error('The node is unavailable. Check your connection and node address.');
  }
  connection.set('online');
  if (!res.ok) throw new Error(data.error || `Request failed (${res.status})`);
  return data as T;
}
export async function refreshSession() {
  try {
    session.set(await api<Session>('/session'));
  } catch {
    session.set({ authenticated: false });
  }
}
export async function logout() {
  await api('/logout', { method: 'POST' });
  session.set({ authenticated: false });
}
export function short(value?: string | null, n = 12) {
  return value ? `${value.slice(0, n)}…${value.slice(-6)}` : '—';
}
export function date(value?: number) {
  return value ? new Date(value * 1000).toLocaleString('en-GB') : '—';
}
export function bytes(value: number) {
  return value < 1024
    ? `${value} B`
    : value < 1048576
      ? `${(value / 1024).toFixed(1)} KB`
      : `${(value / 1048576).toFixed(1)} MB`;
}
export function hex(value: Uint8Array) {
  return Array.from(value, (b) => b.toString(16).padStart(2, '0')).join('');
}
export function thresholdLabel(rule: Threshold, count: number) {
  return `${rule.value}/${count}`;
}
export function errorMessage(e: unknown) {
  return e instanceof Error ? e.message : 'Something went wrong.';
}
export type Threshold = { type: 'absolute'; value: number };
export type Federation = {
  protocol_version: number;
  federation_id: string;
  epoch: number;
  name: string;
  description: string;
  public: boolean;
  members: string[];
  communication_keys: [string, string][];
  threshold: Threshold;
  required: number;
  wasm_hash: string;
  vm_version: string;
  initial_state: number[] | null;
  previous_federation_state_hash: string | null;
  created_at: number;
  relays: string[];
  frost: unknown;
  status: string;
  finalized_jobs: number;
  latest_job: string | null;
  state_hash: string;
};
export type Commitment = {
  result_hash: string;
  next_state_hash: string | null;
  job_id: string;
  input_hash: string;
  [key: string]: unknown;
};
export type JobRow = {
  id: string;
  status: string;
  agreeing: number;
  required: number;
  member_count: number;
  has_proof: boolean;
  job: {
    epoch: number;
    sequence: number | null;
    created_at: number;
    expires_at: number;
    wasm_hash: string;
    input_hash: string;
    previous_state_hash: string | null;
    input: number[];
  };
  consensus: {
    required: number;
    accepted: string | null;
    equivocators: string[];
    conflicting_certificates: boolean;
    groups: { commitment: Commitment; signers: string[]; attestations: unknown[] }[];
  };
};
export async function downloadJson(path: string, filename: string) {
  const data = await api(path);
  const u = URL.createObjectURL(
    new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  );
  const a = document.createElement('a');
  a.href = u;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(u);
}
