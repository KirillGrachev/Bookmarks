import { create } from 'zustand';

import type {
  AccentColor,
  ConflictPolicy,
  EntityId,
  FolderNode,
  Language,
  Note,
  Settings,
  SortOrder,
  Tag,
  Theme,
} from '@/api/contracts';
import { getBackend } from '@/api/backend';
import { clampSettings, defaultSettings, withDefaults } from '@/lib/settingsDefaults';

/**
 * Какой раздел списка показан сейчас.
 *
 * Отдельный тип вместо «id папки или null»: виртуальные разделы
 * («Все», «Закреплённые», «Архив», «Корзина») не являются папками,
 * и смешивать их с `folderId` — источник ошибок при фильтрации.
 */
export type ListView =
  | { kind: 'all' }
  | { kind: 'pinned' }
  | { kind: 'folder'; folderId: EntityId; recursive: boolean }
  | { kind: 'tag'; tagId: EntityId }
  | { kind: 'archive' }
  | { kind: 'trash' };

/** Состояние сейфа в интерфейсе. */
export interface VaultUiState {
  initialized: boolean;
  unlocked: boolean;
}

interface AppState {
  // --- настройки ---
  settings: Settings;
  settingsLoaded: boolean;
  setSettings: (settings: Settings) => void;
  patchInterface: (patch: Partial<Settings['interface']>) => void;
  patchEditor: (patch: Partial<Settings['editor']>) => void;
  loadSettings: () => Promise<void>;
  saveSettings: () => Promise<void>;
  resetSettings: () => Promise<void>;

  // --- язык и тема (быстрый доступ без прохода через settings) ---
  language: Language;
  setLanguage: (language: Language) => void;
  theme: Theme;
  setTheme: (theme: Theme) => void;
  accent: AccentColor;
  setAccent: (accent: AccentColor) => void;
  animationsEnabled: boolean;

  // --- навигация ---
  view: ListView;
  setView: (view: ListView) => void;
  selectedNoteId: EntityId | null;
  selectNote: (id: EntityId | null) => void;
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  sortOrder: SortOrder;
  setSortOrder: (sort: SortOrder) => void;

  // --- справочники ---
  folders: FolderNode[];
  tags: Tag[];
  refreshFolders: () => Promise<void>;
  refreshTags: () => Promise<void>;

  // --- открытая заметка ---
  openNote: Note | null;
  isDirty: boolean;
  isOpeningNote: boolean;
  loadNote: (id: EntityId | null) => Promise<void>;
  markDirty: () => void;
  markClean: () => void;

  // --- сейф ---
  vault: VaultUiState;
  refreshVault: () => Promise<void>;

  // --- синхронизация ---
  conflictPolicy: ConflictPolicy;
  setConflictPolicy: (policy: ConflictPolicy) => void;
  syncRunning: boolean;
  setSyncRunning: (running: boolean) => void;

  // --- окна интерфейса ---
  paletteOpen: boolean;
  setPaletteOpen: (open: boolean) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
  versionsFor: EntityId | null;
  setVersionsFor: (id: EntityId | null) => void;
}

/**
 * Глобальное состояние интерфейса.
 *
 * Здесь только то, что нужно нескольким несвязанным компонентам (тема, язык,
 * текущий раздел, открытая заметка). Данные заметок и результаты поиска живут
 * в react-query: у них есть ключи, кэш и инвалидация, а дублировать их в
 * zustand — значит ловить рассинхрон между списком и редактором.
 */
