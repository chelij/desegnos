# Changelog

## 0.1.0 (unreleased)

First public preview of the visual canvas loop: draw boundaries and frames → describe them → Generate → edit elements → save → export.

- One infinite canvas per project, with Page and Screen boundaries, linked phone/tablet/desktop sizes and a dot grid.
- Element tree per boundary: boxes with free, stack and grid layout; text, shapes, lines, images, buttons, links, form controls, video, audio, embeds and code blocks.
- Frames, reference items, pointers with comments, automatic link lines, Play actions and clash choices.
- Tool strip, Properties/Layers dock, project brief, Style guide, History and alternatives.
- Several projects open at once as project tabs, each with its own canvas.
- Website import from an address, folder or ZIP: pages render in a script-disabled frame and become boundaries and elements, with project pictures, script notes, behaviour pointers and one undo step.
- One keymap for every canvas action, shared by tooltips, menu and keyboard help.
- Generate with an API key, ChatGPT plan or OpenAI-compatible local server; frozen requests, cancellation and usage reporting. The AI answers with elements, not code.
- AI-built website and app exports; direct PDF slide decks and PNG/SVG images. Export metadata saves without adding undo steps.
- `.desegnos` SQLite projects with content-addressed versions, crash recovery, save conflicts and previous-file backups.
- Opt-in, scrubbed crash reports; manual update checks; private security reporting; install guide and release-notes template.

Linux is the tested platform. Windows/macOS builds are untested previews; signing, notarization and remaining 1.0 features are deferred.
