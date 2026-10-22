# Desegnos

Create a website by describing what belongs on the page. Place requests on the canvas, generate a design with your AI connection, then edit and export it. Desegnos is a Rust desktop app with an open canvas and no sidebars.

## Create your first website

1. Choose **Blank website**, **Use my existing brand**, or **Open project**. Recent projects appear on the start screen.
2. Name your project and choose a Web or Mobile screen guide. New pages start light; the appearance control changes that later.
3. Choose **Add request** and click where you want something. Describe it in the box. Add as many requests as you need, on any page.
4. Open **AI settings**. Connect an OpenAI API key or sign in with your ChatGPT plan, then **Test access**. Choose a model and save your settings.
5. Choose **Flexible** for more interpretation or **Precise** to follow your request and placement closely. Click **Generate**. Progress appears above the canvas, and **Cancel** stops waiting.
6. Review the result on the canvas. **Keep**, **Undo**, **Try again**, and **Details** are beside the result. Click an element to select it; double-click text to edit it. Drag a selected element to move it or a corner to resize it. Use **Ask AI** for a generated change.
7. **Save** keeps an editable `.desegnos` project. **Export website** writes a website you can open in a browser and share.

Your project brief grows as you work. Open **Project brief** to review the purpose, audience, brand choices, and design feedback, or make a deliberate exception to a choice.

## Use an existing brand

Choose **Use my existing brand**, then select a Desegnos project, website folder, or ZIP. Review the files before **Analyze brand** sends the selected text and supported logos to your connected AI. Desegnos reads the source without changing it.

Review each finding and its sources, and choose which logos to keep. **Create project** starts a blank page with only your approved brand choices and portable logos. It does not copy the old website onto the canvas.

## Canvas controls

The floating canvas toolbar offers **Select**, **Add request**, **Pan**, and zoom. Double-click empty space to add a request, or Shift-drag to mark an area. Ctrl/Cmd+wheel zooms around the pointer. The grid starts on; **Grid and rulers** controls spacing and snapping.

Single-click selects an element and shows its outline and controls. Right-click opens its actions. Local text, link, image, style, move, and resize edits use no AI request. **Undo** and **Redo** work from the toolbar, menu, or Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z. **Menu → Keyboard shortcuts** lists the shortcuts.

## Your AI connection and files

API keys and sign-in tokens stay in secure device storage or the current session, separate from your project. API requests use your OpenAI billing; ChatGPT requests use the selected account's eligible plan or credits. The API budget is a conservative estimate, not a guaranteed billing cap. Cancelling submitted work may still use credits or incur charges.

Projects save their website source, brief, and approved choices. Existing `.desegni` projects still open and keep their path when you save them. Save keeps a previous archive for recovery. Desegnos preserves your work when a request fails, and interrupted requests do not resend automatically.

## Run and build

Run `npm run release:local` to typecheck and rebuild the frontend, then build the release executable with embedded UI and the default dialog features, skipping installer bundles. The Linux shell entry point `./scripts/build-release.sh` calls the same Node script and can find Node through mise when needed. The script prints the binary's path, size and timestamp. On Linux it also runs a five-second startup check on guarded Xvfb with private temporary app data, saving its screenshot and logs under `test-results/startup/`. Windows and macOS skip that Linux-only check and need manual startup verification.

The default executable is `target/release/desegnos` (`target/release/desegnos.exe` on Windows); the script also respects Cargo's target directory and `CARGO_BUILD_TARGET`. Development commands:

```sh
pnpm install --frozen-lockfile
pnpm build
pnpm desktop build --debug --no-bundle
pnpm desktop build --no-bundle
```

The [generation benchmark](docs/GENERATION_BENCHMARK.md) provides 18 fixed owner-run scenarios. Its offline checks make no AI requests; live quality remains unverified until you run and rate the results.

For protocol, storage, security, recovery, dependencies, and verification details, see the [technical guide](docs/USER_TECHNICAL.md). The [release plan](RELEASE_PLAN.md) tracks remaining work. The editor uses GPL-3.0-only; see [LICENSE](LICENSE).
