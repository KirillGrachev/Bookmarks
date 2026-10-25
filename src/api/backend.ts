/**
 * Доступ к ядру: контракт команд и две реализации.
 *
 * - {@link TauriBackend} — настоящее приложение: каждая команда уходит в Rust
 *   через `invoke`, все операции с данными выполняются в пуле блокирующих
 *   задач, поэтому событийный цикл окна не ждёт мьютекса базы.
 * - {@link DemoBackend} — браузерный демо-режим (`npm run dev` без Tauri):
 *   те же контракты на localStorage, чтобы вёрстку и логику интерфейса можно
 *   было проверять и править без установки Rust-тулчейна.
 *
 * Компоненты всегда работают через `backend`, никогда не вызывают `invoke`
 * напрямую: иначе демо-режим и тесты превращаются в имитацию половины ядра.
 */

import type {
  AppInfo,
  ConflictPolicy,
  DeleteFolderOutcome,
  DuplicateGroup,
  EntityId,
  ExportFormat,
  ExportOptions,
  ExportResult,
  Folder,
  FolderDraft,
  FolderNode,
  FolderPatch,
  ImportOptions,
  ImportReport,
  KnownDevice,
  LibraryStats,
  Note,
  NoteDraft,
  NotePage,
  NotePatch,
  NoteQuery,
  NoteVersion,
  Scope,
  SearchHit,
  Settings,
  SortOrder,
  StorageStats,
  SyncOutcome,
  SyncPlan,
  Tag,
  TagDraft,
  VersionSummary,
} from './contracts';

/** Полный набор операций, которые нужны интерфейсу. */
export interface Backend {
  // --- заметки ---
  listNotes(query: NoteQuery): Promise<NotePage>;
  searchNotes(query: string, filter: NoteQuery['filter'], limit: number): Promise<SearchHit[]>;
  getNote(id: EntityId): Promise<Note>;
  getNoteBody(id: EntityId): Promise<string>;
  createNote(draft: NoteDraft): Promise<Note>;
  updateNote(id: EntityId, patch: NotePatch): Promise<Note>;
  setNotePinned(id: EntityId, pinned: boolean): Promise<Note>;
  setNoteArchived(id: EntityId, archived: boolean): Promise<Note>;
  moveNotes(ids: EntityId[], folderId: EntityId | null): Promise<number>;
  deleteNotes(ids: EntityId[]): Promise<number>;
  restoreNotes(ids: EntityId[]): Promise<number>;
  purgeNotes(ids: EntityId[]): Promise<number>;
  emptyTrash(): Promise<number>;
  duplicateNote(id: EntityId): Promise<Note>;
  findDuplicates(): Promise<DuplicateGroup[]>;
  trashExpired(days: number): Promise<EntityId[]>;

  // --- папки ---
  listFolders(): Promise<Folder[]>;
  folderTree(): Promise<FolderNode[]>;
  createFolder(draft: FolderDraft): Promise<Folder>;
  updateFolder(id: EntityId, patch: FolderPatch): Promise<Folder>;
  deleteFolder(id: EntityId): Promise<DeleteFolderOutcome>;
  reorderFolders(ids: EntityId[]): Promise<void>;

  // --- теги ---
  listTags(): Promise<Tag[]>;
  createTag(draft: TagDraft): Promise<void>;
  renameTag(id: EntityId, name: string): Promise<void>;
  setTagColor(id: EntityId, color: string | null): Promise<void>;
  deleteTag(id: EntityId): Promise<number>;
  addTagToNote(noteId: EntityId, tagName: string): Promise<void>;
  removeTagFromNote(noteId: EntityId, tagId: EntityId): Promise<void>;

  // --- версии ---
  listVersions(noteId: EntityId, limit: number): Promise<VersionSummary[]>;
  getVersion(versionId: number): Promise<NoteVersion>;
  restoreVersion(versionId: number): Promise<Note>;
  deleteVersion(versionId: number): Promise<number>;
  pruneVersions(): Promise<number>;

  // --- статистика и обслуживание ---
  libraryStats(): Promise<LibraryStats>;
  storageStats(): Promise<StorageStats>;
  verifyIntegrity(): Promise<string[]>;
  rebuildSearchIndex(): Promise<number>;
  recomputeDerived(): Promise<number>;

