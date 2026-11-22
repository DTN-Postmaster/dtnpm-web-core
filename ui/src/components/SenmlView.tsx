import { h } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';

export interface SenmlCard {
  id: number;
  name: string;
  unit: string | null;
  custom_label: string | null;
  display_order: number | null;
  is_visible: boolean;
  utc: number | null;
  value: number | null;
  str_value: string | null;
  updatedFlash?: boolean;
}

export interface MetricDictionaryItem {
  id: number;
  name: string;
  unit: string | null;
  custom_label: string | null;
  display_order: number | null;
  is_visible: boolean;
}

export function SenmlView() {
  const [cards, setCards] = useState<SenmlCard[]>([]);
  const [availableMetrics, setAvailableMetrics] = useState<MetricDictionaryItem[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);

  // Editing state for inline label rename
  const [editingCardId, setEditingCardId] = useState<number | null>(null);
  const [editLabelValue, setEditLabelValue] = useState<string>('');

  // Modal state for adding a card
  const [isAddModalOpen, setIsAddModalOpen] = useState<boolean>(false);
  const [selectedMetricName, setSelectedMetricName] = useState<string>('');
  const [customMetricName, setCustomMetricName] = useState<string>('');
  const [newCardLabel, setNewCardLabel] = useState<string>('');
  const [isSubmitting, setIsSubmitting] = useState<boolean>(false);

  // Fetch dashboard cards
  const fetchDashboardCards = async () => {
    try {
      setLoading(true);
      setError(null);
      const res = await fetch('/api/senml/v1/metrics/latest?dashboard_only=true');
      if (!res.ok) throw new Error(`HTTP error ${res.status}`);
      const data: SenmlCard[] = await res.json();
      setCards(data);
    } catch (e: any) {
      setError(e.message || 'Failed to load telemetry metrics');
    } finally {
      setLoading(false);
    }
  };

  // Fetch all dictionary metrics for the add modal
  const fetchAllMetrics = async () => {
    try {
      const res = await fetch('/api/senml/v1/metrics');
      if (res.ok) {
        const data: MetricDictionaryItem[] = await res.json();
        setAvailableMetrics(data);
      }
    } catch (e) {
      console.warn('Failed to fetch dictionary metrics', e);
    }
  };

  useEffect(() => {
    fetchDashboardCards();
    fetchAllMetrics();
  }, []);

  // Subscribe to real-time SSE telemetry stream
  useEffect(() => {
    let es: EventSource | null = null;
    const flashTimers = new Set<ReturnType<typeof setTimeout>>();
    try {
      es = new EventSource('/api/senml/v1/stream');

      es.addEventListener('telemetry', (e: MessageEvent) => {
        try {
          const pt = JSON.parse(e.data);
          // pt: { id: string, utc: number, value: any, unit?: string, label?: string }
          setCards((prevCards) =>
            prevCards.map((c) => {
              if (c.name === pt.id) {
                const isNum = typeof pt.value === 'number' && Number.isFinite(pt.value);
                const numericVal = isNum ? pt.value : null;
                const strVal = isNum || pt.value === undefined || pt.value === null ? null : String(pt.value);
                return {
                  ...c,
                  utc: pt.utc,
                  value: numericVal,
                  str_value: strVal,
                  unit: pt.unit !== undefined ? pt.unit : c.unit,
                  updatedFlash: true,
                };
              }
              return c;
            })
          );

          // Reset pulse flash after 800ms
          const t = setTimeout(() => {
            flashTimers.delete(t);
            setCards((prevCards) =>
              prevCards.map((c) => (c.name === pt.id ? { ...c, updatedFlash: false } : c))
            );
          }, 800);
          flashTimers.add(t);
        } catch (err) {
          console.debug('Failed to parse telemetry SSE event', err);
        }
      });
    } catch (err) {
      console.warn('Failed to connect to /api/senml/v1/stream', err);
    }

    return () => {
      if (es) {
        es.close();
      }
      flashTimers.forEach((t) => clearTimeout(t));
      flashTimers.clear();
    };
  }, []);

  // Inline rename handler
  const handleStartEdit = (card: SenmlCard) => {
    setEditingCardId(card.id);
    setEditLabelValue(card.custom_label || card.name);
  };

  const handleSaveEdit = async (cardId: number) => {
    try {
      const res = await fetch(`/api/senml/v1/dashboard/cards/${cardId}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ label: editLabelValue.trim() || null }),
      });
      if (!res.ok) throw new Error('Failed to update label');
      const updated: SenmlCard = await res.json();
      setCards((prev) =>
        prev.map((c) => (c.id === cardId ? { ...c, custom_label: updated.custom_label } : c))
      );
      setEditingCardId(null);
    } catch (err: any) {
      alert(err.message);
    }
  };

  // Reorder card handlers (Up / Down)
  const handleMove = async (index: number, direction: 'up' | 'down') => {
    const targetIndex = direction === 'up' ? index - 1 : index + 1;
    if (targetIndex < 0 || targetIndex >= cards.length) return;

    const currentCard = cards[index];
    const targetCard = cards[targetIndex];

    const currentOrder = currentCard.display_order ?? index + 1;
    const targetOrder = targetCard.display_order ?? targetIndex + 1;

    // Swap locally for instant responsiveness
    const newCards = [...cards];
    newCards[index] = { ...targetCard, display_order: currentOrder };
    newCards[targetIndex] = { ...currentCard, display_order: targetOrder };
    setCards(newCards);

    try {
      await Promise.all([
        fetch(`/api/senml/v1/dashboard/cards/${currentCard.id}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ order: targetOrder }),
        }),
        fetch(`/api/senml/v1/dashboard/cards/${targetCard.id}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ order: currentOrder }),
        }),
      ]);
    } catch (err) {
      fetchDashboardCards();
    }
  };

  // Remove card from dashboard
  const handleRemoveCard = async (cardId: number) => {
    if (!confirm('Remove this metric from the dashboard? (Historical data will be kept)')) {
      return;
    }
    try {
      const res = await fetch(`/api/senml/v1/dashboard/cards/${cardId}`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error('Failed to remove card');
      setCards((prev) => prev.filter((c) => c.id !== cardId));
    } catch (err: any) {
      alert(err.message);
    }
  };

  // Clear all cards
  const handleClearAll = async () => {
    if (!confirm('Remove all cards from the dashboard?')) {
      return;
    }
    try {
      const res = await fetch('/api/senml/v1/dashboard/cards', {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error('Failed to reset dashboard');
      setCards([]);
    } catch (err: any) {
      alert(err.message);
    }
  };

  // Add card form submission
  const handleAddCardSubmit = async (e: Event) => {
    e.preventDefault();
    const finalName = customMetricName.trim() || selectedMetricName.trim();
    if (!finalName) {
      alert('Please select or enter a metric name');
      return;
    }

    try {
      setIsSubmitting(true);
      const res = await fetch('/api/senml/v1/dashboard/cards', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: finalName,
          label: newCardLabel.trim() || null,
        }),
      });

      if (!res.ok) {
        const errJson = await res.json().catch(() => ({}));
        throw new Error(errJson.error || 'Failed to add card');
      }

      setIsAddModalOpen(false);
      setSelectedMetricName('');
      setCustomMetricName('');
      setNewCardLabel('');
      await fetchDashboardCards();
      await fetchAllMetrics();
    } catch (err: any) {
      alert(err.message);
    } finally {
      setIsSubmitting(false);
    }
  };

  // Helper to format timestamps relative to now
  const formatTimeAgo = (utcMs: number | null): string => {
    if (!utcMs) return 'No readings';
    const now = Date.now();
    const diffSec = Math.floor((now - utcMs) / 1000);

    if (diffSec < 5) return 'just now';
    if (diffSec < 60) return `${diffSec}s ago`;
    const diffMin = Math.floor(diffSec / 60);
    if (diffMin < 60) return `${diffMin}m ago`;
    const diffHours = Math.floor(diffMin / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
  };

  // Format measurement value
  const formatValue = (card: SenmlCard): string => {
    if (card.value !== null && card.value !== undefined) {
      return Number.isInteger(card.value) ? card.value.toString() : card.value.toFixed(2);
    }
    if (card.str_value) {
      return card.str_value;
    }
    return '--';
  };

  return (
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">
      {/* Top action bar */}
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 bg-white dark:bg-slate-800 p-4 rounded-xl shadow-xs border border-slate-200 dark:border-slate-700">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-slate-900 dark:text-white flex items-center gap-2">
            <span>📊</span>
            <span>SenML Telemetry</span>
            <span class="text-xs font-normal text-slate-500 dark:text-slate-400 bg-slate-100 dark:bg-slate-700 px-2.5 py-0.5 rounded-full">
              RFC 8428
            </span>
          </h1>
          <p class="text-xs sm:text-sm text-slate-500 dark:text-slate-400 mt-1">
            Dashboard of IoT sensor readings received by the local DTN <code class="text-emerald-600 dark:text-emerald-400 font-mono">senml</code> service.
          </p>
        </div>

        <div class="flex items-center gap-2">
          <button
            onClick={() => {
              fetchAllMetrics();
              setIsAddModalOpen(true);
            }}
            class="inline-flex items-center gap-1.5 px-3 py-2 bg-emerald-600 hover:bg-emerald-700 text-white text-xs sm:text-sm font-semibold rounded-lg shadow-xs transition-colors"
          >
            <span>➕</span>
            <span>Add Card</span>
          </button>

          {cards.length > 0 && (
            <button
              onClick={handleClearAll}
              class="inline-flex items-center gap-1.5 px-3 py-2 bg-slate-100 hover:bg-rose-50 text-slate-600 hover:text-rose-600 dark:bg-slate-700 dark:hover:bg-rose-950/40 dark:text-slate-300 dark:hover:text-rose-400 text-xs sm:text-sm font-semibold rounded-lg border border-slate-200 dark:border-slate-600 transition-colors"
              title="Clear dashboard"
            >
              <span>🗑️</span>
              <span class="hidden sm:inline">Clear All</span>
            </button>
          )}

          <button
            onClick={fetchDashboardCards}
            class="p-2 text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-white rounded-lg hover:bg-slate-100 dark:hover:bg-slate-700 transition-colors"
            title="Refresh"
            aria-label="Refresh"
          >
            🔄
          </button>
        </div>
      </div>

      {/* Error Banner */}
      {error && (
        <div class="bg-rose-50 dark:bg-rose-950/50 border border-rose-200 dark:border-rose-900 text-rose-800 dark:text-rose-200 px-4 py-3 rounded-xl text-sm flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span>⚠️</span>
            <span>{error}</span>
          </div>
          <button onClick={() => setError(null)} class="text-rose-500 hover:text-rose-700 font-bold">
            ×
          </button>
        </div>
      )}

      {/* Loading state */}
      {loading && cards.length === 0 ? (
        <div class="text-center py-16 bg-white dark:bg-slate-800 rounded-xl border border-slate-200 dark:border-slate-700 text-slate-500">
          <div class="animate-spin inline-block w-8 h-8 border-4 border-current border-t-transparent text-emerald-600 rounded-full mb-3" />
          <p class="text-sm font-medium">Loading telemetry data...</p>
        </div>
      ) : cards.length === 0 ? (
        /* Empty state */
        <div class="text-center py-16 bg-white dark:bg-slate-800 rounded-xl border border-dashed border-slate-300 dark:border-slate-700 p-8">
          <div class="text-4xl mb-3">📡</div>
          <h3 class="text-base font-bold text-slate-900 dark:text-white">No metrics on dashboard</h3>
          <p class="text-xs sm:text-sm text-slate-500 dark:text-slate-400 max-w-md mx-auto mt-1 mb-6">
            Measurements received on <code class="font-mono">dtn://&lt;node&gt;/senml</code> are stored in the database. Click the button below to add a card to this dashboard.
          </p>
          <button
            onClick={() => {
              fetchAllMetrics();
              setIsAddModalOpen(true);
            }}
            class="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 text-white font-semibold rounded-lg text-sm transition-colors shadow-xs"
          >
            ➕ Add Card
          </button>
        </div>
      ) : (
        /* Cards Grid */
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {cards.map((card, index) => (
            <div
              key={card.id}
              class={`relative flex flex-col justify-between bg-white dark:bg-slate-800 rounded-xl p-5 border transition-all duration-300 shadow-xs hover:shadow-md ${
                card.updatedFlash
                  ? 'border-emerald-500 ring-2 ring-emerald-500/20 bg-emerald-50/20 dark:bg-emerald-950/20'
                  : 'border-slate-200 dark:border-slate-700'
              }`}
            >
              {/* Card Header */}
              <div class="flex items-start justify-between gap-2 mb-3">
                <div class="flex-1 min-w-0">
                  {editingCardId === card.id ? (
                    <div class="flex items-center gap-1.5 mt-1">
                      <input
                        type="text"
                        value={editLabelValue}
                        onInput={(e) => setEditLabelValue((e.target as HTMLInputElement).value)}
                        placeholder="Custom label"
                        class="px-2 py-1 text-sm bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-600 rounded text-slate-900 dark:text-white font-semibold focus:outline-none focus:ring-1 focus:ring-emerald-500 w-full"
                        autoFocus
                      />
                      <button
                        onClick={() => handleSaveEdit(card.id)}
                        class="p-1 text-emerald-600 hover:bg-emerald-50 dark:hover:bg-emerald-950/50 rounded"
                        title="Save"
                        aria-label="Save"
                      >
                        ✓
                      </button>
                      <button
                        onClick={() => setEditingCardId(null)}
                        class="p-1 text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-700 rounded"
                        title="Cancel"
                        aria-label="Cancel"
                      >
                        ✕
                      </button>
                    </div>
                  ) : (
                    <div class="flex items-center gap-1.5 group">
                      <span
                        class="text-base font-bold text-slate-900 dark:text-white truncate"
                        title={card.custom_label || card.name}
                      >
                        {card.custom_label || card.name}
                      </span>
                      <button
                        onClick={() => handleStartEdit(card)}
                        class="opacity-0 group-hover:opacity-100 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 text-xs transition-opacity p-0.5 rounded"
                        title="Edit label"
                        aria-label="Edit label"
                      >
                        ✏️
                      </button>
                    </div>
                  )}

                  {/* Subtitle with raw metric name if alias is defined */}
                  {card.custom_label && (
                    <div class="text-[11px] font-mono text-slate-400 dark:text-slate-500 truncate mt-0.5" title={card.name}>
                      {card.name}
                    </div>
                  )}
                </div>

                {/* Reordering and remove controls */}
                <div class="flex items-center gap-1 shrink-0 text-xs">
                  <button
                    onClick={() => handleMove(index, 'up')}
                    disabled={index === 0}
                    class="p-1 rounded text-slate-400 hover:text-slate-700 dark:hover:text-slate-200 disabled:opacity-30 disabled:hover:text-slate-400 transition-colors"
                    title="Move up"
                    aria-label="Move up"
                  >
                    ▲
                  </button>
                  <button
                    onClick={() => handleMove(index, 'down')}
                    disabled={index === cards.length - 1}
                    class="p-1 rounded text-slate-400 hover:text-slate-700 dark:hover:text-slate-200 disabled:opacity-30 disabled:hover:text-slate-400 transition-colors"
                    title="Move down"
                    aria-label="Move down"
                  >
                    ▼
                  </button>
                  <button
                    onClick={() => handleRemoveCard(card.id)}
                    class="p-1 rounded text-slate-400 hover:text-rose-600 dark:hover:text-rose-400 transition-colors ml-1"
                    title="Remove from dashboard"
                    aria-label="Remove from dashboard"
                  >
                    ✕
                  </button>
                </div>
              </div>

              {/* Main Measurement Value & Unit */}
              <div class="my-4 flex items-baseline gap-2">
                <span class="text-3xl sm:text-4xl font-extrabold tracking-tight text-slate-900 dark:text-white font-mono">
                  {formatValue(card)}
                </span>
                {card.unit && (
                  <span class="px-2 py-0.5 text-xs font-bold rounded-md bg-slate-100 dark:bg-slate-700/80 text-slate-700 dark:text-slate-300 font-mono">
                    {card.unit}
                  </span>
                )}
              </div>

              {/* Timestamp & Status Footer */}
              <div class="pt-3 border-t border-slate-100 dark:border-slate-700/60 flex items-center justify-between text-xs text-slate-400 dark:text-slate-500">
                <span
                  class="cursor-help flex items-center gap-1"
                  title={card.utc ? new Date(card.utc).toISOString() : 'Unknown'}
                >
                  <span>⏱️</span>
                  <span>{formatTimeAgo(card.utc)}</span>
                </span>

                <span class="text-[10px] uppercase font-mono px-1.5 py-0.5 rounded bg-slate-100 dark:bg-slate-800 text-slate-400">
                  #{card.display_order ?? index + 1}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Modal: Add Card */}
      {isAddModalOpen && (
        <div class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-900/60 backdrop-blur-xs">
          <div
            class="bg-white dark:bg-slate-800 rounded-2xl max-w-md w-full shadow-2xl border border-slate-200 dark:border-slate-700 overflow-hidden"
            role="dialog"
            aria-modal="true"
            aria-labelledby="modal-add-card-title"
          >
            <div class="p-6">
              <div class="flex items-center justify-between pb-3 border-b border-slate-200 dark:border-slate-700">
                <h3 id="modal-add-card-title" class="text-lg font-bold text-slate-900 dark:text-white flex items-center gap-2">
                  <span>➕</span>
                  <span>Add Metric to Dashboard</span>
                </h3>
                <button
                  onClick={() => setIsAddModalOpen(false)}
                  class="text-slate-400 hover:text-slate-600 dark:hover:text-slate-200"
                >
                  ✕
                </button>
              </div>

              <form onSubmit={handleAddCardSubmit} class="mt-4 space-y-4">
                {/* Select from discovered metrics */}
                <div>
                  <label class="block text-xs font-bold text-slate-700 dark:text-slate-300 uppercase tracking-wider mb-1.5">
                    Select from received metrics
                  </label>
                  <select
                    value={selectedMetricName}
                    onChange={(e) => {
                      const val = (e.target as HTMLSelectElement).value;
                      setSelectedMetricName(val);
                      if (val) setCustomMetricName('');
                    }}
                    class="w-full px-3 py-2 text-sm bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-600 rounded-lg text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-emerald-500"
                  >
                    <option value="">-- Choose an existing metric --</option>
                    {availableMetrics.map((m) => (
                      <option key={m.id} value={m.name}>
                        {m.name} {m.unit ? `(${m.unit})` : ''} {m.is_visible ? '· [Already shown]' : ''}
                      </option>
                    ))}
                  </select>
                </div>

                {/* Or enter custom metric name */}
                <div>
                  <label class="block text-xs font-bold text-slate-700 dark:text-slate-300 uppercase tracking-wider mb-1.5">
                    Or enter a full metric name
                  </label>
                  <input
                    type="text"
                    placeholder="e.g. urn:dtn:node1:battery_voltage"
                    value={customMetricName}
                    onInput={(e) => {
                      const val = (e.target as HTMLInputElement).value;
                      setCustomMetricName(val);
                      if (val) setSelectedMetricName('');
                    }}
                    class="w-full px-3 py-2 text-sm font-mono bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-600 rounded-lg text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-emerald-500"
                  />
                </div>

                {/* Custom label / alias */}
                <div>
                  <label class="block text-xs font-bold text-slate-700 dark:text-slate-300 uppercase tracking-wider mb-1.5">
                    Display Label / Alias (optional)
                  </label>
                  <input
                    type="text"
                    placeholder="e.g. Main Battery Voltage"
                    value={newCardLabel}
                    onInput={(e) => setNewCardLabel((e.target as HTMLInputElement).value)}
                    class="w-full px-3 py-2 text-sm bg-slate-50 dark:bg-slate-900 border border-slate-300 dark:border-slate-600 rounded-lg text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-emerald-500"
                  />
                </div>

                <div class="pt-4 border-t border-slate-200 dark:border-slate-700 flex justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => setIsAddModalOpen(false)}
                    class="px-4 py-2 text-sm font-semibold text-slate-600 dark:text-slate-300 hover:bg-slate-100 dark:hover:bg-slate-700 rounded-lg transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={isSubmitting || (!selectedMetricName && !customMetricName)}
                    class="px-4 py-2 text-sm font-semibold text-white bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50 rounded-lg shadow-xs transition-colors"
                  >
                    {isSubmitting ? 'Adding...' : 'Add to Dashboard'}
                  </button>
                </div>
              </form>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
