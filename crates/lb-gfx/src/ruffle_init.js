// WebKitGTK can expose POSIX locales (e.g. C or en_US.UTF-8) through
// navigator. Ruffle's constructor passes language directly to Intl.Locale;
// an invalid tag aborts custom-element construction before ruffle() exists.
(() => {
    const valid = value => {
        try { return new Intl.Locale(value).toString(); }
        catch { return null; }
    };
    if (!valid(navigator.language)) {
        Object.defineProperty(navigator, "language", { value: "en-US", configurable: true });
    }
    const languages = navigator.languages.filter(value => valid(value));
    if (languages.length !== navigator.languages.length || languages.length === 0) {
        Object.defineProperty(navigator, "languages", {
            value: languages.length ? languages : [navigator.language], configurable: true
        });
    }
})();
