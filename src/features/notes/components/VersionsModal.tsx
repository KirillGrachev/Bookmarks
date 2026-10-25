import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { History, RotateCcw, Trash2 } from 'lucide-react';
import { toast } from 'sonner';

import { getBackend } from '@/api/backend';
import { queryKeys } from '@/api/keys';
import type { EntityId } from '@/api/contracts';
import { Modal } from '@/components/Modal';
import { IconButton } from '@/components/ui/IconButton';
import { useI18n } from '@/hooks/useI18n';
import { formatDate, formatTime } from '@/lib/time';
import { cn, pluralRu } from '@/lib/utils';
import { useAppStore } from '@/store/useAppStore';

/**
 * История версий заметки.
 *
 * Версии создаёт ядро автоматически перед каждым изменением содержимого,
 * поэтому здесь только просмотр, откат и удаление. Откат сам пишет версию
 * «перед откатом», так что ошибочное восстановление обратимо.
 */
export function VersionsModal({ noteId, onClose }: { noteId: EntityId | null; onClose: () => void }) {
  const { t, lang } = useI18n();
  const queryClient = useQueryClient();
  const loadNote = useAppStore((state) => state.loadNote);

  const versions = useQuery({
    queryKey: noteId ? queryKeys.versions(noteId) : ['versions', 'none'],
    queryFn: () => getBackend().listVersions(noteId as EntityId, 100),
    enabled: noteId !== null,
  });

  const restore = useMutation({
    mutationFn: (versionId: number) => getBackend().restoreVersion(versionId),
    onSuccess: async (note) => {
      await loadNote(note.id);
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      void queryClient.invalidateQueries({ queryKey: ['versions'] });
      toast.success(t('versions.restored'));
      onClose();
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const remove = useMutation({
    mutationFn: (versionId: number) => getBackend().deleteVersion(versionId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['versions'] }),
    onError: (error: Error) => toast.error(error.message),
  });

  const prune = useMutation({
    mutationFn: () => getBackend().pruneVersions(),
    onSuccess: (count) => {
      void queryClient.invalidateQueries({ queryKey: ['versions'] });
      toast.success(t('versions.pruned', { count }));
    },
    onError: (error: Error) => toast.error(error.message),
  });

  let lastDate = '';

  return (
    <Modal
      open={noteId !== null}
      onClose={onClose}
      title={t('versions.title')}
      closeLabel={t('common.close')}
      width="max-w-2xl"
      footer={
        <button
          type="button"
          onClick={() => prune.mutate()}
          disabled={prune.isPending}
          className="cursor-pointer text-[0.8rem] text-muted-foreground underline-offset-2 transition-colors hover:text-foreground hover:underline"
        >
          {t('versions.prune')}
        </button>
      }
    >
      {versions.isPending && (
        <p className="py-8 text-center text-[0.83rem] text-muted-foreground">{t('common.loading')}</p>
      )}

      {versions.data?.length === 0 && (
        <div className="py-10 text-center">
          <History className="mx-auto mb-2 size-7 text-muted-foreground/30" aria-hidden />
          <p className="text-[0.85rem] text-foreground/80">{t('versions.empty')}</p>
          <p className="mt-1 text-[0.78rem] text-muted-foreground">{t('versions.emptyHint')}</p>
        </div>
      )}

      <ul className="space-y-1">
        {(versions.data ?? []).map((version) => {
          const date = formatDate(version.createdAt, lang);
          const showDate = date !== lastDate;
          lastDate = date;
          return (
            <li key={version.id}>
              {showDate && (
                <p className="mt-3 mb-1 text-[0.7rem] font-semibold tracking-wide text-muted-foreground/70 uppercase">
                  {date}
                </p>
              )}
              <div className="group flex items-center gap-3 rounded-lg border border-transparent px-3 py-2 transition-colors hover:border-border hover:bg-surface-hover">
                <span className="w-12 shrink-0 text-[0.75rem] text-muted-foreground tabular-nums">
                  {formatTime(version.createdAt, lang)}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[0.83rem] text-foreground">
                    {version.title || t('nav.untitled')}
                  </p>
                  <p className="text-[0.72rem] text-muted-foreground">
                    {lang === 'ru'
                      ? pluralRu(version.wordCount, 'слово', 'слова', 'слов')
                      : `${version.wordCount} words`}
                    {' · '}
                    {t(`versions.origin.${version.origin}`)}
                  </p>
                </div>
                <div
                  className={cn(
                    'flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity',
                    'group-hover:opacity-100 group-focus-within:opacity-100',
                  )}
                >
                  <IconButton
                    icon={RotateCcw}
                    label={t('versions.restore')}
                    size="sm"
                    onClick={() => restore.mutate(version.id)}
                  />
                  <IconButton
                    icon={Trash2}
                    label={t('versions.delete')}
                    size="sm"
                    danger
                    onClick={() => remove.mutate(version.id)}
                  />
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </Modal>
  );
}
