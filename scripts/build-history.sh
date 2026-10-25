#!/usr/bin/env bash
# Rebuilds the Bookmarks commit history.
#
# One-shot script: it lays the already finished tree out into logical commits in
# the order the project actually grew (core -> storage -> search -> versions ->
# vault -> sync -> export -> UI -> CI). After that, history continues normally.
#
# Author identity must match the GitHub account that owns the repository,
# otherwise commits are not attributed to it (no avatar, no contribution graph).
set -euo pipefail

# Repository root — one level above this script. An earlier version used
# `cd "$(dirname "$0")"`, which made every `git add` look inside scripts/ and
# silently stage nothing, collapsing the whole history into a single commit.
cd "$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -f Cargo.toml ]; then
  echo "Run this from the project tree (Cargo.toml not found)" >&2
  exit 1
fi

export GIT_AUTHOR_NAME="KirillGrachev"
export GIT_AUTHOR_EMAIL="NeyComiunity@gmail.com"
export GIT_COMMITTER_NAME="KirillGrachev"
export GIT_COMMITTER_EMAIL="NeyComiunity@gmail.com"

commit() {
  local message_file="$1"
  # Commit only what the caller staged: `git add -A` here would collapse the
  # whole history into one commit.
  if git diff --cached --quiet; then
    echo "  (nothing staged, skipping)"
    return 0
  fi
  git commit -q -F "$message_file"
  echo "  -> $(git log -1 --pretty=%s)"
}

MSG=$(mktemp -d)
trap 'rm -rf "$MSG"' EXIT

cat > "$MSG/01" <<'EOF'
chore: workspace scaffold for Bookmarks

Cargo workspace with two crates: bookmarks-core (the whole domain, storage and
sync logic with no Tauri dependency) and src-tauri (the window shell). The split
is deliberate — the core is testable without a GUI, and a mobile Tauri target can
reuse the same crate.

- shared workspace.package and dependency versions; release profile tuned for
  size (opt-level="s", lto, codegen-units=1, strip) because cold start and
  installer size matter more than peak throughput in a desktop notes app
- eslint config bans implicit any and enforces consistent-type-imports: the IPC
  contract is typed end to end, a stray any breaks it silently
- prettier config matches the existing projects (single quotes, 110 columns)
- MIT license
EOF
git add Cargo.toml Cargo.lock crates/bookmarks-core/Cargo.toml src-tauri/Cargo.toml \
        src-tauri/src/main.rs src-tauri/src/lib.rs .gitignore .prettierrc.json \
        eslint.config.js LICENSE
commit "$MSG/01"

cat > "$MSG/02" <<'EOF'
feat: domain models, input limits and a single error type for the core

The foundation both storage and IPC stand on. Models double as the wire
contract, so normalization happens once here instead of being spread across
layers.

Domain (Rust):
- Note/NoteDraft/NotePatch: `None` in a patch means "leave as is", `Some(None)`
  means "clear" (folder, color), which lets autosave send only what changed
- Folder/Tag with label normalization: `#Работа` and `работа` are the same tag,
  comparison is case-insensitive
- limits live in domain::limits and are generous on purpose — a 10k-char title
  breaks list layout, a multi-megabyte body breaks the full-text index
- the label palette is a closed set of 9 colors: an arbitrary color picker
  value could end up unreadable on the dark theme
- Error::InvalidInput carries the offending field so the UI highlights a form
  input instead of showing a generic toast; ErrorPayload is its IPC shape
- Timestamp is UTC milliseconds: cheap for SQLite (INTEGER), cheap over IPC (a
  number, not a string to parse) and directly usable by Intl.DateTimeFormat
EOF
git add crates/bookmarks-core/src/lib.rs crates/bookmarks-core/src/error.rs \
        crates/bookmarks-core/src/ids.rs crates/bookmarks-core/src/time.rs \
        crates/bookmarks-core/src/domain
commit "$MSG/02"

cat > "$MSG/03" <<'EOF'
feat: SQLite storage — schema, migrations, connection pool, note CRUD

