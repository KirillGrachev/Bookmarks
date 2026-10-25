/**
 * Демонстрационная реализация ядра для браузерного режима.
 *
 * Нужна для трёх задач:
 * 1. разработка интерфейса без Rust-тулчейна (`npm run dev` → тот же UI);
 * 2. unit-тесты компонентов на предсказуемом хранилище;
 * 3. демонстрация проекта в GitHub Pages / скриншотах.
 *
 * Данные живут в localStorage и повторяют правила ядра там, где это дёшево:
 * нормализация заголовков, мягкое удаление, история версий, поиск по словам.
 * Криптография не имитируется — в демо-режиме сейф показывает заглушку.
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
  StorageStats,
  SyncOutcome,
  SyncPlan,
  Tag,
  TagDraft,
  VersionSummary,
} from './contracts';
import type { Backend } from './backend';
import { defaultSettings } from '@/lib/settingsDefaults';

const STORAGE_KEY = 'bookmarks.demo.v1';

interface DemoVersion {
  id: number;
  noteId: EntityId;
  title: string;
  body: string;
  createdAt: number;
  origin: VersionSummary['origin'];
}

interface DemoState {
  notes: Note[];
  folders: Folder[];
  tags: Tag[];
  links: Array<{ noteId: EntityId; tagId: EntityId }>;
  versions: DemoVersion[];
  settings: Settings;
  nextVersionId: number;
}

function newId(): EntityId {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `demo-${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
}

/** Хеш содержимого: не криптографический, но стабильный — как в ядре. */
function contentHash(title: string, body: string): string {
  const input = `bookmarks/note/v1\0${title}\0${body}`;
  let hash = 2166136261;
  for (let index = 0; index < input.length; index += 1) {
    hash ^= input.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, '0').repeat(4);
}

function countWords(body: string): number {
  return body.split(/[^\p{L}\p{N}_]+/u).filter(Boolean).length;
}

function normalizeLabel(value: string): string {
  return value.replace(/\s+/g, ' ').trim();
}

