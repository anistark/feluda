// Custom js

document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('a[href*="github.com"], a[href*="discord.gg"], a[href*="crates.io"]').forEach(function(link) {
        link.setAttribute('target', '_blank');
        link.setAttribute('rel', 'noopener noreferrer');
    });

    // Fetch and render contributors
    const contributorsContainer = document.getElementById('contributors-container');
    if (contributorsContainer) {
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
    }
});
