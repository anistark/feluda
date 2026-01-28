// Custom js

document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('a[href*="github.com"], a[href*="discord.gg"]').forEach(function(link) {
        link.setAttribute('target', '_blank');
        link.setAttribute('rel', 'noopener noreferrer');
    });

    // Fetch and render contributors
    const contributorsContainer = document.getElementById('contributors-container');
    if (contributorsContainer) {
        fetch('https://api.github.com/repos/anistark/feluda/contributors')
            .then(response => response.json())
            .then(contributors => {
                const filtered = contributors.filter(c => !c.login.includes('[bot]') && c.login !== 'dependabot');
                contributorsContainer.innerHTML = filtered.map(contributor => `
                    <a href="${contributor.html_url}" target="_blank" rel="noopener noreferrer" title="${contributor.login}">
                        <img src="${contributor.avatar_url}" alt="${contributor.login}" class="contributor-avatar" />
                    </a>
                `).join('');
            })
            .catch(error => {
                console.error('Failed to fetch contributors:', error);
                contributorsContainer.innerHTML = '<p>Unable to load contributors.</p>';
            });
    }
});
