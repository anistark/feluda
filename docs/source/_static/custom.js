// Custom js

document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('a[href*="github.com"], a[href*="discord.gg"]').forEach(function(link) {
        link.setAttribute('target', '_blank');
        link.setAttribute('rel', 'noopener noreferrer');
    });
});
