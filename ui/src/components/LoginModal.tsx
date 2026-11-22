import { h } from 'preact';
import { useState } from 'preact/hooks';

export interface AuthState {
  authenticated: boolean;
  username: string;
  roles: string[];
  auth_mode: string;
  capabilities: {
    can_manage_clas: boolean;
    can_restart_server: boolean;
    can_inject_bundles: boolean;
  };
}

interface LoginModalProps {
  stationCallsign?: string;
  onSuccess: (auth: AuthState) => void;
}

export function LoginModal({ stationCallsign, onSuccess }: LoginModalProps) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    if (!username.trim() || !password) {
      setError('Please enter your callsign / username and password.');
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      const res = await fetch('/api/v1/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: username.trim(), password }),
      });

      if (res.ok) {
        const data: AuthState = await res.json();
        setPassword('');
        onSuccess(data);
        return;
      }

      let serverMsg = '';
      try {
        const errJson = await res.json();
        if (errJson && typeof errJson.error === 'string') {
          serverMsg = errJson.error;
        }
      } catch {
        // Ignore JSON parse failures on non-JSON error responses
      }

      if (res.status === 429) {
        const retryAfter = res.headers.get('Retry-After');
        setError(
          serverMsg ||
            (retryAfter
              ? `Too many failed attempts. Try again in ${retryAfter}s.`
              : 'Too many failed attempts. Please wait before trying again.')
        );
      } else if (res.status === 401) {
        setError('Invalid username or password.');
      } else {
        setError(serverMsg || `Login error (HTTP ${res.status})`);
      }
    } catch (err: any) {
      setError(err?.message || 'Network error during login.');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="dtn-login-title"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 backdrop-blur-sm p-4"
    >
      <div class="w-full max-w-md rounded-2xl bg-white dark:bg-slate-800 border border-slate-200 dark:border-slate-700 shadow-2xl p-6 sm:p-8 space-y-6">
        <div class="flex items-center space-x-3">
          <div class="flex items-center justify-center w-11 h-11 rounded-xl bg-gradient-to-tr from-emerald-600 to-teal-400 text-white shadow-md shrink-0">
            <span class="text-2xl">🔐</span>
          </div>
          <div>
            <h2
              id="dtn-login-title"
              class="text-lg font-bold text-slate-900 dark:text-white"
            >
              DTN Station Authentication
            </h2>
            <p class="text-xs text-slate-500 dark:text-slate-400">
              {stationCallsign
                ? `Station ${stationCallsign} — Protected Access`
                : 'Please sign in to access the console.'}
            </p>
          </div>
        </div>

        {error && (
          <div
            role="alert"
            class="p-3 rounded-xl bg-red-50 dark:bg-red-950/60 border border-red-300 dark:border-red-800 text-red-800 dark:text-red-300 text-xs font-medium flex items-start space-x-2"
          >
            <span>⚠️</span>
            <span>{error}</span>
          </div>
        )}

        <form onSubmit={handleSubmit} class="space-y-4">
          <div>
            <label
              for="login-username"
              class="block text-xs font-semibold text-slate-700 dark:text-slate-300 mb-1"
            >
              Callsign / Username
            </label>
            <input
              id="login-username"
              type="text"
              autocomplete="username"
              value={username}
              onInput={(e: any) => setUsername(e.target.value)}
              placeholder="e.g. f4abc or admin"
              required
              disabled={submitting}
              class="w-full px-3.5 py-2 text-sm rounded-xl border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900 text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:opacity-60"
            />
          </div>

          <div>
            <label
              for="login-password"
              class="block text-xs font-semibold text-slate-700 dark:text-slate-300 mb-1"
            >
              Password
            </label>
            <input
              id="login-password"
              type="password"
              autocomplete="current-password"
              value={password}
              onInput={(e: any) => setPassword(e.target.value)}
              placeholder="••••••••"
              required
              disabled={submitting}
              class="w-full px-3.5 py-2 text-sm rounded-xl border border-slate-300 dark:border-slate-600 bg-white dark:bg-slate-900 text-slate-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:opacity-60"
            />
          </div>

          <button
            type="submit"
            disabled={submitting}
            class="w-full py-2.5 px-4 rounded-xl bg-emerald-600 hover:bg-emerald-700 text-white text-sm font-semibold shadow-sm transition-colors cursor-pointer disabled:opacity-50"
          >
            {submitting ? 'Signing in...' : 'Sign in'}
          </button>
        </form>
      </div>
    </div>
  );
}
