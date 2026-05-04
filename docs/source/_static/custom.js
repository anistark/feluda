// Custom js

document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('a[href*="github.com"], a[href*="discord.gg"], a[href*="crates.io"]').forEach(function(link) {
        link.setAttribute('target', '_blank');
        link.setAttribute('rel', 'noopener noreferrer');
    });

    // Fetch and render contributors
    const contributorsContainer = document.getElementById('contributors-container');
    if (contributorsContainer) {
        (() => {
            const CACHE_KEY = 'feluda_contributors';
            const CACHE_TTL = 24 * 60 * 60 * 1000; // 24 hours

            const renderContributors = (contributors) => {
                const filtered = contributors.filter(c => !c.login.includes('[bot]') && c.login !== 'dependabot');
                contributorsContainer.innerHTML = filtered.map(contributor => `
                    <a href="${contributor.html_url}" target="_blank" rel="noopener noreferrer" title="${contributor.login}">
                        <img src="${contributor.avatar_url}" alt="${contributor.login}" class="contributor-avatar" />
                    </a>
                `).join('');
            };

            const showFallback = () => {
                contributorsContainer.innerHTML = '<a href="https://github.com/anistark/feluda/graphs/contributors" target="_blank" rel="noopener noreferrer">View contributors on GitHub</a>';
            };

            // Check cache first
            const cached = localStorage.getItem(CACHE_KEY);
            if (cached) {
                const { data, timestamp } = JSON.parse(cached);
                if (Date.now() - timestamp < CACHE_TTL) {
                    renderContributors(data);
                    return;
                }
            }

            // Fetch from API
            fetch('https://api.github.com/repos/anistark/feluda/contributors')
                .then(response => {
                    if (!response.ok) throw new Error(`HTTP ${response.status}`);
                    return response.json();
                })
                .then(contributors => {
                    localStorage.setItem(CACHE_KEY, JSON.stringify({ data: contributors, timestamp: Date.now() }));
                    renderContributors(contributors);
                })
                .catch(error => {
                    console.error('Failed to fetch contributors:', error);
                    // Try to use stale cache if available
                    if (cached) {
                        renderContributors(JSON.parse(cached).data);
                    } else {
                        showFallback();
                    }
                });
        })();
    }

    // Fetch and render Field Reports timeline
    const fieldReportsContainer = document.getElementById('field-reports-container');
    if (fieldReportsContainer) {
        const escapeHtml = (str) => String(str ?? '').replace(/[&<>"']/g, (c) => ({
            '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
        }[c]));

        const formatDate = (iso) => {
            const d = new Date(iso);
            if (isNaN(d)) return iso;
            return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
        };

        const typeMeta = {
            video:     { label: 'Video',          icon: '▶' },
            slides:    { label: 'Slides',         icon: '📄' },
            upcoming:  { label: 'Upcoming',       icon: '🔮' },
            article:   { label: 'Article',        icon: '✍' },
            podcast:   { label: 'Podcast',        icon: '🎙' },
            talk:      { label: 'Talk',           icon: '🎤' },
            lightning: { label: 'Lightning Talk', icon: '⚡' }
        };

        const renderReports = (reports) => {
            if (!Array.isArray(reports) || reports.length === 0) {
                fieldReportsContainer.innerHTML = '<p class="field-reports-empty">No field reports yet — check back soon.</p>';
                return;
            }

            const sorted = [...reports].sort((a, b) => new Date(b.date) - new Date(a.date));

            const items = sorted.map((r) => {
                const type = (r.type || '').toLowerCase();
                const meta = typeMeta[type] || { label: type || 'Talk', icon: '•' };
                const isUpcoming = type === 'upcoming' || new Date(r.date) > new Date();
                const url = r.url ? escapeHtml(r.url) : '';
                const titleHtml = url
                    ? `<a href="${url}" target="_blank" rel="noopener noreferrer">${escapeHtml(r.title)}</a>`
                    : escapeHtml(r.title);

                const bylineParts = [];
                if (r.speaker) bylineParts.push(`<span class="field-report-speaker">${escapeHtml(r.speaker)}</span>`);
                if (r.venue) bylineParts.push(`<span class="field-report-venue">${escapeHtml(r.venue)}</span>`);
                const bylineHtml = bylineParts.join('<span class="field-report-byline-sep" aria-hidden="true"> · </span>');

                const itemClasses = [
                    'field-report-item',
                    isUpcoming ? 'is-upcoming' : '',
                    url ? 'has-link' : ''
                ].filter(Boolean).join(' ');

                return `
                    <li class="${itemClasses}" data-type="${escapeHtml(type)}">
                        <div class="field-report-rail">
                            <span class="field-report-dot" aria-hidden="true"></span>
                        </div>
                        <div class="field-report-content">
                            <div class="field-report-meta">
                                <time datetime="${escapeHtml(r.date)}">${escapeHtml(formatDate(r.date))}</time>
                                <span class="field-report-chip">${meta.icon} ${escapeHtml(meta.label)}</span>
                            </div>
                            <h3 class="field-report-title">${titleHtml}</h3>
                            ${bylineHtml ? `<p class="field-report-byline">${bylineHtml}</p>` : ''}
                            ${r.description ? `<p class="field-report-desc">${escapeHtml(r.description)}</p>` : ''}
                        </div>
                    </li>
                `;
            }).join('');

            fieldReportsContainer.innerHTML = `<ol class="field-reports-timeline">${items}</ol>`;
        };

        const showReportsFallback = (errMsg) => {
            const detail = errMsg ? ` <code>${escapeHtml(errMsg)}</code>` : '';
            fieldReportsContainer.innerHTML = `<p class="field-reports-empty">Field reports unavailable right now.${detail}</p>`;
        };

        // Resolve URL relative to the loaded custom.js script so it works
        // regardless of page depth or deployment subpath.
        let reportsUrl = '_static/field-reports.json';
        const scriptEl = document.querySelector('script[src*="custom.js"]');
        if (scriptEl) {
            try {
                reportsUrl = new URL('field-reports.json', scriptEl.src).href;
            } catch (e) {
                // fall through to relative fallback
            }
        }

        fetch(reportsUrl, { cache: 'no-cache' })
            .then(response => {
                if (!response.ok) throw new Error(`HTTP ${response.status} ${response.statusText}`);
                return response.json();
            })
            .then(renderReports)
            .catch(error => {
                console.error('Failed to load field reports:', error);
                showReportsFallback(error.message || String(error));
            });
    }
});
