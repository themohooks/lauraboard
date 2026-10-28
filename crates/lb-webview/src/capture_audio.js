// Route Web Audio into the broadcast pipe, never the desktop speakers.
(() => {
    const Native = window.AudioContext || window.webkitAudioContext;
    if (!Native) return;
    class CaptureContext extends Native {
        constructor(options) {
            super({ ...options, sampleRate: 48000 });
            const destination = this.destination;
            const tap = this.createScriptProcessor(2048, 2, 2);
            tap.onaudioprocess = event => {
                const input = event.inputBuffer;
                const bytes = new Uint8Array(input.length * 8);
                const view = new DataView(bytes.buffer);
                for (let i = 0; i < input.length; i++) {
                    for (let c = 0; c < 2; c++) {
                        view.setFloat32((i * 2 + c) * 4, input.getChannelData(Math.min(c, input.numberOfChannels - 1))[i], true);
                    }
                }
                let binary = "";
                for (const byte of bytes) binary += String.fromCharCode(byte);
                window.ipc.postMessage(JSON.stringify({ pcm: btoa(binary) }));
            };
            tap.connect(destination);
            Object.defineProperty(this, "destination", { value: tap });
            this.resume().catch(() => {});
        }
    }
    window.AudioContext = CaptureContext;
    if (window.webkitAudioContext) window.webkitAudioContext = CaptureContext;
})();
