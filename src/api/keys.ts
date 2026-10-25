import type { EntityId, NoteQuery, Scope, SortOrder } from '@/api/contracts';

/**
 * Ключи кэша react-query.
 *
 * Собранные в одном месте, чтобы инвалидация после мутаций не зависела от
 * того, кто и в каком порядке создавал ключ: `invalidateNotes()` сбрасывает и
 * список, и поиск, и статистику — иначе счётчики в сайдбаре расходились бы со
 * списком после создания заметки.
 */

/** Раздел списка → область видимости в запросе к ядру. */
export function scopeFor(kind: string): Scope {
  switch (kind) {
    case 'archive':
      return 'archived';
    case 'trash':
      return 'trashed';
    default:
      return 'active';
  }
}

export const queryKeys = {
  notesPage: (query: NoteQuery) => ['notes', 'page', query] as const,
  search: (text: string, filter: NoteQuery['filter'], limit: number) =>
    ['notes', 'search', text, filter, limit] as const,
  note: (id: EntityId) => ['notes', 'item', id] as const,
  noteBody: (id: EntityId) => ['notes', 'body', id] as const,
  folders: () => ['folders'] as const,
  folderList: () => ['folders', 'flat'] as const,
  tags: () => ['tags'] as const,
  versions: (id: EntityId) => ['versions', id] as const,
  stats: () => ['stats', 'library'] as const,
  storageStats: () => ['stats', 'storage'] as const,
  vault: () => ['vault', 'status'] as const,
  syncStatus: () => ['sync', 'status'] as const,
  syncDevices: () => ['sync', 'devices'] as const,
  duplicates: () => ['maintenance', 'duplicates'] as const,
};

/** Ключи, которые нужно сбросить после любого изменения заметок. */
export const noteInvalidations = [['notes'], ['stats'], ['tags'], ['folders']] as const;

/** Ключи, которые нужно сбросить после изменения папок. */
export const folderInvalidations = [['folders'], ['notes'], ['stats']] as const;

/** Ключи, которые нужно сбросить после изменения тегов. */
export const tagInvalidations = [['tags'], ['notes']] as const;

/** Стабильный порядок сортировки по умолчанию. */
export const defaultSort: SortOrder = 'updated_desc';

/** Размер страницы списка: достаточно для плавного скролла, мало для IPC. */
export const PAGE_SIZE = 60;
