// Common sanitized markdown and HTML utility for dtn-postmaster web interface

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

export function escapeAttribute(attr: string): string {
  return attr
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

// Validates whether an image URL is safe.
// By default, allows only raster data:image URIs (png, jpeg, gif, webp — no SVG) and relative paths.
// Remote HTTP/HTTPS images require explicit opt-in via allowExternalImages = true.
export function isSafeImageUrl(
  url: string,
  allowExternalImages: boolean = false
): boolean {
  const trimmed = url.trim();
  if (!trimmed || /[\x00-\x1f\x7f]/.test(trimmed)) {
    return false;
  }
  const isRasterDataUri =
    /^data:image\/(?:png|jpeg|jpg|gif|webp);base64,[A-Za-z0-9+/=\s]+$/i.test(trimmed);
  if (isRasterDataUri) {
    return true;
  }
  // Safe relative paths (not protocol-relative //)
  if (!trimmed.startsWith('//') && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed)) {
    return /^[a-zA-Z0-9\-._~/?#=&%]+$/i.test(trimmed);
  }
  if (allowExternalImages) {
    return /^https?:\/\/[a-zA-Z0-9\-._~:/?#[\]@!$&'()*+,;=]+$/i.test(trimmed);
  }
  return false;
}

// Validates whether a link URL is safe (http, https, dtn, mailto, tel, relative paths)
export function isSafeLinkUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!trimmed) return false;

  // Disallow control characters or newlines
  if (/[\x00-\x1f\x7f]/.test(trimmed)) {
    return false;
  }

  // Reject URLs containing HTML entity escapes (e.g. &#, &colon;) to prevent scheme obfuscation
  if (/&[#a-zA-Z0-9]+;/.test(trimmed)) {
    return false;
  }

  // Check if it has a URI scheme before any '/', '?', or '#'
  // RFC 3986: scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
  const schemeMatch = trimmed.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):/);
  if (schemeMatch) {
    const scheme = schemeMatch[1].toLowerCase();
    return ['http', 'https', 'dtn', 'mailto', 'tel'].includes(scheme);
  }

  // Protocol-relative URLs: allow if followed by valid characters
  if (trimmed.startsWith('//')) {
    return /^\/\/[a-zA-Z0-9\-._~:/?#[\]@!$&'()*+,;=]+$/i.test(trimmed);
  }

  // Relative URLs: path, query, hash (e.g. "src/bin/dtnprint.rs", "./foo", "../bar", "/path", "#anchor")
  return /^[a-zA-Z0-9\-._~:/?#[\]@!$&'()*+,;=]+$/i.test(trimmed);
}

function generateNonce(): string {
  if (typeof globalThis.crypto !== 'undefined' && typeof globalThis.crypto.getRandomValues === 'function') {
    const buf = new Uint32Array(2);
    globalThis.crypto.getRandomValues(buf);
    return buf[0].toString(36) + buf[1].toString(36);
  }
  return Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
}

export function renderMarkdown(
  content: string,
  options: { allowImages?: boolean; allowExternalImages?: boolean } = {}
): string {
  const placeholders: { token: string; html: string }[] = [];
  const nonce = generateNonce();
  let tokenCounter = 0;

  function createPlaceholder(html: string): string {
    // Use control-char delimiter + per-call random alphanumeric nonce without '_' or '*'
    // so formatting regexes cannot alter placeholders and attackers cannot forge tokens.
    const token = `\x00DTNMD${nonce}X${tokenCounter++}\x00`;
    placeholders.push({ token, html });
    return token;
  }

  // Strip NUL bytes from untrusted input so placeholder delimiters cannot be injected
  let textToProcess = content.replace(/\x00/g, '');

  // 1. Fenced code blocks ```code```
  textToProcess = textToProcess.replace(/```([a-zA-Z0-9_-]*)\n?([\s\S]*?)```/g, (_match, _lang, code) => {
    return createPlaceholder(
      `<pre class="my-2 p-3 bg-slate-900 text-slate-100 rounded-xl text-xs font-mono overflow-x-auto whitespace-pre-wrap">${escapeHtml(code.trim())}</pre>`
    );
  });

  // 2. Inline code `code`
  textToProcess = textToProcess.replace(/`([^`]+)`/g, (_match, code) => {
    return createPlaceholder(
      `<code class="px-1.5 py-0.5 bg-slate-200 dark:bg-slate-700 rounded text-xs font-mono">${escapeHtml(code)}</code>`
    );
  });

  // 3. Images ![alt](url)
  if (options.allowImages) {
    textToProcess = textToProcess.replace(/!\[([^\]]*)\]\(([^)\s]+(?:\s+[^)]*)?)\)/g, (_match, alt, rawUrl) => {
      const cleanUrl = rawUrl.trim();
      if (!isSafeImageUrl(cleanUrl, Boolean(options.allowExternalImages))) {
        return `[Image blocked: unsafe URL]`;
      }
      const safeAlt = escapeHtml(alt || 'Image');
      const safeUrl = escapeAttribute(cleanUrl);
      const tag = `<div class="my-3"><img src="${safeUrl}" alt="${safeAlt}" class="max-h-96 rounded-xl shadow border border-slate-200 dark:border-slate-700 object-contain mx-auto" /><p class="text-center text-xs text-slate-500 dark:text-slate-400 mt-1">${safeAlt}</p></div>`;
      return createPlaceholder(tag);
    });
  }

  // 4. Markdown links: [label](url "optional title") — linear regex without nested quantifiers
  const linkRegex = /\[([^\]]+)\]\(\s*([^)\s]+)(?:\s+"([^"]{0,200})"|\s+'([^']{0,200})')?\s*\)/g;
  textToProcess = textToProcess.replace(linkRegex, (_match, label, url, dqTitle, sqTitle) => {
    const cleanUrl = url.trim();
    if (!isSafeLinkUrl(cleanUrl)) {
      return `[${escapeHtml(label)}](unsafe link blocked)`;
    }
    const safeUrl = escapeAttribute(cleanUrl);
    const title = dqTitle ?? sqTitle;
    const titleAttr = title ? ` title="${escapeAttribute(title)}"` : '';
    let formattedLabel = escapeHtml(label);
    formattedLabel = formattedLabel.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    formattedLabel = formattedLabel.replace(/\*([^*]+)\*/g, '<em>$1</em>');
    formattedLabel = formattedLabel.replace(/_([^_]+)_/g, '<em>$1</em>');
    const tag = `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer"${titleAttr} class="text-emerald-600 dark:text-emerald-400 underline hover:opacity-85">${formattedLabel}</a>`;
    return createPlaceholder(tag);
  });

  // 5. Angle-bracketed autolinks <https://...> / <dtn://...> before escapeHtml
  textToProcess = textToProcess.replace(
    /<((?:https?|dtn):\/\/[a-zA-Z0-9\-._~:/?#[\]@!$&'()*+,;=]+)>/g,
    (_match, url) => {
      if (!isSafeLinkUrl(url)) {
        return escapeHtml(`<${url}>`);
      }
      const safeUrl = escapeAttribute(url);
      return createPlaceholder(
        `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer" class="text-emerald-600 dark:text-emerald-400 underline hover:opacity-85">${escapeHtml(url)}</a>`
      );
    }
  );

  // 6. Bare autolinks before escapeHtml so '&' / '_' in URLs aren't corrupted
  textToProcess = textToProcess.replace(
    /(?:https?|dtn):\/\/[a-zA-Z0-9\-._~:/?#[\]@!$&*+,;=()]+/g,
    (match) => {
      let url = match;
      let trailing = '';
      while (/[.,;:!?)]$/.test(url)) {
        if (url.endsWith(')')) {
          const openCount = (url.match(/\(/g) || []).length;
          const closeCount = (url.match(/\)/g) || []).length;
          if (closeCount <= openCount) {
            break;
          }
        }
        trailing = url.slice(-1) + trailing;
        url = url.slice(0, -1);
      }
      if (!isSafeLinkUrl(url)) {
        return escapeHtml(match);
      }
      const safeUrl = escapeAttribute(url);
      return createPlaceholder(
        `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer" class="text-emerald-600 dark:text-emerald-400 underline hover:opacity-85">${escapeHtml(url)}</a>`
      ) + trailing;
    }
  );

  // 7. Escape HTML on remaining text
  let formatted = escapeHtml(textToProcess);

  // 8. Headings
  formatted = formatted.replace(/^### (.*$)/gm, '<h3 class="text-base font-bold text-slate-900 dark:text-white mt-3 mb-1">$1</h3>');
  formatted = formatted.replace(/^## (.*$)/gm, '<h2 class="text-lg font-bold text-emerald-600 dark:text-emerald-400 mt-4 mb-2">$1</h2>');
  formatted = formatted.replace(/^# (.*$)/gm, '<h1 class="text-xl font-extrabold text-slate-900 dark:text-white mt-4 mb-2 pb-1 border-b border-slate-200 dark:border-slate-700">$1</h1>');

  // 9. Bold & Italic
  formatted = formatted.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  formatted = formatted.replace(/\*([^*]+)\*/g, '<em>$1</em>');
  formatted = formatted.replace(/_([^_]+)_/g, '<em>$1</em>');

  // 10. Checklists / Task lists (GFM & variants: - [ ], - [x], - [X], * [ ], + [x], 1. [ ], etc.)
  formatted = formatted.replace(
    /^([ \t]*)(?:[-*+]|\d+\.)\s*\[([ xXvV+*\-]*)\]\s*(.*$)/gm,
    (_match, indent, mark, text) => {
      const margin = indent.length >= 2 ? 'ml-8' : 'ml-4';
      const cleanMark = mark.trim().toLowerCase();
      const isChecked = ['x', 'v', '+', '*'].includes(cleanMark);
      const isCancelled = cleanMark === '-';

      let textSpan = '';
      let checkboxAttr = '';

      if (isChecked) {
        checkboxAttr = 'checked disabled';
        textSpan = `<span class="line-through text-slate-400 dark:text-slate-500">${text}</span>`;
      } else if (isCancelled) {
        checkboxAttr = 'disabled';
        textSpan = `<span class="line-through text-slate-400 dark:text-slate-500 opacity-75">${text}</span>`;
      } else {
        checkboxAttr = 'disabled';
        textSpan = `<span>${text}</span>`;
      }

      return `<li class="${margin} list-none flex items-center space-x-2 my-1 text-slate-800 dark:text-slate-200"><input type="checkbox" ${checkboxAttr} class="rounded border-slate-300 dark:border-slate-600 text-emerald-600 focus:ring-0 cursor-default h-3.5 w-3.5" />${textSpan}</li>`;
    }
  );

  // 11. Bullet lists
  formatted = formatted.replace(/^[-*+] (.*$)/gm, '<li class="ml-4 list-disc">$1</li>');

  // Avoid extra line breaks between list items and block elements
  formatted = formatted.replace(/(<\/(?:li|pre|h[1-3])>)\n/g, '$1');

  // 12. Line breaks
  formatted = formatted.replace(/\n/g, '<br/>');

  // 13. Restore placeholders in reverse order
  for (let i = placeholders.length - 1; i >= 0; i--) {
    const item = placeholders[i];
    formatted = formatted.replaceAll(item.token, item.html);
  }

  return formatted;
}
