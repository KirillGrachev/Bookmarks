import { useEffect, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { FileText, StickyNote } from 'lucide-react';
import { toast } from 'sonner';

import { getBackend } from '@/api/backend';
import type { EntityId } from '@/api/contracts';
import { Sidebar } from '@/components/Sidebar';
import { Modal } from '@/components/Modal';
import { TextField } from '@/components/ui/TextField';
import { Button } from '@/components/ui/Button';
import { NoteList } from '@/features/notes/components/NoteList';
import { NoteEditor } from '@/features/notes/components/NoteEditor';
import { VersionsModal } from '@/features/notes/components/VersionsModal';
import { CommandPalette } from '@/components/CommandPalette';
import { useI18n } from '@/hooks/useI18n';
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts';
import { useAppStore } from '@/store/useAppStore';

/**
 * Основной экран: панель папок → список заметок → редактор.
 *
 * Три колонки, а не роутинг по страницам: в заметочнике переход «заметка →
 * назад» встречается постоянно, и на роутере он стоил бы потери позиции списка
 * и состояния прокрутки.
 */
export function NotesPage() {
  const { t, lang } = useI18n();
  const queryClient = useQueryClient();
  const selectedId = useAppStore((state) => state.selectedNoteId);
  const selectNote = useAppStore((state) => state.selectNote);
  const loadNote = useAppStore((state) => state.loadNote);
  const openNote = useAppStore((state) => state.openNote);
  const setPaletteOpen = useAppStore((state) => state.setPaletteOpen);
  const paletteOpen = useAppStore((state) => state.paletteOpen);
  const versionsFor = useAppStore((state) => state.versionsFor);
  const setVersionsFor = useAppStore((state) => state.setVersionsFor);
  const refreshFolders = useAppStore((state) => state.refreshFolders);
  const refreshTags = useAppStore((state) => state.refreshTags);
  const refreshVault = useAppStore((state) => state.refreshVault);

  const [folderModal, setFolderModal] = useState(false);
  const [folderName, setFolderName] = useState('');
  const [folderError, setFolderError] = useState<string | null>(null);

  // Открытая заметка подтягивается отдельно от списка: в списке тела нет
  // (экономия IPC), а редактору нужен полный текст.
  useEffect(() => {
    void loadNote(selectedId);
  }, [selectedId, loadNote]);

  useEffect(() => {
    void refreshFolders();
    void refreshTags();
    void refreshVault();
  }, [refreshFolders, refreshTags, refreshVault]);

  const createNote = useMutation({
    mutationFn: () => getBackend().createNote({}),
    onSuccess: (note) => {
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      void queryClient.invalidateQueries({ queryKey: ['stats'] });
      selectNote(note.id);
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const createFolder = useMutation({
    mutationFn: (name: string) => getBackend().createFolder({ name }),
    onSuccess: async () => {
      await refreshFolders();
      setFolderModal(false);
      setFolderName('');
      setFolderError(null);
      toast.success(t('folder.created'));
    },
    onError: (error: Error) => setFolderError(error.message),
  });

  useKeyboardShortcuts([
    {
      id: 'new-note',
      shortcut: { code: 'KeyN', ctrl: true, allowInInput: true },
      action: () => createNote.mutate(),
    },
    {
      id: 'palette',
      shortcut: { code: 'KeyK', ctrl: true, allowInInput: true },
      action: () => setPaletteOpen(!paletteOpen),
    },
    {
      id: 'settings',
      shortcut: { code: 'Comma', ctrl: true },
      action: () => useAppStore.getState().setSettingsOpen(true),
    },
    { id: 'escape', shortcut: { code: 'Escape', allowInInput: true }, action: () => setPaletteOpen(false) },
  ]);

  return (
    <div className="flex min-h-0 flex-1">
      <Sidebar
        t={t}
        lang={lang}
        onNewFolder={() => setFolderModal(true)}
        onOpenSettings={() => useAppStore.getState().setSettingsOpen(true)}
      />

      <div className="w-[340px] shrink-0">
        <NoteList t={t} lang={lang} />
      </div>

      <main className="min-w-0 flex-1">
        {openNote && selectedId ? (
          <NoteEditor
            key={openNote.id}
            note={openNote}
            t={t}
            lang={lang}
            onOpenVersions={(id: EntityId) => setVersionsFor(id)}
          />
        ) : (
          <div className="grid h-full place-items-center bg-background text-center">
            <div className="max-w-sm px-8">
              <StickyNote className="mx-auto mb-3 size-9 text-muted-foreground/30" aria-hidden />
              <p className="text-[0.9rem] font-medium text-foreground/80">{t('list.empty')}</p>
              <p className="mt-1.5 text-[0.8rem] leading-relaxed text-muted-foreground">
                {t('list.emptyHint')}
              </p>
              <Button
                className="mt-4"
                size="sm"
                onClick={() => createNote.mutate()}
                disabled={createNote.isPending}
              >
                <FileText className="size-4" aria-hidden />
                {t('palette.newNote')}
              </Button>
            </div>
          </div>
        )}
      </main>

      <Modal
        open={folderModal}
        onClose={() => setFolderModal(false)}
        title={t('folder.new')}
        closeLabel={t('common.close')}
        width="max-w-sm"
        footer={
          <>
            <Button variant="ghost" size="sm" onClick={() => setFolderModal(false)}>
              {t('common.cancel')}
            </Button>
            <Button
              size="sm"
              disabled={!folderName.trim() || createFolder.isPending}
              onClick={() => createFolder.mutate(folderName)}
            >
              {t('common.save')}
            </Button>
          </>
        }
      >
        <TextField
          autoFocus
          label={t('folder.name')}
          value={folderName}
          error={folderError}
          onChange={(event) => {
            setFolderName(event.target.value);
            setFolderError(null);
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && folderName.trim()) {
              createFolder.mutate(folderName);
            }
          }}
        />
      </Modal>

      <VersionsModal noteId={versionsFor} onClose={() => setVersionsFor(null)} />
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
    </div>
  );
}