Notes, folders, tags, versions, attachments and the vault in one file, plus an
FTS5 index maintained by triggers so writing to `notes` keeps search up to date
without a second code path.

Backend (Rust):
- migrations are hand-rolled (~20 lines) instead of pulling a library: they
  control user_version explicitly, and a user database must be upgradeable from
  any previous schema. A database from a newer version is rejected with a clear
  message rather than half-opened
- FTS5 indexes mb_plain_text(body), a user-defined SQL function that strips
  Markdown: searching for "**important**" must find the word, not the asterisks
- encrypted notes are indexed with an empty body — text the user put behind a
  password is not searchable, while the title stays visible in lists
- a small connection pool behind a mutex lets concurrent IPC commands (list
  search plus autosave of the open note) avoid queueing on one connection
- the in-memory store is a shared-cache database with a unique name: with plain
  `:memory:` a second pooled connection would see an empty schema
- compound operations (note plus tags, version plus index) run in a transaction
- folder name uniqueness per parent is checked in Rust, not SQL: SQLite lower()
  and COLLATE NOCASE are ASCII-only, and folder names here are Cyrillic
- moving a folder walks ancestors from the new parent upwards; the reversed
  argument order silently allowed a cycle and is now covered by a test

Tests: 22 integration tests against real SQLite.
EOF
git add crates/bookmarks-core/src/db/mod.rs crates/bookmarks-core/src/db/schema.rs \
        crates/bookmarks-core/src/db/sql_functions.rs crates/bookmarks-core/src/db/read.rs \
        crates/bookmarks-core/src/db/write.rs
commit "$MSG/03"

cat > "$MSG/04" <<'EOF'
feat: search query language and ranked FTS5 results

One parser serves both the SQL query to FTS5 and instant filtering of an
already-loaded list on the frontend (mirrored in src/lib/searchQuery.ts, both
sides covered by tests so a behavior drift fails CI).

- supported syntax: `"exact phrase"`, `tag:work`, `#tag`, `folder:Projects`,
  `is:pinned`, `is:untagged`, `has:task|image|code|table`, `color:red`,
  `after:`/`before:`, `-excluded`
- operators never reach FTS5; they are applied to results, so they behave the
  same in any locale and do not distort bm25 ranking
- the MATCH expression is assembled from escaped tokens only (quotes are
  doubled), so user input cannot change query semantics
- negation is the binary `NOT` (`A NOT B`): the unary minus means a column
  filter in FTS5 and fails with "no such column", while `AND NOT` is a syntax
  error. A query made of exclusions only yields an empty expression, and then
  MATCH is omitted entirely instead of erroring out
- dates accept ISO, dd.mm.yyyy, yyyymmdd and relative forms (today/7d/week);
  `before:` takes the end of day, otherwise "before Jan 31" loses that whole day
- Markdown to text via pulldown-cmark: code blocks stay searchable, links keep
  their label only, HTML is dropped so tag names never match
EOF
git add crates/bookmarks-core/src/search
commit "$MSG/04"

cat > "$MSG/05" <<'EOF'
feat: note version history with reversible rollback

A version is a snapshot of title and body that the core takes on its own, with
no explicit user action: before a note update, on import, and when applying
remote changes during sync.

- duplicate snapshots are skipped by comparing content hashes, otherwise
  autosave would fill history with identical rows within minutes
- rollback itself writes a version with origin=restore, so an accidental
  "restore" does not lose the current text
- retention pruning runs as one SQL statement per note instead of a loop over
  versions: seconds instead of minutes on a library with tens of thousands of
  notes. Policies: off, keep_recent(N), keep_days(D)
- versions are removed by cascade together with the note

Tests: 8 unit tests in versions.rs.
EOF
git add crates/bookmarks-core/src/versions.rs crates/bookmarks-core/src/db/versions.rs
commit "$MSG/05"

cat > "$MSG/06" <<'EOF'
feat: encrypted vault — PBKDF2 + AES-256-GCM in a BKENC1 container

