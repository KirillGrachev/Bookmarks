/**
 * Контракт IPC: типы, которые фронтенд и Rust-ядро обязаны понимать одинаково.
 *
 * Зеркалит `bookmarks-core::domain` и `bookmarks-core::db` (serde в camelCase).
 * Любое расхождение здесь ловится не компилятором, а тестами и руками, поэтому
 * файл меняется только вместе с ядром — и это фиксируется в сообщении коммита.
 */

/** Метка времени: миллисекунды с эпохи Unix в UTC. */
export type Timestamp = number;

/** Идентификатор сущности (UUID v4). */
export type EntityId = string;

// ---------------------------------------------------------------------------
// Заметки
// ---------------------------------------------------------------------------

export interface Note {
  id: EntityId;
  title: string;
  body: string;
  folderId?: EntityId | null;
  tags: string[];
  pinned: boolean;
  archived: boolean;
  color?: string | null;
  createdAt: Timestamp;
  updatedAt: Timestamp;
  deletedAt?: Timestamp | null;
  contentHash: string;
  wordCount: number;
  encrypted: boolean;
}

/** Заметка для виртуализированного списка — без тела, с готовым превью. */
export interface NoteSummary {
  id: EntityId;
  title: string;
  excerpt: string;
  folderId?: EntityId | null;
  tags: string[];
  pinned: boolean;
  archived: boolean;
  color?: string | null;
  encrypted: boolean;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

export interface NoteDraft {
  title?: string | null;
  body?: string | null;
  folderId?: EntityId | null;
  tags?: string[];
  pinned?: boolean | null;
  color?: string | null;
  encrypted?: boolean | null;
}

/** `null` внутри `Some` означает «сбросить значение» (папка, цвет). */
export interface NotePatch {
  title?: string | null;
  body?: string | null;
  folderId?: EntityId | null;
  tags?: string[] | null;
  pinned?: boolean | null;
  archived?: boolean | null;
  color?: string | null;
}

// ---------------------------------------------------------------------------
// Папки и теги
// ---------------------------------------------------------------------------

export interface Folder {
  id: EntityId;
  name: string;
  parentId?: EntityId | null;
  color?: string | null;
  icon?: string | null;
  sortOrder: number;
  createdAt: Timestamp;
  updatedAt: Timestamp;
  noteCount: number;
}

export interface FolderNode {
  folder: Folder;
  path: string;
  depth: number;
  children: FolderNode[];
}

export interface FolderDraft {
  name: string;
  parentId?: EntityId | null;
  color?: string | null;
  icon?: string | null;
}

export interface FolderPatch {
  name?: string | null;
  parentId?: EntityId | null;
  color?: string | null;
  icon?: string | null;
  sortOrder?: number | null;
}

export interface DeleteFolderOutcome {
  movedNotes: number;
  movedFolders: number;
}

export interface Tag {
  id: EntityId;
  name: string;
  color?: string | null;
  createdAt: Timestamp;
  noteCount: number;
}

export interface TagDraft {
  name: string;
  color?: string | null;
}

// ---------------------------------------------------------------------------
// Списки и поиск
// ---------------------------------------------------------------------------

export type Scope = 'active' | 'archived' | 'trashed' | 'active_and_archived' | 'everything';

export type SortOrder =
  'updated_desc' | 'updated_asc' | 'created_desc' | 'created_asc' | 'title_asc' | 'title_desc';

export interface NoteFilter {
  folderId?: EntityId | null;
  recursive?: boolean;
  tags?: EntityId[];
  pinned?: boolean | null;
  color?: string | null;
  encrypted?: boolean | null;
  scope?: Scope | null;
  updatedSince?: Timestamp | null;
  updatedUntil?: Timestamp | null;
}

export interface NoteQuery {
  filter: NoteFilter;
  query: string;
  sort: SortOrder;
  limit: number;
  offset: number;
}

export interface NotePage {
  items: NoteSummary[];
  total: number;
  offset: number;
  hasMore: boolean;
}

/** Фрагмент совпадения: ядро помечает hit символами \u0001 / \u0002. */
export interface SearchHit {
  note: NoteSummary;
  snippet: string;
  rank: number;
}

export interface DuplicateGroup {
  contentHash: string;
  count: number;
  noteIds: EntityId[];
  sampleTitle: string;
}

// ---------------------------------------------------------------------------
// Версии
// ---------------------------------------------------------------------------

export type VersionOrigin = 'edit' | 'import' | 'sync' | 'restore';

export interface VersionSummary {
  id: number;
  title: string;
  wordCount: number;
  createdAt: Timestamp;
  origin: VersionOrigin;
  contentHash: string;
}

export interface NoteVersion extends VersionSummary {
  noteId: EntityId;
  body: string;
}

// ---------------------------------------------------------------------------
// Статистика
// ---------------------------------------------------------------------------

export interface LibraryStats {
  notes: number;
  words: number;
  oldest?: Timestamp | null;
  newest?: Timestamp | null;
  folders: number;
  tags: number;
  trashed: number;
  archived: number;
  encrypted: number;
}

export interface StorageStats {
  notes: number;
  notesInTrash: number;
  archivedNotes: number;
  encryptedNotes: number;
  folders: number;
  tags: number;
  versions: number;
  attachments: number;
  databaseBytes: number;
}

// ---------------------------------------------------------------------------
// Сейф
// ---------------------------------------------------------------------------

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
}

// ---------------------------------------------------------------------------
// Экспорт и импорт
// ---------------------------------------------------------------------------

export type ExportFormat = 'markdown' | 'json' | 'html';