/** Текст без Markdown-разметки — для превью и поиска. */
function plaintext(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, (block) => block.replace(/`/g, ''))
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_~`>#]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

function emptyState(): DemoState {
  return {
    notes: [],
    folders: [],
    tags: [],
    links: [],
    versions: [],
    settings: defaultSettings(),
    nextVersionId: 1,
  };
}

function seedState(state: DemoState): DemoState {
  if (state.notes.length > 0 || state.folders.length > 0) {
    return state;
  }
  const now = Date.now();
  const workFolder: Folder = {
    id: newId(),
    name: 'Работа',
    parentId: null,
    color: 'blue',
    icon: 'briefcase',
    sortOrder: 1,
    createdAt: now,
    updatedAt: now,
    noteCount: 0,
  };
  const ideasFolder: Folder = {
    id: newId(),
    name: 'Идеи',
    parentId: null,
    color: 'violet',
    icon: 'lightbulb',
    sortOrder: 2,
    createdAt: now,
    updatedAt: now,
    noteCount: 0,
  };
  const samples: Array<[string, string, Folder | null, string[]]> = [
    [
      'Добро пожаловать в Bookmarks',
      [
        '# Добро пожаловать',
        '',
        'Это **локальный** блокнот: заметки лежат на вашем устройстве, поиск работает мгновенно,',
        'а синхронизация и шифрование — опциональны.',
        '',
        '- `Ctrl+N` — новая заметка',
        '- `Ctrl+K` — палитра команд',
        '- `Ctrl+F` — поиск',
        '- `Ctrl+,` — настройки',
        '',
        '> Демо-режим в браузере хранит данные в localStorage.',
        '> В приложении это SQLite с полнотекстовым индексом.',
      ].join('\n'),
      null,
      ['стартовая'],
    ],
    [
      'Совещание по релизу',
      '- [ ] собрать список багов\n- [ ] договориться о дате\n- [x] назначить ответственных',
      workFolder,
      ['встречи'],
    ],
    ['Идея: палитра команд', 'Быстрый переход по заметкам через Ctrl+K.', ideasFolder, ['идеи']],
  ];

  const tags = new Map<string, Tag>();
  for (const [title, body, folder, tagNames] of samples) {
    const noteId = newId();
    const tagIds: EntityId[] = [];
    for (const name of tagNames) {
      let tag = tags.get(name);
      if (!tag) {
        tag = { id: newId(), name, color: null, createdAt: now, noteCount: 0 };
        tags.set(name, tag);
        state.tags.push(tag);
      }
      tagIds.push(tag.id);
      state.links.push({ noteId, tagId: tag.id });
    }
    state.notes.push({
      id: noteId,
      title,
      body,
      folderId: folder?.id ?? null,
      tags: tagNames,
      pinned: title.startsWith('Добро'),
      archived: false,
      color: null,
      createdAt: now,
      updatedAt: now,
      deletedAt: null,
      contentHash: contentHash(title, body),
      wordCount: countWords(body),
      encrypted: false,
    });
  }
  state.folders.push(workFolder, ideasFolder);
  return state;
}

/** Демонстрационное хранилище. */
export class DemoBackend implements Backend {
  private state: DemoState;

  private static instance: DemoBackend | null = null;

  /** Общий экземпляр — интерфейс и тесты видят одни данные. */
  static shared(): DemoBackend {
    if (!DemoBackend.instance) {
      DemoBackend.instance = new DemoBackend();
    }
    return DemoBackend.instance;
  }

  constructor(state?: DemoState) {
    this.state = state ?? DemoBackend.load();
  }

  private static load(): DemoState {
    if (typeof localStorage === 'undefined') {
      return seedState(emptyState());
    }
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) {
        const seeded = seedState(emptyState());
        localStorage.setItem(STORAGE_KEY, JSON.stringify(seeded));
        return seeded;
      }
      const parsed = JSON.parse(raw) as DemoState;
      return { ...emptyState(), ...parsed };
    } catch {
      return seedState(emptyState());
    }
  }

  private persist(): void {
    if (typeof localStorage === 'undefined') {
      return;
    }
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.state));
    } catch {
      // Переполнение localStorage не должно ронять интерфейс: демо-режим
      // продолжает работать в памяти до перезагрузки.
    }
  }

  /** Полный сброс демо-данных (кнопка в настройках демо-режима). */
  reset(): void {
    this.state = seedState(emptyState());
    this.persist();
  }

  private tagNames(noteId: EntityId): string[] {
    return this.state.links
      .filter((link) => link.noteId === noteId)
      .map((link) => this.state.tags.find((tag) => tag.id === link.tagId)?.name)
      .filter((name): name is string => Boolean(name))
      .sort((left, right) => left.localeCompare(right));
  }

  private toSummary(note: Note): NotePage['items'][number] {
    return {
      id: note.id,
      title: note.title,
      excerpt: plaintext(note.body).slice(0, 180),
      folderId: note.folderId,
      tags: this.tagNames(note.id),
      pinned: note.pinned,
      archived: note.archived,
      color: note.color,
      encrypted: note.encrypted,
      createdAt: note.createdAt,
      updatedAt: note.updatedAt,
    };
  }

  private matchesScope(note: Note, scope: Scope | null | undefined): boolean {
    switch (scope ?? 'active') {
      case 'active':
        return !note.archived && (note.deletedAt ?? null) === null;
      case 'archived':
        return note.archived && (note.deletedAt ?? null) === null;
      case 'trashed':
        return (note.deletedAt ?? null) !== null;
      case 'active_and_archived':
        return (note.deletedAt ?? null) === null;
      case 'everything':
        return true;
    }
  }

  private folderIds(filter: NoteQuery['filter']): Set<EntityId> | null {
    if (!filter.folderId) {
      return null;
    }
    const ids = new Set<EntityId>([filter.folderId]);
    if (filter.recursive) {
      let frontier = [filter.folderId];
      while (frontier.length > 0) {
        const next: EntityId[] = [];
        for (const folder of this.state.folders) {
          if (folder.parentId && frontier.includes(folder.parentId) && !ids.has(folder.id)) {
            ids.add(folder.id);
            next.push(folder.id);
          }
        }
        frontier = next;
      }
    }
    return ids;
  }

  private filtered(query: NoteQuery): Note[] {
    const folders = this.folderIds(query.filter);
    const text = query.query.trim().toLowerCase();
    return this.state.notes.filter((note) => {
      if (!this.matchesScope(note, query.filter.scope)) {
        return false;
      }
      if (folders && !(note.folderId && folders.has(note.folderId))) {
        return false;
      }
      if (typeof query.filter.pinned === 'boolean' && note.pinned !== query.filter.pinned) {
        return false;
      }
      if (query.filter.color && note.color !== query.filter.color) {
        return false;
      }
      if (typeof query.filter.encrypted === 'boolean' && note.encrypted !== query.filter.encrypted) {
        return false;
      }
      if (query.filter.tags?.length) {
        const names = this.tagNames(note.id).map((name) => name.toLowerCase());
        const wanted = query.filter.tags
          .map((id) => this.state.tags.find((tag) => tag.id === id)?.name.toLowerCase())
          .filter((name): name is string => Boolean(name));
        if (!wanted.every((name) => names.includes(name))) {
          return false;
        }
      }
      if (text) {
        const haystack = `${note.title}\n${plaintext(note.body)}`.toLowerCase();
        const words = text.split(/\s+/).filter((word) => !word.startsWith('-'));
        if (!words.every((word) => haystack.includes(word))) {
          return false;
        }
        const excluded = text
          .split(/\s+/)
          .filter((word) => word.startsWith('-'))
          .map((word) => word.slice(1));
        if (excluded.some((word) => haystack.includes(word))) {
          return false;
        }
      }
      return true;
    });
  }

  private sort(notes: Note[], order: NoteQuery['sort']): Note[] {
    const sorted = [...notes];
    sorted.sort((left, right) => {
      if (left.pinned !== right.pinned) {
        return left.pinned ? -1 : 1;
      }
      switch (order) {
        case 'updated_asc':
          return left.updatedAt - right.updatedAt;
        case 'created_desc':
          return right.createdAt - left.createdAt;
        case 'created_asc':
          return left.createdAt - right.createdAt;
        case 'title_asc':
          return left.title.localeCompare(right.title);
        case 'title_desc':
          return right.title.localeCompare(left.title);
        case 'updated_desc':
        default:
          return right.updatedAt - left.updatedAt;
      }
    });
    return sorted;
  }

  private requireNote(id: EntityId): Note {
    const note = this.state.notes.find((candidate) => candidate.id === id);
    if (!note) {
      throw new Error(`note not found: ${id}`);
    }
    return note;
  }

  private saveVersion(note: Note, origin: DemoVersion['origin'] = 'edit'): void {
    const last = this.state.versions.find((version) => version.noteId === note.id);
    if (last && last.body === note.body && last.title === note.title) {
      return;
    }
    this.state.versions.unshift({
      id: this.state.nextVersionId++,
      noteId: note.id,
      title: note.title,
      body: note.body,
      createdAt: note.updatedAt,
      origin,
    });
  }

  // --- Backend -----------------------------------------------------------

  async listNotes(query: NoteQuery): Promise<NotePage> {
    const all = this.sort(this.filtered(query), query.sort);
    const items = all.slice(query.offset, query.offset + query.limit).map((note) => this.toSummary(note));
    return {
      items,
      total: all.length,
      offset: query.offset,
      hasMore: query.offset + items.length < all.length,
    };
  }

  async searchNotes(query: string, filter: NoteQuery['filter'], limit: number): Promise<SearchHit[]> {
    const page = await this.listNotes({ filter, query, sort: 'updated_desc', limit, offset: 0 });
    return page.items.map((note) => ({ note, snippet: note.excerpt, rank: 0 }));
  }

  async getNote(id: EntityId): Promise<Note> {
    const note = this.requireNote(id);
    return { ...note, tags: this.tagNames(id) };
  }

  async getNoteBody(id: EntityId): Promise<string> {
    return this.requireNote(id).body;
  }

  async createNote(draft: NoteDraft): Promise<Note> {
    const now = Date.now();
    const body = draft.body ?? '';
    const title = draft.title?.trim()
      ? normalizeLabel(draft.title)
      : plaintext(body).split('\n')[0].slice(0, 300);
    const note: Note = {
      id: newId(),
      title,
      body,
      folderId: draft.folderId ?? null,
      tags: [],
      pinned: draft.pinned ?? false,
      archived: false,
      color: draft.color ?? null,
      createdAt: now,
      updatedAt: now,
      deletedAt: null,
      contentHash: contentHash(title, body),
      wordCount: countWords(body),
      encrypted: draft.encrypted ?? false,
    };
    for (const tagName of draft.tags ?? []) {
      await this.addTagToNote(note.id, tagName);
    }
    this.state.notes.push(note);
    this.persist();
    return this.getNote(note.id);
  }

  async updateNote(id: EntityId, patch: NotePatch): Promise<Note> {
    const note = this.requireNote(id);
    this.saveVersion(note);
    if (patch.title !== undefined && patch.title !== null) {
      note.title = normalizeLabel(patch.title);
    }
    if (patch.body !== undefined && patch.body !== null) {
      note.body = patch.body;
    }
    if (patch.folderId !== undefined) {
      note.folderId = patch.folderId;
    }
    if (patch.pinned !== undefined && patch.pinned !== null) {
      note.pinned = patch.pinned;
    }
    if (patch.archived !== undefined && patch.archived !== null) {
      note.archived = patch.archived;
    }
    if (patch.color !== undefined) {
      note.color = patch.color;
    }
    if (patch.tags) {
      this.state.links = this.state.links.filter((link) => link.noteId !== id);
      for (const tagName of patch.tags) {
        await this.addTagToNote(id, tagName);
      }
    }
    note.updatedAt = Date.now();
    note.contentHash = contentHash(note.title, note.body);
    note.wordCount = countWords(note.body);
    this.persist();
    return this.getNote(id);
  }

  async setNotePinned(id: EntityId, pinned: boolean): Promise<Note> {
    return this.updateNote(id, { pinned });
  }

  async setNoteArchived(id: EntityId, archived: boolean): Promise<Note> {
    return this.updateNote(id, { archived });
  }

  async moveNotes(ids: EntityId[], folderId: EntityId | null): Promise<number> {
    for (const id of ids) {
      await this.updateNote(id, { folderId });
    }
    return ids.length;
  }

  async deleteNotes(ids: EntityId[]): Promise<number> {
    const now = Date.now();
    let count = 0;
    for (const id of ids) {
      const note = this.state.notes.find((candidate) => candidate.id === id);
      if (note && (note.deletedAt ?? null) === null) {
        note.deletedAt = now;
        note.updatedAt = now;
        count += 1;
      }
    }
    this.persist();
    return count;
  }

  async restoreNotes(ids: EntityId[]): Promise<number> {
    let count = 0;
    for (const id of ids) {
      const note = this.state.notes.find((candidate) => candidate.id === id);
      if (note && (note.deletedAt ?? null) !== null) {
        note.deletedAt = null;
        note.updatedAt = Date.now();
        count += 1;
      }
    }
    this.persist();
    return count;
  }

  async purgeNotes(ids: EntityId[]): Promise<number> {
    const before = this.state.notes.length;
    this.state.notes = this.state.notes.filter((note) => !ids.includes(note.id));
    this.state.links = this.state.links.filter((link) => !ids.includes(link.noteId));
    this.state.versions = this.state.versions.filter((version) => !ids.includes(version.noteId));
    this.persist();
    return before - this.state.notes.length;
  }

  async emptyTrash(): Promise<number> {
    const trashed = this.state.notes
      .filter((note) => (note.deletedAt ?? null) !== null)
      .map((note) => note.id);
    return this.purgeNotes(trashed);
  }

  async duplicateNote(id: EntityId): Promise<Note> {
    const source = await this.getNote(id);
    return this.createNote({
      title: `${source.title} (копия)`.slice(0, 300),
      body: source.body,
      folderId: source.folderId,
      tags: source.tags,
      color: source.color,
    });
  }

  async findDuplicates(): Promise<DuplicateGroup[]> {
    const groups = new Map<string, Note[]>();
    for (const note of this.state.notes) {
      if ((note.deletedAt ?? null) !== null) {
        continue;
      }
      const group = groups.get(note.contentHash) ?? [];
      group.push(note);
      groups.set(note.contentHash, group);
    }
    return [...groups.values()]
      .filter((group) => group.length > 1)
      .map((group) => ({
        contentHash: group[0].contentHash,
        count: group.length,
        noteIds: group.map((note) => note.id),
        sampleTitle: group[0].title,
      }));
  }

  async trashExpired(days: number): Promise<EntityId[]> {
    const cutoff = Date.now() - days * 86_400_000;
    return this.state.notes
      .filter((note) => note.deletedAt != null && note.deletedAt < cutoff)
      .map((note) => note.id);
  }

  async listFolders(): Promise<Folder[]> {
    return this.state.folders
      .map((folder) => ({
        ...folder,
        noteCount: this.state.notes.filter(
          (note) => note.folderId === folder.id && (note.deletedAt ?? null) === null && !note.archived,
        ).length,
      }))
      .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name));
  }

  async folderTree(): Promise<FolderNode[]> {
    const folders = await this.listFolders();
    const build = (parentId: EntityId | null, depth: number, prefix: string): FolderNode[] =>
      folders
        .filter((folder) => folder.parentId === parentId)
        .map((folder) => {
          const path = prefix ? `${prefix} / ${folder.name}` : folder.name;
          return { folder, path, depth, children: build(folder.id, depth + 1, path) };
        });
    return build(null, 0, '');
  }

  async createFolder(draft: FolderDraft): Promise<Folder> {
    const name = normalizeLabel(draft.name);
    if (!name) {
      throw new Error('название папки не может быть пустым');
    }
    const exists = this.state.folders.some(
      (folder) =>
        folder.parentId === (draft.parentId ?? null) && folder.name.toLowerCase() === name.toLowerCase(),
    );
    if (exists) {
      throw new Error('папка с таким названием уже есть в этом месте');
    }
    const now = Date.now();
    const folder: Folder = {
      id: newId(),
      name,
      parentId: draft.parentId ?? null,
      color: draft.color ?? null,
      icon: draft.icon ?? null,
      sortOrder: this.state.folders.length + 1,
      createdAt: now,
      updatedAt: now,
      noteCount: 0,
    };
    this.state.folders.push(folder);
    this.persist();
    return folder;
  }

  async updateFolder(id: EntityId, patch: FolderPatch): Promise<Folder> {
    const folder = this.state.folders.find((candidate) => candidate.id === id);
    if (!folder) {
      throw new Error(`folder not found: ${id}`);
    }
    if (patch.parentId !== undefined) {
      if (patch.parentId === id) {
        throw new Error('папка не может быть своей собственной родительской');
      }
      let cursor = patch.parentId;
      while (cursor) {
        if (cursor === id) {
          throw new Error('нельзя переместить папку внутрь её собственного поддерева');
        }
        cursor = this.state.folders.find((candidate) => candidate.id === cursor)?.parentId ?? null;
      }
      folder.parentId = patch.parentId;
    }
    if (patch.name) {
      folder.name = normalizeLabel(patch.name);
    }
    if (patch.color !== undefined) {
      folder.color = patch.color;
    }
    if (patch.icon !== undefined) {
      folder.icon = patch.icon;
    }
    if (patch.sortOrder !== undefined && patch.sortOrder !== null) {
      folder.sortOrder = patch.sortOrder;
    }
    folder.updatedAt = Date.now();
    this.persist();
    return folder;
  }

  async deleteFolder(id: EntityId): Promise<DeleteFolderOutcome> {
    const movedNotes = this.state.notes.filter((note) => note.folderId === id).length;
    this.state.notes.forEach((note) => {
      if (note.folderId === id) {
        note.folderId = null;
      }
    });
    const parent = this.state.folders.find((folder) => folder.id === id)?.parentId ?? null;
    const movedFolders = this.state.folders.filter((folder) => folder.parentId === id).length;
    this.state.folders.forEach((folder) => {
      if (folder.parentId === id) {
        folder.parentId = parent;
      }
    });
    this.state.folders = this.state.folders.filter((folder) => folder.id !== id);
    this.persist();
    return { movedNotes, movedFolders };
  }

  async reorderFolders(ids: EntityId[]): Promise<void> {
    ids.forEach((id, index) => {
      const folder = this.state.folders.find((candidate) => candidate.id === id);
      if (folder) {
        folder.sortOrder = index;
      }
    });
    this.persist();
  }

  async listTags(): Promise<Tag[]> {
    return this.state.tags
      .map((tag) => ({
        ...tag,
        noteCount: this.state.links.filter((link) => {
          if (link.tagId !== tag.id) {
            return false;
          }
          const note = this.state.notes.find((candidate) => candidate.id === link.noteId);
          return Boolean(note && (note.deletedAt ?? null) === null);
        }).length,
      }))
      .sort((left, right) => right.noteCount - left.noteCount || left.name.localeCompare(right.name));
  }

  async createTag(draft: TagDraft): Promise<void> {
    const name = normalizeLabel(draft.name.replace(/^#+/, ''));
    if (!name) {
      throw new Error('название тега не может быть пустым');
    }
    if (this.state.tags.some((tag) => tag.name.toLowerCase() === name.toLowerCase())) {
      throw new Error(`тег «${name}» уже существует`);
    }
    this.state.tags.push({
      id: newId(),
      name,
      color: draft.color ?? null,
      createdAt: Date.now(),
      noteCount: 0,
    });
    this.persist();
  }

  async renameTag(id: EntityId, name: string): Promise<void> {
    const tag = this.state.tags.find((candidate) => candidate.id === id);
    if (!tag) {
      throw new Error(`tag not found: ${id}`);
    }
    const normalized = normalizeLabel(name.replace(/^#+/, ''));
    const existing = this.state.tags.find(
      (candidate) => candidate.id !== id && candidate.name.toLowerCase() === normalized.toLowerCase(),
    );
    if (existing) {
      this.state.links.forEach((link) => {
        if (link.tagId === id) {
          link.tagId = existing.id;
        }
      });
      this.state.links = this.state.links.filter(
        (link, index, all) =>
          all.findIndex((other) => other.noteId === link.noteId && other.tagId === link.tagId) === index,
      );
      this.state.tags = this.state.tags.filter((candidate) => candidate.id !== id);
    } else {
      tag.name = normalized;
    }
    this.persist();
  }

  async setTagColor(id: EntityId, color: string | null): Promise<void> {
    const tag = this.state.tags.find((candidate) => candidate.id === id);
    if (tag) {
      tag.color = color;
      this.persist();
    }
  }

  async deleteTag(id: EntityId): Promise<number> {
    const detached = this.state.links.filter((link) => link.tagId === id).length;
    this.state.links = this.state.links.filter((link) => link.tagId !== id);
    this.state.tags = this.state.tags.filter((tag) => tag.id !== id);
    this.persist();
    return detached;
  }

  async addTagToNote(noteId: EntityId, tagName: string): Promise<void> {
    const name = normalizeLabel(tagName.replace(/^#+/, ''));
    if (!name) {
      return;
    }
    let tag = this.state.tags.find((candidate) => candidate.name.toLowerCase() === name.toLowerCase());
    if (!tag) {
      tag = { id: newId(), name, color: null, createdAt: Date.now(), noteCount: 0 };
      this.state.tags.push(tag);
    }
    if (!this.state.links.some((link) => link.noteId === noteId && link.tagId === tag.id)) {
      this.state.links.push({ noteId, tagId: tag.id });
    }
  }

  async removeTagFromNote(noteId: EntityId, tagId: EntityId): Promise<void> {
    this.state.links = this.state.links.filter((link) => !(link.noteId === noteId && link.tagId === tagId));
    this.persist();
  }

  async listVersions(noteId: EntityId, limit: number): Promise<VersionSummary[]> {
    return this.state.versions
      .filter((version) => version.noteId === noteId)
      .slice(0, limit)
      .map((version) => ({
        id: version.id,
        title: version.title,
        wordCount: countWords(version.body),
        createdAt: version.createdAt,
        origin: version.origin,
        contentHash: contentHash(version.title, version.body),
      }));
  }

  async getVersion(versionId: number): Promise<NoteVersion> {
    const version = this.state.versions.find((candidate) => candidate.id === versionId);
    if (!version) {
      throw new Error(`version not found: ${versionId}`);
    }
    return {
      id: version.id,
      noteId: version.noteId,
      title: version.title,
      body: version.body,
      wordCount: countWords(version.body),
      createdAt: version.createdAt,
      origin: version.origin,
      contentHash: contentHash(version.title, version.body),
    };
  }

  async restoreVersion(versionId: number): Promise<Note> {
    const version = await this.getVersion(versionId);
    return this.updateNote(version.noteId, { title: version.title, body: version.body });
  }

  async deleteVersion(versionId: number): Promise<number> {
    const before = this.state.versions.length;
    this.state.versions = this.state.versions.filter((version) => version.id !== versionId);
    return before - this.state.versions.length;
  }

  async pruneVersions(): Promise<number> {
    const keep = this.state.settings.versions.keepCount;
    const before = this.state.versions.length;
    const byNote = new Map<EntityId, DemoVersion[]>();
    for (const version of this.state.versions) {
      const list = byNote.get(version.noteId) ?? [];
      list.push(version);
      byNote.set(version.noteId, list);
    }
    const kept: DemoVersion[] = [];
    for (const list of byNote.values()) {
      kept.push(...list.slice(0, keep));
    }
    this.state.versions = kept;
    this.persist();
    return before - kept.length;
  }

  async libraryStats(): Promise<LibraryStats> {
    const active = this.state.notes.filter((note) => (note.deletedAt ?? null) === null && !note.archived);
    return {
      notes: active.length,
      words: this.state.notes
        .filter((note) => (note.deletedAt ?? null) === null)
        .reduce((sum, note) => sum + note.wordCount, 0),
      oldest: Math.min(...this.state.notes.map((note) => note.createdAt), Date.now()),
      newest: Math.max(...this.state.notes.map((note) => note.updatedAt), 0),
      folders: this.state.folders.length,
      tags: this.state.tags.length,
      trashed: this.state.notes.filter((note) => (note.deletedAt ?? null) !== null).length,
      archived: this.state.notes.filter((note) => note.archived && (note.deletedAt ?? null) === null).length,
      encrypted: this.state.notes.filter((note) => note.encrypted).length,
    };
  }

  async storageStats(): Promise<StorageStats> {
    const stats = await this.libraryStats();
    return {
      notes: stats.notes,
      notesInTrash: stats.trashed,
      archivedNotes: stats.archived,
      encryptedNotes: stats.encrypted,
      folders: stats.folders,
      tags: stats.tags,
      versions: this.state.versions.length,
      attachments: 0,
      databaseBytes: JSON.stringify(this.state).length,
    };
  }

  async verifyIntegrity(): Promise<string[]> {
    return [];
  }

  async rebuildSearchIndex(): Promise<number> {
    return this.state.notes.length;
  }

  async recomputeDerived(): Promise<number> {
    return 0;
  }

  async getSettings(): Promise<Settings> {
    return structuredClone(this.state.settings);
  }

  async saveSettings(settings: Settings): Promise<Settings> {
    this.state.settings = structuredClone(settings);
    this.persist();
    return this.getSettings();
  }

  async resetSettings(): Promise<Settings> {
    this.state.settings = defaultSettings();
    this.persist();
    return this.getSettings();
  }

  async vaultStatus(): Promise<{ initialized: boolean; unlocked: boolean }> {
    return { initialized: false, unlocked: false };
  }

  async vaultInitialize(): Promise<void> {
    throw new Error('сейф недоступен в демо-режиме: запустите приложение');
  }

  async vaultUnlock(): Promise<void> {
    throw new Error('сейф недоступен в демо-режиме: запустите приложение');
  }

  async vaultLock(): Promise<void> {
    /* демо-сейф не блокируется */
  }

  async vaultChangePassword(): Promise<number> {
    throw new Error('сейф недоступен в демо-режиме');
  }

  async vaultDisable(): Promise<number> {
    throw new Error('сейф недоступен в демо-режиме');
  }

  async encryptNote(): Promise<void> {
    throw new Error('сейф недоступен в демо-режиме');
  }

  async decryptNote(): Promise<void> {
    throw new Error('сейф недоступен в демо-режиме');
  }

  async exportLibrary(format: ExportFormat, options: ExportOptions): Promise<ExportResult> {
    const notes = this.state.notes.filter((note) =>
      this.matchesScope(note, options.includeTrashed ? 'everything' : 'active_and_archived'),
    );
    const files = notes.map((note) => ({
      path: `${note.title || 'без названия'}.md`,
      content: `# ${note.title}\n\n${note.body}\n`,
    }));
    return {
      format,
      files,
      notes: notes.length,
      skipped: 0,
      skippedReasons: [],
      bytes: files.reduce((sum, file) => sum + file.content.length, 0),
    };
  }

  async importBundle(raw: string, options: ImportOptions): Promise<ImportReport> {
    const parsed = JSON.parse(raw) as { notes?: Array<{ title: string; body: string }> };
    let created = 0;
    for (const note of parsed.notes ?? []) {
      await this.createNote({ title: note.title, body: note.body, folderId: options.folderId ?? null });
      created += 1;
    }
    return {
      created,
      updated: 0,
      skipped: 0,
      failed: 0,
      errors: [],
      foldersCreated: 0,
      tagsCreated: 0,
    };
  }

  async importMarkdownFiles(files: Array<[string, string]>, options: ImportOptions): Promise<ImportReport> {
    let created = 0;
    for (const [path, content] of files) {
      const title = path.split('/').pop()?.replace(/\.md$/, '') ?? 'Без названия';
      await this.createNote({ title, body: content, folderId: options.folderId ?? null });
      created += 1;
    }
    return { created, updated: 0, skipped: 0, failed: 0, errors: [], foldersCreated: 0, tagsCreated: 0 };
  }

  async syncStatus(): Promise<{ running: boolean; lastRunAt: number | null; lastError: string | null }> {
    return { running: false, lastRunAt: null, lastError: null };
  }

  async syncPlan(_policy: ConflictPolicy): Promise<SyncPlan> {
    return { steps: [], uploads: 0, downloads: 0, localDeletions: 0, remoteDeletions: 0, conflicts: 0 };
  }

  async syncRun(_policy: ConflictPolicy): Promise<SyncOutcome> {
    return {
      plan: await this.syncPlan(_policy),
      uploaded: 0,
      downloaded: 0,
      deletedLocally: 0,
      deletedRemotely: 0,
      conflictsResolved: 0,
      versionsSaved: 0,
      startedAt: Date.now(),
      finishedAt: Date.now(),
      errors: ['синхронизация недоступна в демо-режиме'],
    };
  }

  async syncProbe(): Promise<void> {
    throw new Error('синхронизация недоступна в демо-режиме');
  }

  async syncDevices(): Promise<KnownDevice[]> {
    return [];
  }

  async appInfo(): Promise<AppInfo> {
    return {
      version: '0.1.0-demo',
      tauriVersion: '—',
      platform: 'browser',
      arch: 'web',
      dataDir: 'localStorage',
      databasePath: STORAGE_KEY,
    };
  }

  async openPath(path: string): Promise<void> {
    if (typeof window !== 'undefined') {
      window.open(path, '_blank', 'noopener');
    }
  }

  async saveFiles(files: Array<{ path: string; content: string }>, root: string): Promise<string> {
    // В браузере файл отдаётся через скачивание; путь условный.
    for (const file of files) {
      const blob = new Blob([file.content], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `${root}/${file.path}`.replace(/\/+/g, '_');
      anchor.click();
      URL.revokeObjectURL(url);
    }
    return root;
  }
}
