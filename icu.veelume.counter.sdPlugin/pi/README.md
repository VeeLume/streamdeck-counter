# Property Inspector Notes

## sdpi-components

We use **sdpi-components v4** (local file — `pi/sdpi-components.js`). Do not reference
the CDN or any external URL. Include it once in `<head>` with no CSS link needed
(v4 bundles its own styles):

```html
<head>
    <meta charset="utf-8" />
    <script src="sdpi-components.js"></script>
</head>
```

There is no separate CSS file in v4 — do not add a `<link>` for it.

---

## How the PI connects to the plugin

Stream Deck invokes `connectElgatoStreamDeckSocket` on `window` after the DOM loads.
The signature is:

```js
window.connectElgatoStreamDeckSocket = function(
    port,           // WebSocket port number (string)
    uuid,           // action instance context UUID
    registerEvent,  // "registerPropertyInspector"
    info,           // JSON string — GlobalSettings / app/device info
    actionInfo,     // JSON string — action UUID, settings, coordinates, state
) { ... }
```

**sdpi-components v4 handles all of this automatically.** Just include the script tag and
use the custom elements — no manual `connectElgatoStreamDeckSocket` needed.

---

## Receiving messages from the plugin (`sendToPropertyInspector`)

When the plugin calls `cx.sd().send_to_property_inspector(ctx_id, payload)`, the PI
receives a WebSocket message with this structure:

```json
{
    "action":  "icu.veelume.starcitizen.execute-action",
    "context": "<context-id>",
    "event":   "sendToPropertyInspector",
    "payload": { ...your payload... }
}
```

### With sdpi-components v4

```js
SDPIComponents.streamDeckClient.sendToPropertyInspector.subscribe((msg) => {
    // msg is the full event object; msg.payload is what the plugin sent
    const data = msg.payload;
    if (data.type === 'actionInfo') {
        document.getElementById('actionIdLabel').textContent = data.actionId || '—';
    }
});
```

### Without sdpi-components (manual WebSocket)

```js
window.connectElgatoStreamDeckSocket = function(port, uuid, registerEvent, info, actionInfo) {
    const ws = new WebSocket('ws://127.0.0.1:' + port);
    ws.onopen = () => ws.send(JSON.stringify({ event: registerEvent, uuid }));
    ws.onmessage = (evt) => {
        const msg = JSON.parse(evt.data);
        if (msg.event === 'sendToPropertyInspector') {
            handlePluginMessage(msg.payload);
        }
    };
};
```

> **Do NOT use `window.addEventListener('message', ...)`** — that is the browser
> `postMessage` API for cross-origin frames and has nothing to do with the Stream Deck
> WebSocket channel.

---

## Sending messages to the plugin (`sendToPlugin`)

```js
SDPIComponents.streamDeckClient.send('sendToPlugin', { action: 'refresh' });
```

The plugin receives this in `did_receive_property_inspector_message`.

---

## RegistrationInfo type (from `info` param)

```ts
type RegistrationInfo = {
    application: { language: string; platform: string; version: string };
    plugin:      { uuid: string; version: string };
    devicePixelRatio: number;
    colors: { buttonPressedBorderColor: string; /* ... */ };
    devices: Array<{ id: string; name: string; size: { columns: number; rows: number }; type: number }>;
};
```

---

## sdpi-components v4 reference

- Homepage: https://sdpi-components.dev
- Component reference: https://sdpi-components.dev/docs/components