  // --- настройки ---
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  resetSettings(): Promise<Settings>;

  // --- сейф ---
  vaultStatus(): Promise<{ initialized: boolean; unlocked: boolean }>;
  vaultInitialize(password: string): Promise<void>;
  vaultUnlock(password: string): Promise<void>;
  vaultLock(): Promise<void>;
  vaultChangePassword(current: string, next: string): Promise<number>;
  vaultDisable(password: string): Promise<number>;
  encryptNote(id: EntityId): Promise<void>;
  decryptNote(id: EntityId): Promise<void>;

  // --- экспорт и импорт ---
  exportLibrary(format: ExportFormat, options: ExportOptions): Promise<ExportResult>;
  importBundle(raw: string, options: ImportOptions): Promise<ImportReport>;
  importMarkdownFiles(files: Array<[string, string]>, options: ImportOptions): Promise<ImportReport>;

  // --- синхронизация ---
  syncStatus(): Promise<{ running: boolean; lastRunAt: number | null; lastError: string | null }>;
  syncPlan(policy: ConflictPolicy): Promise<SyncPlan>;
  syncRun(policy: ConflictPolicy): Promise<SyncOutcome>;
  syncProbe(): Promise<void>;
  syncDevices(): Promise<KnownDevice[]>;

  // --- система ---
  appInfo(): Promise<AppInfo>;
  openPath(path: string): Promise<void>;
  saveFiles(files: Array<{ path: string; content: string }>, root: string): Promise<string>;
}

/** Приложение запущено внутри Tauri (а не в браузере). */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Вызывает команду Rust-ядра.
 *
 * Ошибка приходит структурой `ErrorPayload` (см. `bookmarks-core::error`);
 * она пробрасывается как есть, а локализацией занимается слой UI.
 */
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
  return tauriInvoke<T>(command, args);
}

/** Реализация контракта поверх IPC. */
export class TauriBackend implements Backend {
  listNotes = (query: NoteQuery) => invoke<NotePage>('list_notes', { query });
  searchNotes = (query: string, filter: NoteQuery['filter'], limit: number) =>
    invoke<SearchHit[]>('search_notes', { query, filter, limit });
  getNote = (id: EntityId) => invoke<Note>('get_note', { id });
  getNoteBody = (id: EntityId) => invoke<string>('get_note_body', { id });
  createNote = (draft: NoteDraft) => invoke<Note>('create_note', { draft });
  updateNote = (id: EntityId, patch: NotePatch) => invoke<Note>('update_note', { id, patch });
  setNotePinned = (id: EntityId, pinned: boolean) => invoke<Note>('set_note_pinned', { id, pinned });
  setNoteArchived = (id: EntityId, archived: boolean) => invoke<Note>('set_note_archived', { id, archived });
  moveNotes = (ids: EntityId[], folderId: EntityId | null) => invoke<number>('move_notes', { ids, folderId });
  deleteNotes = (ids: EntityId[]) => invoke<number>('delete_notes', { ids });
  restoreNotes = (ids: EntityId[]) => invoke<number>('restore_notes', { ids });
  purgeNotes = (ids: EntityId[]) => invoke<number>('purge_notes', { ids });
  emptyTrash = () => invoke<number>('empty_trash');
  duplicateNote = (id: EntityId) => invoke<Note>('duplicate_note', { id });
  findDuplicates = () => invoke<DuplicateGroup[]>('find_duplicates');
  trashExpired = (days: number) => invoke<EntityId[]>('trash_expired', { days });

  listFolders = () => invoke<Folder[]>('list_folders');
  folderTree = () => invoke<FolderNode[]>('folder_tree');
  createFolder = (draft: FolderDraft) => invoke<Folder>('create_folder', { draft });
  updateFolder = (id: EntityId, patch: FolderPatch) => invoke<Folder>('update_folder', { id, patch });
  deleteFolder = (id: EntityId) => invoke<DeleteFolderOutcome>('delete_folder', { id });
  reorderFolders = (ids: EntityId[]) => invoke<void>('reorder_folders', { ids });