Optional per-note encryption with a password that never leaves the device.

Backend (Rust):
- container layout `BKENC1`: magic | version | salt | nonce | ciphertext+tag,
  stored as base64 in notes.body. A newer format version is rejected explicitly
  and a foreign file is never mistaken for ours
- PBKDF2-HMAC-SHA256 at 210k iterations (current OWASP guidance) derives a
  master key; blake3 domain separation turns it into two independent keys —
  encryption and verifier. Only the verifier hash is stored
- every container gets its own random salt and nonce, so identical notes
  produce different ciphertext and re-encryption on password change never
  reuses a nonce
- verifier comparison is constant time; the session key is zeroed on lock
- changing the password re-encrypts every note in one transaction; disabling the
  vault decrypts them back and drops the vault row
- creating an encrypted note without an initialized vault is an error, not a
  silent flag: otherwise the note would be unreadable after restoring a backup
- iteration count is overridable for tests (set_kdf_iterations_for_tests) — the
  logic does not depend on it, and 210k iterations across dozens of vault tests
  would take seconds each

Tests: 14 crypto unit tests + 10 vault integration tests.
EOF
git add crates/bookmarks-core/src/crypto
commit "$MSG/06"

cat > "$MSG/07" <<'EOF'
feat: device sync with three-way merge and deterministic conflict resolution

Model: each device publishes its own manifest (note fingerprints) and writes
note bodies only into its own subtree, devices/<device_id>/notes/.

Backend (Rust):
- per-device namespaces are essential. With a shared note file, the device that
  resolved a conflict first overwrote the other one's version, the conflict
  became invisible and the edit was lost silently
- three-way merge over base/local/remote, where base is this device's own
  manifest from the previous run
- the fingerprint is blake3(content hash + deleted flag): trashing a note does
  not change its text, so without the flag deletions never reached other devices
- on equal timestamps the winner is picked by device id, deterministically —
  otherwise two machines could keep pushing the same note back and forth
- the losing side always lands in version history (origin=sync), and the
  duplicate_both policy additionally creates a "… (conflict with <device>)" note
- the plan is built before anything is applied, so it can be shown to the user
- the manifest is published even when individual notes failed, otherwise
  already-synced notes would look modified again on the next run
- a foreign, corrupted or newer manifest is ignored instead of being partially
  trusted: decisions based on it could delete someone else's notes
- transport is behind the Remote trait: a directory (writes go through a temp
  file plus rename, so a killed process cannot leave a half-written note) and an
  in-memory implementation for tests. Paths are validated — `../` and absolute
  paths are rejected because they come from other devices' manifests
- folder structure travels as a path, not an id: internal keys differ per device

Tests: 14 integration tests, two devices over a shared remote — propagation,
conflicts, deletions, offline, foreign manifests, encrypted exchange.
EOF
git add crates/bookmarks-core/src/sync crates/bookmarks-core/src/db/sync.rs \
        crates/bookmarks-core/tests
commit "$MSG/07"

cat > "$MSG/08" <<'EOF'
feat: export and import — Markdown, JSON bundle, single-file HTML

Three formats for three jobs: moving to another notes app, keeping a backup, and
printing or sending a note to someone without the app.

- Markdown export writes a directory tree mirroring the folders plus YAML
  front-matter that Obsidian, Logseq, Joplin and Hugo already understand, and an
  index.md table of contents
- front-matter is parsed only at the very start of a file and only when a closing
  delimiter exists, so a `---` horizontal rule in the middle of a note does not
  swallow half of it; slices use byte offsets of line boundaries, otherwise
  Cyrillic text panicked on a non-char boundary
- the JSON bundle (MBBNDL1) carries folders, tags, notes and stats in one file
  and is versioned, so a file from a future version is rejected whole
- HTML export is a single self-contained document with the app palette inlined
  and no external resources — it opens offline and prints
- the minimal Markdown renderer escapes everything and replaces javascript: and
  vbscript: hrefs, so an export cannot become an executable file
