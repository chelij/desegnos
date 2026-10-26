# Desegnos technical guide

Desegnos opens and saves `.desegnos` projects using project schema 10 and generation requirements format 27. Only current formats are supported. Saves preserve the preceding archive as `.previous`; current-format journals can recover work after a crash without resubmitting provider requests. The owner moves their app data manually.


In AI settings, **Image generation** is enabled for OpenAI connections with confirmed
Image API access. New images use GPT Image 2.5 Flare; edits use Sunburst with the supplied
attachment or project picture. Edits can include a PNG alpha mask; transparent cut-outs
stay PNG. Up to two 1024×1024 images are attempted per job. The USD 1 allowance remains
part of the estimate and spending-limit check; actual charges may vary. Stored results
may be reduced to fit the 256 KiB image limit. Failed images leave labelled slots while
the rest of the draft applies. The generation log shows the resulting images and prompts.

Desegnos checks the active connection first with a free invalid-size request. If that
credential cannot use images, **Add image key** in the Features row connects a separate
Image API key using secure storage (or this session only). The row shows which credential
images use. This lets text stay on your ChatGPT plan while images are billed to the key.
Some keys require **API Organization Verification**. Checks that fail transiently remain
unknown. The probe never requests a real image. Other providers omit this capability.


Desegnos is a Rust desktop HTML editor built around pointing at a page and describing what belongs there. The full product contract is in [RELEASE_PLAN.md](../RELEASE_PLAN.md), with core and plugin responsibilities in [PLUGIN_REVIEW.md](../PLUGIN_REVIEW.md).

This development build implements project authoring, decision memory, persistence and generation of HTML, CSS and browser JavaScript with an API key or an authorized ChatGPT plan. It is not the 1.0 release. Live OpenAI acceptance requires private sign-in or key entry in the application; controlled HTTP verification does not establish that result. [docs/IMPLEMENTATION.md](IMPLEMENTATION.md) records the checks and remaining release work.

## Run the desktop editor

