import GLib from 'gi://GLib';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

export default class WxshotNoFlashExtension extends Extension {
    enable() {
        this._originals = new Map();
        this._patched = false;
        this._timer = 0;
        this._tryPatch();
        if (!this._patched) {
            this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
                this._tryPatch();
                return this._patched ? GLib.SOURCE_REMOVE : GLib.SOURCE_CONTINUE;
            });
        }
    }

    _tryPatch() {
        const service = Main.shellDBusService?._screenshotService;
        if (!service) return;

        const wrap = (name, flashIndex) => {
            const original = service[name];
            if (typeof original !== 'function') return;
            this._originals.set(name, original);
            service[name] = async (params, invocation) => {
                const args = params.slice();
                if (flashIndex >= 0 && flashIndex < args.length) args[flashIndex] = false;
                return await original.call(service, args, invocation);
            };
        };

        wrap('ScreenshotAsync', 1);
        wrap('ScreenshotWindowAsync', 2);
        wrap('ScreenshotAreaAsync', 4);

        const flash = service.FlashAreaAsync;
        if (typeof flash === 'function') {
            this._originals.set('FlashAreaAsync', flash);
            service.FlashAreaAsync = async () => {};
        }

        this._patched = true;
        console.log('[wxshot-noflash] patched: ' + [...this._originals.keys()].join(', '));
    }

    disable() {
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        const service = Main.shellDBusService?._screenshotService;
        if (service) {
            for (const [name, fn] of this._originals) service[name] = fn;
        }
        this._originals = null;
        console.log('[wxshot-noflash] disabled');
    }
}