- file names are sanitized for Windows: forbidden characters, reserved names
  (CON, NUL, COM1…), trailing dots and collisions like "Report!" vs "Report"
- import finds existing notes by id, and by content hash when there is no id, so
  re-importing the same backup does not create duplicates
- encrypted notes are skipped while the vault is locked and the skip is reported:
  an export must neither fail nor silently lose data

Tests: 30 export unit tests covering round-trips, foreign front-matter and XSS.
EOF
git add crates/bookmarks-core/src/export
commit "$MSG/08"

cat > "$MSG/09" <<'EOF'
feat: IPC contract and a browser demo backend

Components talk to a Backend interface and never call invoke directly, otherwise
the demo mode and the tests would turn into a reimplementation of half the core.

Frontend (TypeScript):
- src/api/contracts.ts mirrors bookmarks-core::domain and db (serde camelCase).
  A drift here is not caught by the compiler, so it is caught by tests and must
  be mentioned in the commit message
- TauriBackend sends every command to Rust, where it runs on the blocking pool,
  so the window event loop never waits on the database mutex
- DemoBackend implements the same contract on localStorage, which makes
  `npm run dev` usable without a Rust toolchain. Vault and sync honestly report
  that they are unavailable instead of pretending to encrypt
- ErrorPayload (kind/message/field) is passed through as-is; localization is the
  UI layer's job
EOF
git add src/api src/test
commit "$MSG/09"

cat > "$MSG/10" <<'EOF'
feat: design system — theme tokens, accents, motion presets

The palette and the interaction details follow the PhoneBook app so both look
like one family.

- light theme is slate/blue, dark is zinc/blue; tokens are declared through
  Tailwind v4 @theme, so utilities like bg-surface-hover work out of the box
- 8 accent colors are applied via a data-attribute on <html>: switching accents
  needs no rebuild and does not multiply utility classes
- theme switching uses a temporary .theme-transitioning class (500ms, colors
  only, no geometry animation)
- one motion family: motionPresets.ts (0.2s crossfade, 0.035s row stagger) and
  the CSS .submenu-pop at 0.18s for dropdowns. The "Animations" toggle disables
  both through .disable-animations and MotionConfig duration=0
- thin translucent scrollbars, sonner toasts restyled to match app cards
- Markdown preview typography and search hit highlighting (<mark>)
EOF
git add src/index.css src/main.tsx src/lib
commit "$MSG/10"

cat > "$MSG/11" <<'EOF'
feat: desktop UI — folder tree, note list, editor, command palette

Three columns instead of route-per-screen: going back from a note is constant in
a notes app, and with routing it would cost the list position and scroll state
every time.

Frontend (React 19 + TypeScript):
- Sidebar: sections (all/pinned/archive/trash), a folder tree with counts, tags.
  Selection changes background color only, never font weight — otherwise text
  jumps by half a pixel on every click
- the list is paged at 60 rows with scroll-to-load-more, so the DOM never holds
  more than a couple hundred nodes. A virtualizer would only pay off at tens of
  thousands of rows and would add jumping row heights
- note bodies are not sent to the list at all, only substr(body, 1, 600) for the
  excerpt, which cuts IPC payload by an order of magnitude on large libraries
- editor: Markdown toolbar, edit/split/preview modes, Ctrl+B/I/S, a status line
  with word count and last save time
- autosave uses the configured delay and always flushes on unmount, so the last
  edit is not lost when switching notes or closing the window
- an encrypted note with a locked vault shows a placeholder instead of the
  ciphertext container
- Ctrl+K palette mixes actions, notes and folders in one result set
- version history modal: list, rollback, delete, retention pruning
- frameless-window TitleBar driven by the stock Tauri JS API; the
  maximize/restore icon is kept in sync through an onResized subscription
- ErrorBoundary shows a message and a reload button instead of a white window;
  the data is in SQLite, not in React state, so nothing is lost
EOF
git add src/App.tsx src/components src/features/notes src/hooks src/store index.html \
        vite.config.ts vitest.config.ts tsconfig.json tsconfig.node.json package.json \
        package-lock.json