Install Rust through rustup, Node.js, pnpm, and the native prerequisites for [Tauri 2](https://v2.tauri.app/start/prerequisites/). Linux needs GTK 3, WebKitGTK 4.1, JavaScriptCoreGTK 4.1, libsoup 3 and a C/C++ toolchain. System key storage uses Secret Service on Linux, Keychain on macOS and Credential Manager on Windows. Windows and macOS need their corresponding native prerequisites and independent verification.

The project pins Rust 1.99.1 and pnpm 11.25.0. Scripts recognize the matching rustup stable installation even when its bin directory has not reached the shell's PATH.

```sh
pnpm install --frozen-lockfile
pnpm desktop dev
```

After a normal release build, launch `target/release/desegnos` from the project folder. On Linux with the NVIDIA kernel module loaded and a Wayland connection, Desegnos applies the confirmed `__NV_DISABLE_EXPLICIT_SYNC=1` startup workaround before GTK initializes. Explicit user values take precedence; X11-only launches and other GPU drivers keep their existing environment. This setting belongs only to Desegnos's process. See the [startup correction record](IMPLEMENTATION.md#nvidiawayland-startup-correction).

Choose **Blank website** or **Use my existing brand**, then name the project and choose a **Web/Mobile** screen guide and resolution. Custom dimensions are available. Every new project starts in the editor's current light or dark mode; use the sun/moon appearance control to switch it later. Each page opens as a browser-style tab with a full blank workspace around its screen guide. Double-click to place plain text boxes anywhere, including above or beside the guide. There are no sidebars. Position and size describe intent; the agent coordinates the site's actual layout.

The grid starts on. **Grid and rulers** sets pixel or percentage spacing, snapping, and pixel or percentage rulers. Rulers measure against the reference screen; negative values and values above 100% are valid. Pan by dragging empty space with the left mouse button, the middle mouse button or Space and drag. Shift+drag marks an area; Ctrl+wheel zooms around the pointer. You can scroll sideways with Shift and the wheel, or use **Menu → View → Center screen guide**. Each page remembers its view during the session. Request boxes keep the size you draw or resize, without a viewport-height cap or fixed canvas edge. The canvas repositions its scrolling window as you travel; use Requests to return to an area, including very large ones.

For keyboard placement on a blank page, focus the workspace and press Enter. Single-click a generated element to outline it; its top-left bar has Layers, Copy, Cut and Delete icons. Double-click text to edit locally. Right-click an element for Edit, Replace, Ask AI, Move or resize, clipboard and arrangement commands. Drag the selected element itself to move it or a corner to resize it. F2 opens Edit; Ctrl/Cmd+K opens its AI request. Right-click pending requests or blank canvas for their own commands. Text selections retain the normal native text menu. Arrow keys move a selected element by one pixel and Shift+arrows by ten. Enter selects the next element, Alt+Enter selects its parent, and Shift+Enter adds an area. Request grips support arrow keys and the chosen snap spacing; Alt makes fine adjustments. Escape cancels a drag or closes a menu/dialog.

Empty requests disappear when you select something else; typed requests stay. Undo restores abandoned requests. Panning and zooming change only your view.

**Menu → Keyboard shortcuts** (F1) lists the controls. Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z or Ctrl+Y redoes, Alt+Z steps only the selected element back to its previous version (also the undo-arrow button beside the selection and **Undo changes to this element** in its right-click menu; each press goes one version further back, other changes stay, and the main Undo reverts it; CSS rules added for that element are not reverted), +/− zooms, 0 fits the guide, 1 shows actual size, and 2 shows 200%. Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+Page Down / Page Up switch pages. New/Open/Save use Ctrl/Cmd+N/O/S; Save as uses Ctrl/Cmd+Shift+S. Delete/Backspace removes a selected request with confirmation. Native text editing and text undo keep working inside prompts. These follow established [Photoshop](https://helpx.adobe.com/photoshop/web/get-set-up/preferences-and-settings/keyboard-shortcuts.html) and [editor tab](https://code.visualstudio.com/docs/editing/getting-started/userinterface) conventions.

The editor follows the supplied Desegnos identity: local monochrome wordmarks, neutral grey light/dark chrome, functional blue, Archivo headings and system controls. The bundled Archivo font includes its SIL Open Font License under `public/licenses/Archivo-OFL.txt`.

**Project brief** holds the brief, accepted choices, and creative exceptions. Desegnos also records each completed generated draft automatically, including its summary, proposed choices, feedback, and attempt provenance. Generated identity choices remain suggestions until you explicitly save a project choice. Accepted choices and protected rules remain authoritative. The project brief and its `PROJECT.md` persist with the project; Undo marks an applied generation as undone without erasing its record.

## Import an existing brand

**Import context** reads a website address, folder, ZIP or project. Website addresses
include up to 20 linked pages on the same site. In the review, untick any photos or
existing work you do not want to add. WebP photos are supported alongside PNG/JPEG.
The AI log includes the exact follow-up context, scrubbed failed replies and a
read-only **Past generations** list. These records stay on this device.

Review the findings, their source files, uncertainty and suggested logos. Uncheck anything you do not want. **Create project** makes an empty light page with the approved identity choices and portable logo bytes in its project memory. With **Use it for my brand only**, the source website stays separate from the new canvas. Choose **Edit this website** instead to load its existing pages without AI. The project brief records the imported identity, and later generation requirements include the approved choices and actual reusable images.

## Generate with your key

1. Open **AI** and choose **API key**. Add your key to device storage or choose the current session. Choose a model and API budget per generation; detailed model controls are under **Advanced settings**. **Test access** checks the connection without generating.
2. Describe at least one request on the site. Choose the single **Flexible/Precise** setting beside **Generate**. It applies to all filled requests across the project. Flexible allows more interpretation; Precise follows descriptions, relative placement and grouping closely while coordinating usable layout.
3. Click **Generate** once. Desegnos freezes the exact requirements Markdown, current affected-page source, global mode and provider settings, then sends the request. It uses your configured API request allowance.
4. The horizontal progress strip shows activity and elapsed time without blocking the canvas. Details opens the progress dialog. Only the submitted elements or areas blur; the rest of the canvas stays sharp. **Cancel generation** stops waiting. Closing generation details keeps the request running; open Details again without sending another request.
5. Valid output appears directly on the canvas after the safety checkpoint and stale-requirements checks pass. Completed prompt boxes move into history. Use Ask AI to open a change prompt beside an element, or Shift+drag to add something new. **Undo** restores the previous site and requests; **Redo** restores the result without another AI request. Errors and unresolved conflicts preserve the current site and requests. **Details → More → Advanced** holds exact requirements, source, model details and attempts.

**Menu → Advanced → View AI instructions** lets you inspect and save Markdown without provider setup or submission. Changes to dependent requests, identity, routes, appearance, mode or existing affected-page source block stale candidates. Restoring the exact dependencies, including by Undo, can restore a complete candidate for review without another request.

Keys remain in the native credential manager or session memory, outside project files, requirements, generation records and diagnostics. A session key overrides an existing system key temporarily; closing the application restores the saved system key. Remove deletes both only after system deletion succeeds. The native module does not silently fall back to plaintext storage.

Requests go directly to OpenAI using the user's key. Desegnos sends `store: false`; OpenAI's [data controls](https://developers.openai.com/api/docs/guides/your-data) still apply. Cost is a conservative, dated local estimate, not a guaranteed billing cap. Cancellation may leave charges for submitted work. Retries require explicit confirmation and may incur another charge. Interrupted submissions become uncertain on restart and never resubmit automatically.

## Generate with your ChatGPT plan

Open **AI settings**, choose **ChatGPT plan**, choose system or current-session storage, then **Continue with ChatGPT**. Authorize Desegnos in your system browser and explicitly grant plan usage. Eligible requests use that account's ChatGPT plan or credits balance through OpenAI's [official open-source integration](https://developers.openai.com/siwc/token-sharing-open-source). Desegnos verifies the signed identity before accepting it. **Got it** acknowledges the first plan-use confirmation; closing it only dismisses it temporarily.

Settings lets you add, select, reconnect or sign out of separate saved registrations. The model picker shows the intersection of the selected account's discovered models and Desegnos's verified catalogue. **Test access** refreshes availability without generation. **Manage usage** opens ChatGPT's usage settings. A valid identity without plan permission remains signed in but cannot generate.

Generate sends the same exact requirements file and global Flexible/Precise setting. Preparation freezes the chosen account and authentication method for that job, including explicit retries. Switching accounts or choosing API key later cannot redirect it. Technical details show plan/credit use instead of an API price estimate or an unsupported API output cap. Saved credentials are reused; generation reloads account model availability when its in-memory catalogue is empty. Plan limits and failed streams stop generation without billing fallback or automatic retries.

OAuth access, refresh and ID tokens stay in Rust-owned secure storage or explicitly selected session memory. They never enter the editor, project, Markdown or job ledger. A session-only reconnect retires that registration's older system credential; closing Desegnos ends its access. Sign-out cancels bound work, attempts remote session revocation and clears local tokens while keeping the registration for reconnect. If revocation cannot be confirmed, the app says so.

If secure storage fails after token rotation, keep Desegnos open and use **Repair secure storage** for the affected account. Rust retains the replacement privately and blocks generation until repair saves it; repair sends no OAuth or generation request. A process exit loses that unsaved replacement and can require reconnecting. The [ChatGPT contract](CHATGPT_CONTRACT.md) documents protocol, storage and isolation details.

## Generated source and preview

Pages store actual HTML body fragments, CSS and browser JavaScript. Source is available through **Menu → Advanced → Site source** or the optional generation details. Accepted source renders on the canvas; Ask AI on an element creates a source-bound structural target in the next requirements file. **Preview** opens a separate interactive preview.

Scripts start paused. The selectable canvas uses a permanently scriptless same-origin document so the trusted editor can inspect element bounds; navigation invalidates selection. Interactive preview uses its separate opaque-origin package. Paused frames prohibit website scripts across navigation. **Enable interactions** creates a separate opaque-origin package for trying buttons and internal links. Preview CSP blocks external network, frames, forms and unregistered resources; no provider key or editor command authority reaches website code. Native **Workspace → Reload workspace (pause website scripts)**, or Ctrl/Cmd+Shift+R, recreates the editor with scripts paused while preserving accepted native state. A native heartbeat watchdog also offers recovery when an enabled preview stops responding.

This gate supports native HTML/DOM capabilities. It has no qualified external component catalogue, React/JSX compiler, asset pipeline, package installation or external service bindings. Requirements state those capabilities explicitly; missing required dependencies must produce conflicts. Direct editing preserves nested text formatting, embedded local images, CSS and JavaScript, and uses the same saved project and Undo/Redo history. The first local edit normalizes HTML with the existing HTML5 parser; Undo restores the original source bytes. Local images can be PNG, JPEG, WebP, GIF or SVG up to 512 KB and 16,384 pixels per side, within the page source limit. Position changes use an individual CSS translate while preserving existing 2D transforms; perspective and 3D transforms require changing the layout instead. Complete React component integration, canonical document editing, breakpoint/state controls and React export remain separate release gates. This build sends complete affected-page source plus the selected element to the agent; the result remains complete page source.

## Saving and recovery

New projects create `<Location>/<Project Name>/<Project Name>.desegnos` alongside the live website files. The default location is OS Documents plus `Desegnos` (Linux uses XDG Documents, falling back to `~/Documents`). **Choose folder…** changes the parent for this project; **Default project location** on the creation screen or in Menu changes the persistent default, with Reset. Names are made safe across operating systems; collisions get ` 2`, ` 3`, etc. The first Save needs no dialog.

The archive retains full project context, including pictures, source, brief, decisions and notes, with validated JSON and `PROJECT.md`. Later saves preserve `.previous`. Each save commits the archive first, then stages and syncs the same public files as website export. `.desegnos-site.json` tracks owned files and interrupted writes. Untracked files and external edits are preserved; sync failures show a warning without losing the archive save. Save again to retry. New **Save as**/**Save as copy** destinations get their own folders; existing archives retain the conflict/Replace flow. Export remains a separate website-only folder.

**Open project** opens an archive; **Open folder…** or **Menu → Open project folder…** opens the newest immediate archive (filename breaks timestamp ties). A website folder offers **Edit this website** (the default) or **Use it for my brand only**. Editing imports up to 100 UTF-8 HTML pages using the real-page detector, preserving their original HTML and filenames, linked/inline CSS, scripts, local pictures and fonts. Editor IDs are the only import-time HTML changes. Local scripts run in Live/Preview, with relative page links and root-relative assets served from an isolated snapshot. Saving a plain site updates its files in place and keeps a one-time `.desegnos-original/` backup. Keep that backup and the hidden manifest. Unsupported files, including video/audio, are skipped and listed briefly.

Framework sites use fresh output from `dist/`, `build/`, `out/`, `_site/` or Hugo/Gatsby `public/`. Stale or missing output offers **Build site**, with progress, **Stop** and a five-minute timeout. Desegnos runs the project's build script using its declared package manager or lockfile, or the installed Hugo/Eleventy command. It never installs dependencies: missing `node_modules` shows the exact install command to run yourself. Framework projects are saved in a new folder at the configured project location, outside both the source and built output; edits do not change framework source files.

Each accepted edit first writes durable local recovery. Ownership locks prevent concurrent writers to the same project. Separate projects retain separate recoveries. Explicit recovery detaches when the original file is missing, damaged or no longer matches its identity/revision. Undo retains 200 in-memory steps and creates forward revisions. Restart restores accepted state, not the former undo stack. Consumed generation IDs survive Undo so duplicate Apply is harmless. Private generation records retain frozen requests, attempts, usage and safety checkpoints separately from project archives.

Desegnos keeps its own data (recoveries, recent projects, AI settings, usage records) in the OS application-data folder. At startup it checks that folder can be written. When it cannot — a read-only disk, a full disk, or a folder without permission — Desegnos says so and offers **Choose another folder…**; the chosen folder is remembered in `data-location.json` in the app's configuration folder. Setting `DESEGNOS_DATA_DIR` overrides both the platform default and a remembered choice for one run, which suits tests and portable installs. On Linux, file dialogs also need the desktop portal (`xdg-desktop-portal` plus a backend such as `xdg-desktop-portal-gtk`).

## Code boundaries

| Location | Responsibility |
| --- | --- |
| `crates/desegnos-core` | Project/source/candidate validation, signed placement bounds, grid settings, decisions/dossier, exact requirements, scoped transactions and undo/redo |
| `crates/desegnos-storage` | Bounded archives, integrity, atomic writes, backups, locks and durable recovery |
| `crates/desegnos-ai` | Fixed OpenAI HTTPS transport, native credentials, verified catalogue, estimates and bounded stream parsing; no project mutation |
| `src-tauri` | Native lifecycle/permissions, file dialogs, durable generation coordination, preview registry and application |
| `src` | Sidebar-free React editor, transient input, disclosure/review and display of native state |
| `tests/native` | Actual isolated Linux WebKitGTK workflows with real Rust commands and controlled HTTP |

Unknown versioned extension data survives saves and edits. That is persistence support; an executable plugin host remains a later release gate. Future plugins cannot bypass requirements assembly, permissions, core validation or transactional application. Provider adapters cannot install packages or mutate projects.

Dependencies have concrete roles: Tauri/React, Serde, UUID, SHA-256, ZIP and native dialogs form the existing foundation. Linux preview setup uses the same WebKitGTK binding as Tauri to allow anonymous reads of registered preview resources while retaining an opaque frame. The AI boundary uses established reqwest, Rustls, Tokio, keyring, Base64 and zeroize, plus jsonwebtoken for verified OpenID identity. The maintained `open` browser launcher, also used by Tauri, opens trusted sign-in and usage destinations from native code. Versions, licenses, advisories and limitations are recorded in [the provider review](../crates/desegnos-ai/README.md). The optional WebdriverIO Tauri driver, loopback endpoints and isolated vault adapters exist only in explicit debug tests; release compilation rejects them.

## Standalone website export

**Export** or **Menu → Project → Export website…** writes included pages into a new folder under your chosen destination. Open `index.html` in an ordinary browser. Desegnos writes relative page links, real styles/scripts and embedded image files; the output has no editor runtime. Prompts, brief, generation history and credentials are excluded. Hidden pages are omitted; included blank pages remain blank and are reported. External services, URLs, forms, remote fonts and module requirements still need their actual services. Export lists portability warnings rather than inventing a backend. React export remains later work. See [the export contract](WEBSITE_EXPORT.md).

## Update check

**Menu → Check for updates…** asks GitHub for the newest release of Desegnos and shows its version. If it is newer, **Download** opens that release page in your browser; Desegnos downloads and installs nothing itself. This is the only time the app contacts GitHub for updates: there is no check at startup. The request reads public release information only and sends no telemetry.

## Crash reports

After a crash or an unexpected close, Desegnos shows one notice offering **Report on GitHub**. The temporary report is stored in `crash-reports/pending.json` inside Desegnos's data folder and contains the app version, operating system and architecture, time, error text, location and a short backtrace. Open **What's included** to read the exact text before anything else happens.

Home folders, user names, email addresses, API keys, bearer and JWT tokens, query strings and long hex or base64 runs are removed; paths under your folders are reduced to file names. **Report on GitHub** opens a prefilled issue in your browser, where you can review it and submit; nothing is sent automatically. **Dismiss** deletes the local file. A normal close leaves no report. If the editor itself fails to render, it records only the error and component stack.

## Checks and local builds

```sh
pnpm build
pnpm test
pnpm check:rust
pnpm test:canvas
node scripts/cargo.mjs fmt --all -- --check
pnpm test:native
pnpm test:vault
pnpm desktop build --debug --no-bundle
pnpm desktop build --no-bundle
```

Rust/provider tests need task-owned loopback sockets. The native harness runs `xvfb-run -d` with isolated data and a private bus, resolved from `PATH` (or `XVFB_RUN`/`DBUS_RUN_SESSION`), and verifies the main desktop after cleanup. See [tests/native/README.md](../tests/native/README.md). The opt-in Linux vault runner creates a private Secret Service and verifies its process/bus/data before using synthetic credentials; ordinary tests never access a real vault.

## License

The editor is GPL-3.0-only; see [LICENSE](../LICENSE). Export templates and runtime helpers require the separate licensing described in the release plan when implemented. This build contains no export templates.

Generated drafts can use clearly labelled placeholders for missing content, and download known direct PNG/JPEG image URLs into the project. For a blocked attempt, use **Finish draft → Generate draft**, or **Change request** to edit it on the canvas. See [draft and image behavior](DRAFT_IMAGES.md) for bounds and export details.

OpenAI image availability is checked per API credential or ChatGPT account on connection, account switching, Test access, and after seven days; Check again retries without generating an image.