export const useAppStore = create<AppState>((set, get) => ({
  settings: defaultSettings(),
  settingsLoaded: false,
  setSettings: (settings) => set({ settings: clampSettings(settings) }),
  patchInterface: (patch) => {
    const next = { ...get().settings, interface: { ...get().settings.interface, ...patch } };
    set({ settings: clampSettings(next) });
  },
  patchEditor: (patch) => {
    const next = { ...get().settings, editor: { ...get().settings.editor, ...patch } };
    set({ settings: clampSettings(next) });
  },
  loadSettings: async () => {
    try {
      const loaded = await getBackend().getSettings();
      set({ settings: clampSettings(withDefaults(loaded)), settingsLoaded: true });
    } catch {
      // Настройки недоступны (например, повреждён файл) — работаем с
      // значениями по умолчанию и помечаем состояние загруженным, чтобы
      // splash не висел вечно.
      set({ settings: defaultSettings(), settingsLoaded: true });
    }
  },
  saveSettings: async () => {
    const current = get().settings;
    try {
      const saved = await getBackend().saveSettings(current);
      set({ settings: clampSettings(withDefaults(saved)) });
    } catch {
      // Ошибка показывается тостом в месте вызова; состояние не откатываем,
      // иначе интерфейс «прыгнет» обратно на глазах у пользователя.
    }
  },
  resetSettings: async () => {
    try {
      const saved = await getBackend().resetSettings();
      set({ settings: clampSettings(withDefaults(saved)) });
    } catch {
      set({ settings: defaultSettings() });
    }
  },

  language: 'ru',
  setLanguage: (language) =>
    set((state) => ({
      language,
      settings: { ...state.settings, interface: { ...state.settings.interface, language } },
    })),
  theme: 'system',
  setTheme: (theme) =>
    set((state) => ({
      theme,
      settings: { ...state.settings, interface: { ...state.settings.interface, theme } },
    })),
  accent: 'blue',
  setAccent: (accent) =>
    set((state) => ({
      accent,
      settings: { ...state.settings, interface: { ...state.settings.interface, accent } },
    })),
  animationsEnabled: true,

  view: { kind: 'all' },
  setView: (view) => set({ view, selectedNoteId: null }),
  selectedNoteId: null,
  selectNote: (id) => set({ selectedNoteId: id }),
  searchQuery: '',
  setSearchQuery: (query) => set({ searchQuery: query }),
  sortOrder: 'updated_desc',
  setSortOrder: (sortOrder) =>
    set((state) => ({
      sortOrder,
      settings: { ...state.settings, interface: { ...state.settings.interface, sortOrder } },
    })),

  folders: [],
  tags: [],
  refreshFolders: async () => {
    try {
      const folders = await getBackend().folderTree();
      set({ folders });
    } catch {
      set({ folders: [] });
    }
  },
  refreshTags: async () => {
    try {
      set({ tags: await getBackend().listTags() });
    } catch {
      set({ tags: [] });
    }
  },

  openNote: null,
  isDirty: false,
  isOpeningNote: false,
  loadNote: async (id) => {
    if (!id) {
      set({ openNote: null, isDirty: false });
      return;
    }
    set({ isOpeningNote: true });
    try {
      const note = await getBackend().getNote(id);
      // Тело зашифрованной заметки ядро отдаёт контейнером; при
      // заблокированном сейфе показываем заглушку, а не мусор.
      const body = note.encrypted ? await safeBody(id) : note.body;
      set({ openNote: { ...note, body }, isDirty: false, isOpeningNote: false });
    } catch {
      set({ openNote: null, isDirty: false, isOpeningNote: false });
    }
  },
  markDirty: () => set({ isDirty: true }),
  markClean: () => set({ isDirty: false }),

  vault: { initialized: false, unlocked: false },
  refreshVault: async () => {
    try {
      set({ vault: await getBackend().vaultStatus() });
    } catch {
      set({ vault: { initialized: false, unlocked: false } });
    }
  },

  conflictPolicy: 'keep_newest',
  setConflictPolicy: (conflictPolicy) => set({ conflictPolicy }),
  syncRunning: false,
  setSyncRunning: (syncRunning) => set({ syncRunning }),

  paletteOpen: false,
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  settingsOpen: false,
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  versionsFor: null,
  setVersionsFor: (versionsFor) => set({ versionsFor }),
}));

/** Читает тело, не роняя открытие заметки при заблокированном сейфе. */
async function safeBody(id: EntityId): Promise<string> {
  try {
    return await getBackend().getNoteBody(id);
  } catch {
    // Сейф заблокирован или заметка повреждена: редактор покажет заглушку.
    return '';
  }
}

/** Селектор: язык интерфейса из настроек (единый источник правды). */
export function selectLanguage(state: AppState): Language {
  return state.settings.interface.language;
}

/** Селектор: тема с учётом системной. */
export function selectTheme(state: AppState): Theme {
  return state.settings.interface.theme;
}