commit "$MSG/11"

cat > "$MSG/12" <<'EOF'
feat: settings — interface, editor, vault, sync, data and about

A full screen over the workspace rather than a modal: six sections with their own
scrolling inside a dialog turns into a window-in-a-window, which is cramped on
desktop.

- every change is persisted immediately, there is no Save button — one less step
- loaded settings are merged with defaults (withDefaults), so a file written by
  an older version opens without a migration
- numeric fields are clamped (clampSettings): hand-editing the JSON could set
  fontSize=0 and break the layout
- vault section: create, unlock, lock, status indicator; in demo mode it warns
  that encryption is unavailable instead of failing silently
- sync section: transport choice, location, interval, E2EE toggle, connection
  probe
- data section: storage statistics, integrity_check, search index rebuild
- RU/EN switch applies in place, the interface language follows the setting
EOF
git add src/features/settings
commit "$MSG/12"

cat > "$MSG/13" <<'EOF'
chore: GitHub Actions checks, README and roadmap

CI runs exactly the local check set, so "green locally" means "green in the
pipeline".

Tooling:
- rust job on windows-latest: rustfmt --check, clippy --all-targets
  -D warnings, cargo test --workspace. Windows is the target platform, and a
  Linux Tauri build needs webkit dependencies and does not reflect the real
  artifact
- frontend job on ubuntu-latest: tsc (app and node configs), eslint
  --max-warnings 0, prettier --check, vitest, production build, dist artifact
- concurrency cancels stale runs of the same ref
- README documents features, architecture, build commands and the security
  decisions; docs/ROADMAP.md marks a stage done only when it is green in CI
EOF
git add .github README.md docs public
commit "$MSG/13"

cat > "$MSG/14" <<'EOF'
feat: Tauri shell — window, IPC commands, logging, startup tasks

The shell wires the core to a window. It owns no logic: every command converts
types where the wire format differs and runs the work on the blocking pool.

Backend (Rust):
- every command touching SQLite, settings or the vault is async and executes via
  spawn_blocking, so the window event loop never waits on the store mutex held
  by a long write transaction (no frozen input, dragging or cursor)
- NotePatch over IPC has no folderId on purpose: Option<Option<T>> cannot be told
  apart from a missing key once it crosses the JS boundary, so moving notes is an
  explicit move_notes call where null unambiguously means the root
- CommandError serializes transparently into the core ErrorPayload, and also
  implements Display/std::error::Error so startup code and tracing can use it
- settings live in a JSON file in the config dir, not in the notes database: they
  must be readable before the store opens and must survive a damaged database.
  Writes go through a temp file plus rename; a broken file is quarantined as
  settings.json.broken and defaults are used instead of refusing to start
- a damaged database is never silently recreated (that would look like "all my
  notes disappeared") — startup reports the path and the reason and stops
- device id is persisted in the config dir and mirrored into settings: without a
  stable id every launch would look like a new device to the sync engine and
  manifests would accumulate
- logging goes to a file in the platform log dir with one rotation generation: a
  packaged Windows app has no console, so stdout would be lost exactly when it is
  needed
- single-instance plugin focuses the existing window instead of starting a second
  process on the same SQLite file
- startup tasks (trash retention, version pruning, optional sync) run in the
  background so the first frame never waits on the network
- E2EE sync refuses to run with a locked vault rather than silently uploading
  plaintext
- window is created hidden; the frontend shows it once settings are applied, so
  there is no flash of the default theme

Tests: 8 shell tests (settings round-trip and quarantine, device id stability,
log rotation).
EOF
git add src-tauri
commit "$MSG/14"

cat > "$MSG/15" <<'EOF'
chore: app icon and remaining service files

Branded bookmark icon (rounded square with the accent gradient) generated into
all bundle formats, plus the one-shot script that rebuilds this history.
EOF
git add -A
commit "$MSG/15"

echo
echo "Commits: $(git rev-list --count HEAD)"
git log --pretty='%h %an <%ae> %s' | head -20
