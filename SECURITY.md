# Security

Report vulnerabilities through [GitHub private vulnerability reporting](https://github.com/chelij/desegnos/security/advisories/new). Do not open public issues for vulnerabilities.

In scope: the Desegnos desktop app, AI credential handling, website and brand import, the local static server, export previews and website exports. Include the affected version, platform, impact and steps to reproduce; omit real credentials and private site content.

Security fixes target the latest 0.1.x preview. Older preview versions are unsupported; update before checking whether a problem persists.

## Trust boundaries

- **Credentials.** API keys and ChatGPT tokens live in native secure storage or session memory. The renderer sees status, never a secret. Keys are cleared from password fields on submit and never written to project files, crash reports or logs.
- **Imports.** Website fetches use public https addresses only, reject private and special network addresses, carry no credentials, follow same-site redirects under the same checks, and enforce byte and request caps. Folder and ZIP readers reject symlinks, traversal and oversized entries. Page code never runs during import: pages are rendered in a script-disabled frame and only the Desegnos extraction wrapper runs.
- **Local static server.** Exports, previews and imported snapshots are served on loopback at a random port under an unguessable token prefix. Only GET and HEAD are answered; there is no directory listing; files are read on demand and capped. Nothing else on the machine can browse the folder without the prefix.
- **Export.** The destination is chosen natively. Renderer-supplied paths never reach the filesystem. Writes are staged and verified, and an export records the project version and pointer fingerprints it came from.
- **AI requests.** Only an explicit Generate or Export decision sends a paid request. Cancelling submitted work may still incur provider charges or use plan credits; the editor says so. Usage display never implies a guaranteed billing cap.
- **Diagnostics.** Crash and error reporting is opt-in and off by default. Reports exclude prompts, site content, keys and file paths.

## Supported versions

Only the latest preview receives security fixes. Linux is the tested platform; Windows and macOS builds are unsigned, untested previews.