  listTags = () => invoke<Tag[]>('list_tags');
  createTag = (draft: TagDraft) => invoke<void>('create_tag', { draft });
  renameTag = (id: EntityId, name: string) => invoke<void>('rename_tag', { id, name });
  setTagColor = (id: EntityId, color: string | null) => invoke<void>('set_tag_color', { id, color });
  deleteTag = (id: EntityId) => invoke<number>('delete_tag', { id });
  addTagToNote = (noteId: EntityId, tagName: string) => invoke<void>('add_tag_to_note', { noteId, tagName });
  removeTagFromNote = (noteId: EntityId, tagId: EntityId) =>
    invoke<void>('remove_tag_from_note', { noteId, tagId });

  listVersions = (noteId: EntityId, limit: number) =>
    invoke<VersionSummary[]>('list_versions', { noteId, limit });
  getVersion = (versionId: number) => invoke<NoteVersion>('get_version', { versionId });
  restoreVersion = (versionId: number) => invoke<Note>('restore_version', { versionId });
  deleteVersion = (versionId: number) => invoke<number>('delete_version', { versionId });
  pruneVersions = () => invoke<number>('prune_versions');

  libraryStats = () => invoke<LibraryStats>('library_stats');
  storageStats = () => invoke<StorageStats>('storage_stats');
  verifyIntegrity = () => invoke<string[]>('verify_integrity');
  rebuildSearchIndex = () => invoke<number>('rebuild_search_index');
  recomputeDerived = () => invoke<number>('recompute_derived');

  getSettings = () => invoke<Settings>('get_settings');
  saveSettings = (settings: Settings) => invoke<Settings>('save_settings', { settings });
  resetSettings = () => invoke<Settings>('reset_settings');

  vaultStatus = () => invoke<{ initialized: boolean; unlocked: boolean }>('vault_status');
  vaultInitialize = (password: string) => invoke<void>('vault_initialize', { password });
  vaultUnlock = (password: string) => invoke<void>('vault_unlock', { password });
  vaultLock = () => invoke<void>('vault_lock');
  vaultChangePassword = (current: string, next: string) =>
    invoke<number>('vault_change_password', { current, next });
  vaultDisable = (password: string) => invoke<number>('vault_disable', { password });
  encryptNote = (id: EntityId) => invoke<void>('encrypt_note', { id });
  decryptNote = (id: EntityId) => invoke<void>('decrypt_note', { id });

  exportLibrary = (format: ExportFormat, options: ExportOptions) =>
    invoke<ExportResult>('export_library', { format, options });
  importBundle = (raw: string, options: ImportOptions) =>
    invoke<ImportReport>('import_bundle', { raw, options });
  importMarkdownFiles = (files: Array<[string, string]>, options: ImportOptions) =>
    invoke<ImportReport>('import_markdown_files', { files, options });

  syncStatus = () =>
    invoke<{ running: boolean; lastRunAt: number | null; lastError: string | null }>('sync_status');
  syncPlan = (policy: ConflictPolicy) => invoke<SyncPlan>('sync_plan', { policy });
  syncRun = (policy: ConflictPolicy) => invoke<SyncOutcome>('sync_run', { policy });
  syncProbe = () => invoke<void>('sync_probe');
  syncDevices = () => invoke<KnownDevice[]>('sync_devices');

  appInfo = () => invoke<AppInfo>('app_info');
  openPath = (path: string) => invoke<void>('open_path', { path });
  saveFiles = (files: Array<{ path: string; content: string }>, root: string) =>
    invoke<string>('save_files', { files, root });
}

/**
 * Активная реализация ядра.
 *
 * Инициализируется в `main.tsx` до рендера; до этого момента обращение к ней —
 * ошибка программирования, а не состояние гонки, поэтому тип не опциональный.
 */
let activeBackend: Backend | null = null;

/** Подключает реализацию ядра (Tauri или демо). */
export function setBackend(backend: Backend): void {
  activeBackend = backend;
}

/** Возвращает активную реализацию ядра. */
export function getBackend(): Backend {
  if (!activeBackend) {
    throw new Error('backend is not initialized: call setBackend() before rendering');
  }
  return activeBackend;
}

/** Область видимости списка, которую показывает интерфейс. */
export type ViewScope = Exclude<Scope, 'everything'>;

/** Сортировка по умолчанию для нового пользователя. */
export const DEFAULT_SORT: SortOrder = 'updated_desc';
