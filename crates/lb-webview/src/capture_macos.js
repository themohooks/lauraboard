// WKWebView does not deliver animation frames to a hidden native window.
// The capture host still needs them for Ruffle's render loop and HTML animation.
// Keep the normal scheduler when visible and preserve cancellation semantics.
(() => {
    const request = window.requestAnimationFrame.bind(window);
    const cancel = window.cancelAnimationFrame.bind(window);
    const timers = new Map();
    let next = -1;
    window.requestAnimationFrame = callback => {
        if (!document.hidden) return request(callback);
        const id = next--;
        timers.set(id, setTimeout(() => {
            timers.delete(id);
            callback(performance.now());
        }, 1000 / 60));
        return id;
    };
    window.cancelAnimationFrame = id => {
        if (timers.has(id)) {
            clearTimeout(timers.get(id));
            timers.delete(id);
        } else {
            cancel(id);
        }
    };
})();