export interface ExportOptions {
  includeArchived: boolean;
  includeTrashed: boolean;
  includeEncrypted: boolean;
  noteIds: EntityId[];
  frontMatter: boolean;
}

export interface ExportedFile {
  path: string;
  content: string;
}

export interface ExportResult {
  format: ExportFormat;
  files: ExportedFile[];
  notes: number;
  skipped: number;
  skippedReasons: string[];
  bytes: number;
}

export type ImportConflictPolicy = 'skip' | 'duplicate' | 'overwrite';

export interface ImportOptions {
  conflictPolicy: ImportConflictPolicy;
  folderId?: EntityId | null;
  createFolders: boolean;
  extractInlineTags: boolean;
}

export interface ImportReport {
  created: number;
  updated: number;
  skipped: number;
  failed: number;
  errors: string[];
  foldersCreated: number;
  tagsCreated: number;
}

// ---------------------------------------------------------------------------
// Синхронизация
// ---------------------------------------------------------------------------

export type ConflictPolicy = 'keep_newest' | 'keep_local' | 'keep_remote' | 'duplicate_both';

export type SyncStepKind = 'upload' | 'download' | 'delete_local' | 'delete_remote' | 'conflict';

export interface SyncStepDetail {
  step: SyncStepKind;
  noteId: EntityId;
  title: string;
  device?: string | null;
  resolution?: string | null;
}

export interface SyncPlan {
  steps: SyncStepDetail[];
  uploads: number;
  downloads: number;
  localDeletions: number;
  remoteDeletions: number;
  conflicts: number;
}

export interface SyncOutcome {
  plan: SyncPlan;
  uploaded: number;
  downloaded: number;
  deletedLocally: number;
  deletedRemotely: number;
  conflictsResolved: number;
  versionsSaved: number;
  startedAt: Timestamp;
  finishedAt: Timestamp;
  errors: string[];
}

export interface KnownDevice {
  deviceId: EntityId;
  deviceName: string;
  generatedAt: Timestamp;
  notes: number;
}

// ---------------------------------------------------------------------------
// Настройки
// ---------------------------------------------------------------------------

export type Theme = 'light' | 'dark' | 'system';
export type AccentColor = 'blue' | 'cyan' | 'green' | 'amber' | 'red' | 'pink' | 'violet' | 'slate';
export type Language = 'ru' | 'en';
export type EditorView = 'edit' | 'preview' | 'split';
export type RemoteKind = 'none' | 'directory' | 'web_dav';
export type VersionPolicy = 'off' | 'keep_recent' | 'keep_days';

export interface InterfaceSettings {
  theme: Theme;
  accent: AccentColor;
  language: Language;
  animations: boolean;
  compactList: boolean;
  showSidebar: boolean;
  showExcerpts: boolean;
  sortOrder: SortOrder;
  sidebarWidth: number;
}

export interface EditorSettings {
  fontSize: number;
  monospace: boolean;
  comfortableWidth: boolean;
  wordWrap: boolean;
  lineNumbers: boolean;
  spellcheck: boolean;
  autosave: boolean;
  autosaveDelayMs: number;
  defaultView: EditorView;
}

export interface VersionSettings {
  policy: VersionPolicy;
  keepCount: number;
  keepDays: number;
}

export interface SyncSettings {
  remote: RemoteKind;
  location: string;
  username: string;
  autoSync: boolean;
  intervalSecs: number;
  syncOnStart: boolean;
  encryptRemote: boolean;
  deviceId?: EntityId | null;
}

export interface VaultSettings {
  enabled: boolean;
  autoLock: boolean;
  autoLockMinutes: number;
  lockOnMinimize: boolean;
}

export interface BackupSettings {
  enabled: boolean;
  intervalHours: number;
  keepCount: number;
  directory: string;
}

export interface Settings {
  version: number;
  interface: InterfaceSettings;
  editor: EditorSettings;
  versions: VersionSettings;
  sync: SyncSettings;
  vault: VaultSettings;
  backup: BackupSettings;
}

// ---------------------------------------------------------------------------
// Ошибки и служебные типы
// ---------------------------------------------------------------------------

/** Структура ошибки, которую возвращает каждая IPC-команда. */
export interface ErrorPayload {
  kind:
    | 'constraint'
    | 'not_found'
    | 'invalid_input'
    | 'storage'
    | 'migration'
    | 'serialization'
    | 'io'
    | 'vault'
    | 'sync';
  message: string;
  field?: string;
  code?: string;
}

export interface AppInfo {
  version: string;
  tauriVersion: string;
  platform: string;
  arch: string;
  dataDir: string;
  databasePath: string;
}

/** События, которые Rust отправляет во фронтенд через `emit`. */
export type BackendEvent =
  | { type: 'notes-changed' }
  | { type: 'folders-changed' }
  | { type: 'tags-changed' }
  | { type: 'settings-changed'; settings: Settings }
  | { type: 'sync-progress'; state: 'started' | 'finished' | 'error'; detail?: string }
  | { type: 'vault-locked' };

/** Палитра цветовых меток — совпадает с `domain::limits::ALLOWED_COLORS`. */
export const NOTE_COLORS = [
  'slate',
  'blue',
  'cyan',
  'green',
  'amber',
  'orange',
  'red',
  'pink',
  'violet',
] as const;

export type NoteColor = (typeof NOTE_COLORS)[number];

/** Акценты интерфейса — совпадают с `domain::settings::AccentColor`. */
export const ACCENT_COLORS = ['blue', 'cyan', 'green', 'amber', 'red', 'pink', 'violet', 'slate'] as const;
