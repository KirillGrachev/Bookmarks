import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion } from 'motion/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowRight, FilePlus2, FileText, Folder, Search } from 'lucide-react';

import { getBackend } from '@/api/backend';
import { queryKeys } from '@/api/keys';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useI18n } from '@/hooks/useI18n';
import { modalVariants, overlayVariants } from '@/lib/motionPresets';
import { useAppStore } from '@/store/useAppStore';

type Item =
  | { kind: 'action'; id: string; label: string; hint: string; icon: typeof FilePlus2; run: () => void }
  | { kind: 'note'; id: string; label: string; hint: string; icon: typeof FileText; run: () => void }
  | { kind: 'folder'; id: string; label: string; hint: string; icon: typeof Folder; run: () => void };

/**
 * Палитра команд (Ctrl+K).
 *
 * Смешивает действия, заметки и папки в одной выдаче: в заметочнике переход к
 * заметке и команда «создать» — одинаково частые операции, и разделять их по
 * разным местам интерфейса значит заставлять пользователя вспоминать, где что.
 */
export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const selectNote = useAppStore((state) => state.selectNote);
  const setView = useAppStore((state) => state.setView);
  const setSettingsOpen = useAppStore((state) => state.setSettingsOpen);

  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(0);
  const debounced = useDebouncedValue(query, 140);
  const inputRef = useRef<HTMLInputElement>(null);

  const createNote = useMutation({
    mutationFn: () => getBackend().createNote({}),
    onSuccess: (note) => {
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      void queryClient.invalidateQueries({ queryKey: ['stats'] });
      selectNote(note.id);
      onClose();
    },
  });

  const notes = useQuery({
    queryKey: queryKeys.search(debounced, {}, 12),
    queryFn: () => getBackend().searchNotes(debounced, {}, 12),
    enabled: open && debounced.trim().length > 0,
  });

  const folders = useQuery({
    queryKey: queryKeys.folders(),
    queryFn: () => getBackend().folderTree(),
    enabled: open,
  });

  useEffect(() => {
    if (open) {
      setQuery('');
      setCursor(0);
      // Фокус ставим после появления окна, иначе браузер проигнорирует вызов.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const items = useMemo<Item[]>(() => {
    const list: Item[] = [
      {
        kind: 'action',
        id: 'new-note',
        label: t('palette.newNote'),
        hint: 'Ctrl+N',
        icon: FilePlus2,
        run: () => createNote.mutate(),
      },
      {
        kind: 'action',
        id: 'settings',
        label: t('nav.settings'),
        hint: 'Ctrl+,',
        icon: ArrowRight,
        run: () => {
          setSettingsOpen(true);
          onClose();
        },
      },
    ];

    const needle = debounced.trim().toLowerCase();
    for (const node of folders.data ?? []) {
      const stack = [node];
      while (stack.length > 0) {
        const current = stack.pop();
        if (!current) {
          continue;
        }
        stack.push(...current.children);
        if (needle && !current.folder.name.toLowerCase().includes(needle)) {
          continue;
        }
        list.push({
          kind: 'folder',
          id: current.folder.id,
          label: current.folder.name,
          hint: current.path,
          icon: Folder,
          run: () => {
            setView({ kind: 'folder', folderId: current.folder.id, recursive: true });
            onClose();
          },
        });
      }
    }

    for (const hit of notes.data ?? []) {
      list.push({
        kind: 'note',
        id: hit.note.id,
        label: hit.note.title || t('nav.untitled'),
        hint: hit.note.excerpt.slice(0, 80),
        icon: FileText,
        run: () => {
          selectNote(hit.note.id);
          onClose();
        },
      });
    }

    return list.slice(0, 24);
  }, [createNote, debounced, folders.data, notes.data, onClose, selectNote, setSettingsOpen, setView, t]);

  useEffect(() => {
    setCursor((value) => Math.min(value, Math.max(0, items.length - 1)));
  }, [items.length]);

  if (!open) {
    return null;
  }

  return createPortal(
    <AnimatePresence>
      <div className="fixed inset-0 z-[9995] flex items-start justify-center pt-[14vh]">
        <motion.div
          className="absolute inset-0 bg-black/35 backdrop-blur-[1px]"
          variants={overlayVariants}
          initial="hidden"
          animate="visible"
          exit="exit"
          onClick={onClose}
          aria-hidden
        />
        <motion.div
          role="dialog"
          aria-modal="true"
          aria-label={t('palette.title')}
          variants={modalVariants}
          initial="hidden"
          animate="visible"
          exit="exit"
          className="relative w-full max-w-xl overflow-hidden rounded-xl border border-border bg-surface-raised shadow-2xl"
        >
          <div className="flex items-center gap-2 border-b border-border px-4">
            <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden />
            <input
              ref={inputRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'ArrowDown') {
                  event.preventDefault();
                  setCursor((value) => (value + 1) % Math.max(1, items.length));
                } else if (event.key === 'ArrowUp') {
                  event.preventDefault();
                  setCursor((value) => (value - 1 + items.length) % Math.max(1, items.length));
                } else if (event.key === 'Enter') {
                  event.preventDefault();
                  items[cursor]?.run();
                } else if (event.key === 'Escape') {
                  event.preventDefault();
                  onClose();
                }
              }}
              placeholder={t('palette.placeholder')}
              aria-label={t('palette.placeholder')}
              className="h-12 w-full bg-transparent text-[0.88rem] text-foreground outline-none placeholder:text-muted-foreground/60"
            />
          </div>

          <ul role="listbox" className="max-h-[52vh] overflow-y-auto py-1.5">
            {items.length === 0 && (
              <li className="px-4 py-8 text-center text-[0.82rem] text-muted-foreground">
                {t('palette.noResults')}
              </li>
            )}
            {items.map((item, index) => (
              <li key={`${item.kind}:${item.id}`} role="option" aria-selected={index === cursor}>
                <button
                  type="button"
                  onMouseEnter={() => setCursor(index)}
                  onClick={item.run}
                  className={
                    'flex w-full cursor-pointer items-center gap-3 px-4 py-2 text-left transition-colors ' +
                    (index === cursor
                      ? 'bg-primary/10 text-primary'
                      : 'text-foreground hover:bg-surface-hover')
                  }
                >
                  <item.icon className="size-4 shrink-0 opacity-70" aria-hidden />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[0.85rem]">{item.label}</span>
                    {item.hint && (
                      <span className="block truncate text-[0.72rem] text-muted-foreground">{item.hint}</span>
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </motion.div>
      </div>
    </AnimatePresence>,
    document.body,
  );
}
