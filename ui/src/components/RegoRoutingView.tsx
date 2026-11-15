import { h } from 'preact';
import { useEffect, useState } from 'preact/hooks';

export interface BlacklistEntry {
  id: string;
  eid_pattern: string;
  reason: string;
  enabled: boolean;
  created_at: number;
  created_by: string;
}

export interface PolicyStatusResponse {
  config: {
    local_eid: string;
    node_eids: string[];
    callsign?: string | null;
    is_digipeater: boolean;
    supported_aliases: string[];
    trapped_aliases: string[];
    hop_limit: number;
    treb_block_type: number;
    record_treb: boolean;
    active_profile: string;
    custom_ingress_rego?: string | null;
    custom_egress_rego?: string | null;
  };
  sources: {
    constants: string;
    helpers: string;
    ingress: string;
    egress: string;
  };
}

export interface DecisionAuditEntry {
  id: number;
  timestamp: number;
  phase: string;
  bundle_id: string;
  source: string;
  destination: string;
  peer_eid?: string | null;
  action: string;
  reason: string;
  route_key?: string | null;
}

const API_BASE = '/api/rego_routing/v1';

export function RegoRoutingView() {
  const [blacklist, setBlacklist] = useState<BlacklistEntry[]>([]);
  const [policyStatus, setPolicyStatus] = useState<PolicyStatusResponse | null>(null);
  const [decisions, setDecisions] = useState<DecisionAuditEntry[]>([]);

  const [newPattern, setNewPattern] = useState('');
  const [newReason, setNewReason] = useState('');

  const [hopLimit, setHopLimit] = useState(16);
  const [trebBlockType, setTrebBlockType] = useState(201);
  const [customIngress, setCustomIngress] = useState('');
  const [customEgress, setCustomEgress] = useState('');

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);

  const loadAll = async () => {
    try {
      const [blRes, polRes, decRes] = await Promise.all([
        fetch(`${API_BASE}/blacklist`),
        fetch(`${API_BASE}/policy`),
        fetch(`${API_BASE}/decisions`),
      ]);
      if (!blRes.ok) throw new Error(`Blacklist HTTP ${blRes.status}`);
      if (!polRes.ok) throw new Error(`Policy HTTP ${polRes.status}`);
      if (!decRes.ok) throw new Error(`Decisions HTTP ${decRes.status}`);

      const blData: BlacklistEntry[] = await blRes.json();
      const polData: PolicyStatusResponse = await polRes.json();
      const decData: DecisionAuditEntry[] = await decRes.json();

      setBlacklist(blData);
      setPolicyStatus(polData);
      setHopLimit(polData.config.hop_limit);
      setTrebBlockType(polData.config.treb_block_type);
      setCustomIngress(polData.config.custom_ingress_rego ?? polData.sources.ingress);
      setCustomEgress(polData.config.custom_egress_rego ?? polData.sources.egress);
      setDecisions(decData);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    loadAll();
  }, []);

  const handleAddBlacklist = async (e: Event) => {
    e.preventDefault();
    if (!newPattern.trim()) return;
    setBusy(true);
    setError(null);
    setStatusMsg(null);
    try {
      const res = await fetch(`${API_BASE}/blacklist`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          eid_pattern: newPattern.trim(),
          reason: newReason.trim(),
          enabled: true,
        }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body.error || `HTTP ${res.status}`);
      }
      setNewPattern('');
      setNewReason('');
      setStatusMsg('EID ajouté à la liste noire Rego.');
      await loadAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleToggleBlacklist = async (entry: BlacklistEntry) => {
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(`${API_BASE}/blacklist/${encodeURIComponent(entry.id)}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled: !entry.enabled }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      await loadAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleDeleteBlacklist = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(`${API_BASE}/blacklist/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      await loadAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleSavePolicy = async (resetToDefault: boolean) => {
    setBusy(true);
    setError(null);
    setStatusMsg(null);
    try {
      const payload = resetToDefault
        ? { reset_to_default: true, hop_limit: hopLimit, treb_block_type: trebBlockType }
        : {
            active_profile: 'custom',
            hop_limit: Number(hopLimit),
            treb_block_type: Number(trebBlockType),
            custom_ingress_rego: customIngress,
            custom_egress_rego: customEgress,
          };

      const res = await fetch(`${API_BASE}/policy`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body.error || `HTTP ${res.status}`);
      }
      setStatusMsg(
        resetToDefault
          ? 'Politique Rego réinitialisée sur le profil embarqué par défaut.'
          : 'Politique Rego compilée et activée à chaud.',
      );
      await loadAll();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleClearDecisions = async () => {
    setBusy(true);
    try {
      await fetch(`${API_BASE}/decisions/clear`, { method: 'POST' });
      await loadAll();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="max-w-6xl mx-auto p-4 space-y-6">
      <header class="border-b pb-4 dark:border-slate-700 flex flex-wrap justify-between items-center gap-3">
        <div>
          <h1 class="text-2xl font-bold text-slate-900 dark:text-white">
            Routage & Filtrage OPA / Rego
          </h1>
          <p class="text-sm text-slate-500">
            Blacklist d&apos;EIDs, Hop Count RFC 9171, Spray-and-Wait et Trajectoire APRS / TREB
          </p>
        </div>
        {policyStatus && (
          <div class="flex items-center gap-2 text-xs">
            <span class="px-2.5 py-1 rounded-full bg-indigo-100 text-indigo-800 dark:bg-indigo-900/50 dark:text-indigo-200 font-semibold">
              Profil: {policyStatus.config.active_profile}
            </span>
            <span class="px-2.5 py-1 rounded-full bg-slate-100 text-slate-700 dark:bg-slate-800 dark:text-slate-300">
              TREB Block: {policyStatus.config.treb_block_type}
            </span>
          </div>
        )}
      </header>

      {error && (
        <div class="p-3 bg-rose-50 text-rose-800 border border-rose-200 rounded-lg text-sm">
          {error}
        </div>
      )}

      {statusMsg && (
        <div class="p-3 bg-emerald-50 text-emerald-800 border border-emerald-200 rounded-lg text-sm">
          {statusMsg}
        </div>
      )}

      {/* Blacklist CRUD */}
      <section class="bg-slate-50 dark:bg-slate-800/40 p-4 rounded-xl border border-slate-200 dark:border-slate-700 space-y-4">
        <h2 class="text-lg font-semibold text-slate-800 dark:text-slate-100">
          Liste noire des EIDs sources (Ingress Filter)
        </h2>

        <form onSubmit={handleAddBlacklist} class="grid grid-cols-1 md:grid-cols-3 gap-3 items-end">
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              Motif d&apos;EID (exact ou wildcard *)
            </label>
            <input
              type="text"
              required
              placeholder="dtn://spam/* ou ipn:99.*"
              value={newPattern}
              onInput={(e) => setNewPattern((e.target as HTMLInputElement).value)}
              class="w-full px-3 py-2 text-sm rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              Raison / commentaire
            </label>
            <input
              type="text"
              placeholder="Anti-spam / boucle"
              value={newReason}
              onInput={(e) => setNewReason((e.target as HTMLInputElement).value)}
              class="w-full px-3 py-2 text-sm rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
          <button
            type="submit"
            disabled={busy}
            class="px-4 py-2 text-sm font-semibold rounded-lg bg-rose-600 text-white hover:bg-rose-700 disabled:opacity-50"
          >
            Bloquer cet EID
          </button>
        </form>

        <div class="overflow-x-auto">
          <table class="min-w-full text-sm text-left">
            <thead class="text-xs uppercase text-slate-500 border-b dark:border-slate-700">
              <tr>
                <th class="py-2 pr-4">Motif EID</th>
                <th class="py-2 pr-4">Raison</th>
                <th class="py-2 pr-4">État</th>
                <th class="py-2 text-right">Actions</th>
              </tr>
            </thead>
            <tbody class="divide-y dark:divide-slate-700">
              {blacklist.length === 0 ? (
                <tr>
                  <td colSpan={4} class="py-4 text-slate-500 italic">
                    Aucun EID blacklisté.
                  </td>
                </tr>
              ) : (
                blacklist.map((entry) => (
                  <tr key={entry.id}>
                    <td class="py-2 pr-4 font-mono text-xs">{entry.eid_pattern}</td>
                    <td class="py-2 pr-4">{entry.reason || '—'}</td>
                    <td class="py-2 pr-4">
                      <button
                        type="button"
                        onClick={() => handleToggleBlacklist(entry)}
                        disabled={busy}
                        class={`px-2 py-0.5 rounded-full text-xs font-medium ${
                          entry.enabled
                            ? 'bg-rose-100 text-rose-800 dark:bg-rose-900/40 dark:text-rose-200'
                            : 'bg-slate-200 text-slate-600 dark:bg-slate-700 dark:text-slate-300'
                        }`}
                      >
                        {entry.enabled ? 'Actif' : 'Inactif'}
                      </button>
                    </td>
                    <td class="py-2 text-right">
                      <button
                        type="button"
                        onClick={() => handleDeleteBlacklist(entry.id)}
                        disabled={busy}
                        class="text-xs text-rose-600 hover:underline"
                      >
                        Supprimer
                      </button>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </section>

      {/* Policy Profile & Editor */}
      <section class="bg-slate-50 dark:bg-slate-800/40 p-4 rounded-xl border border-slate-200 dark:border-slate-700 space-y-4">
        <div class="flex flex-wrap justify-between items-center gap-2">
          <h2 class="text-lg font-semibold text-slate-800 dark:text-slate-100">
            Politiques OPA / Rego actives
          </h2>
          <div class="flex gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => handleSavePolicy(true)}
              class="px-3 py-1.5 text-xs font-medium rounded-lg border border-slate-300 dark:border-slate-600 hover:bg-slate-100 dark:hover:bg-slate-700"
            >
              Réinitialiser (défaut)
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => handleSavePolicy(false)}
              class="px-3 py-1.5 text-xs font-semibold rounded-lg bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50"
            >
              Compiler et appliquer
            </button>
          </div>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              Limite de sauts par défaut (Type 10)
            </label>
            <input
              type="number"
              min={1}
              max={255}
              value={hopLimit}
              onInput={(e) => setHopLimit(Number((e.target as HTMLInputElement).value))}
              class="w-full px-3 py-2 text-sm rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              Code bloc TREB (chemin passé, 192–255)
            </label>
            <input
              type="number"
              min={192}
              max={255}
              value={trebBlockType}
              onInput={(e) => setTrebBlockType(Number((e.target as HTMLInputElement).value))}
              class="w-full px-3 py-2 text-sm rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              ingress.rego (`package dtn.ingress`)
            </label>
            <textarea
              rows={12}
              value={customIngress}
              onInput={(e) => setCustomIngress((e.target as HTMLTextAreaElement).value)}
              class="w-full p-2 font-mono text-xs rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
          <div>
            <label class="block text-xs font-medium text-slate-600 dark:text-slate-300 mb-1">
              egress.rego (`package dtn.egress`)
            </label>
            <textarea
              rows={12}
              value={customEgress}
              onInput={(e) => setCustomEgress((e.target as HTMLTextAreaElement).value)}
              class="w-full p-2 font-mono text-xs rounded-lg border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900"
            />
          </div>
        </div>
      </section>

      {/* Audit log */}
      <section class="bg-slate-50 dark:bg-slate-800/40 p-4 rounded-xl border border-slate-200 dark:border-slate-700 space-y-3">
        <div class="flex justify-between items-center">
          <h2 class="text-lg font-semibold text-slate-800 dark:text-slate-100">
            Journal des décisions Rego ({decisions.length})
          </h2>
          <div class="flex gap-2">
            <button
              type="button"
              onClick={loadAll}
              class="px-3 py-1 text-xs rounded-lg border border-slate-300 dark:border-slate-600"
            >
              Rafraîchir
            </button>
            <button
              type="button"
              onClick={handleClearDecisions}
              class="px-3 py-1 text-xs rounded-lg border border-slate-300 dark:border-slate-600 text-rose-600"
            >
              Effacer
            </button>
          </div>
        </div>

        <div class="overflow-x-auto">
          <table class="min-w-full text-xs text-left">
            <thead class="uppercase text-slate-500 border-b dark:border-slate-700">
              <tr>
                <th class="py-2 pr-3">Phase</th>
                <th class="py-2 pr-3">Action</th>
                <th class="py-2 pr-3">Source → Dest</th>
                <th class="py-2 pr-3">Route Key / Peer</th>
                <th class="py-2">Raison</th>
              </tr>
            </thead>
            <tbody class="divide-y dark:divide-slate-700">
              {decisions.length === 0 ? (
                <tr>
                  <td colSpan={5} class="py-4 text-slate-500 italic">
                    Aucune évaluation Rego enregistrée pour le moment.
                  </td>
                </tr>
              ) : (
                decisions.map((d) => (
                  <tr key={d.id}>
                    <td class="py-2 pr-3 font-mono">{d.phase}</td>
                    <td class="py-2 pr-3 font-semibold">{d.action}</td>
                    <td class="py-2 pr-3 font-mono">
                      {d.source} → {d.destination}
                    </td>
                    <td class="py-2 pr-3 font-mono">{d.route_key || d.peer_eid || '—'}</td>
                    <td class="py-2">{d.reason}</td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
