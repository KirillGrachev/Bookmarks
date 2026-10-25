import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Bold,
  Check,
  Code,
  Eye,
  Heading1,
  Italic,
  Link as LinkIcon,
  List,
  ListOrdered,
  PenLine,
  SquareCode,
  Strikethrough,
} from 'lucide-react';
import { toast } from 'sonner';

import { getBackend } from '@/api/backend';
import type { EditorView, EntityId, Note } from '@/api/contracts';
import { IconButton } from '@/components/ui/IconButton';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { countWords, renderMarkdown, wrapSelection, prefixLines } from '@/lib/markdown';
import { cn, pluralRu } from '@/lib/utils';
import { formatFull } from '@/lib/time';
import type { Translator } from '@/lib/i18n';
import { useAppStore } from '@/store/useAppStore';

interface NoteEditorProps {
  note: Note;
  t: Translator;
  lang: 'ru' | 'en';
  onOpenVersions: (id: EntityId) => void;
}

/**
 * Редактор заметки: заголовок, Markdown и превью.
 *
 * Автосохранение — с задержкой из настроек и обязательной «доводкой» при
 * переключении заметки или закрытии окна: иначе последняя правка терялась бы
 * ровно в тот момент, когда пользователь ушёл.
 */
export function NoteEditor({ note, t, lang, onOpenVersions }: NoteEditorProps) {
  const queryClient = useQueryClient();
  const editorSettings = useAppStore((state) => state.settings.editor);
  const vaultUnlocked = useAppStore((state) => state.vault.unlocked);
  const markDirty = useAppStore((state) => state.markDirty);
  const markClean = useAppStore((state) => state.markClean);
  const isDirty = useAppStore((state) => state.isDirty);

  const [title, setTitle] = useState(note.title);
  const [body, setBody] = useState(note.body);
  const [view, setView] = useState<EditorView>(editorSettings.defaultView);
  const [savedAt, setSavedAt] = useState<number | null>(note.updatedAt);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  // Смена открытой заметки — единственный момент, когда поля перезаполняются
  // извне; без этого редактор показывал бы прошлый текст на новой заметке.
  useEffect(() => {
    setTitle(note.title);
    setBody(note.body);
    setSavedAt(note.updatedAt);
    markClean();
  }, [note.id, note.title, note.body, note.updatedAt, markClean]);

  const debouncedTitle = useDebouncedValue(title, editorSettings.autosaveDelayMs);
  const debouncedBody = useDebouncedValue(body, editorSettings.autosaveDelayMs);

  const save = useMutation({
    mutationFn: (patch: { title?: string; body?: string }) => getBackend().updateNote(note.id, patch),
    onSuccess: (updated) => {
      setSavedAt(updated.updatedAt);
      markClean();
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      void queryClient.invalidateQueries({ queryKey: ['stats'] });
    },
    onError: (error: Error) => toast.error(error.message),
  });

  // Автосохранение по изменению текста (с задержкой из настроек).
  useEffect(() => {
    if (!editorSettings.autosave || note.encrypted) {
      return;
    }
    if (debouncedTitle === note.title && debouncedBody === note.body) {
      return;
    }
    markDirty();
    save.mutate({ title: debouncedTitle, body: debouncedBody });
  }, [
    debouncedTitle,
    debouncedBody,
    editorSettings.autosave,
    note.body,
    note.id,
    note.title,
    note.encrypted,
    markDirty,
    save,
  ]);

  // Доводка при размонтировании (переключение заметки, закрытие окна).
  const latest = useRef({ title, body, noteTitle: note.title, noteBody: note.body, id: note.id });
  latest.current = { title, body, noteTitle: note.title, noteBody: note.body, id: note.id };
  useEffect(() => {
    return () => {
      const state = latest.current;
      if (state.title !== state.noteTitle || state.body !== state.noteBody) {
        void getBackend()
          .updateNote(state.id, { title: state.title, body: state.body })
          .catch(() => {
            // Ошибка сохранения при закрытии не должна ломать выход из окна;
            // заметка останется в прежнем состоянии и будет видна в списке.
          });
      }
    };
  }, []);

  const applyWrap = useCallback(
    (before: string, after: string) => {
      const area = bodyRef.current;
      if (!area) {
        return;
      }
      const result = wrapSelection(body, area.selectionStart, area.selectionEnd, before, after);
      setBody(result.value);
      markDirty();
      requestAnimationFrame(() => {
        area.focus();
        area.setSelectionRange(result.selectionStart, result.selectionEnd);
      });
    },
    [body, markDirty],
  );

  const applyPrefix = useCallback(
    (prefix: string) => {
      const area = bodyRef.current;
      if (!area) {
        return;
      }
      const result = prefixLines(body, area.selectionStart, area.selectionEnd, prefix);
      setBody(result.value);
      markDirty();
      requestAnimationFrame(() => {
        area.focus();
        area.setSelectionRange(result.selectionStart, result.selectionEnd);
      });
    },
    [body, markDirty],
  );

  const preview = useMemo(() => renderMarkdown(body), [body]);
  const words = useMemo(() => countWords(body), [body]);

  if (note.encrypted && !vaultUnlocked) {
    return (
      <div className="grid h-full place-items-center bg-background px-8 text-center">
        <div>
          <p className="text-[0.95rem] font-medium text-foreground">{t('editor.locked')}</p>
          <p className="mt-1.5 max-w-sm text-[0.82rem] leading-relaxed text-muted-foreground">
            {t('editor.lockedHint')}
          </p>
        </div>
      </div>
    );
  }

  const toolbar: Array<{ icon: typeof Bold; label: string; run: () => void }> = [
    { icon: Bold, label: 'Ctrl+B', run: () => applyWrap('**', '**') },
    { icon: Italic, label: 'Ctrl+I', run: () => applyWrap('*', '*') },
    { icon: Strikethrough, label: '~~', run: () => applyWrap('~~', '~~') },
    { icon: SquareCode, label: '`code`', run: () => applyWrap('`', '`') },
    { icon: Heading1, label: '#', run: () => applyPrefix('# ') },
    { icon: List, label: '- ', run: () => applyPrefix('- ') },
    { icon: ListOrdered, label: '1.', run: () => applyPrefix('1. ') },
    { icon: LinkIcon, label: '[]()', run: () => applyWrap('[', '](https://)') },
    { icon: Code, label: '```', run: () => applyWrap('\n```\n', '\n```\n') },
  ];

  return (
    <article className="flex h-full min-w-0 flex-col bg-background">
      <header className="flex items-center gap-1 border-b border-border px-3 py-2">
        <div className="flex items-center gap-0.5 overflow-x-auto scrollbar-hide">
          {toolbar.map((item) => (
            <IconButton key={item.label} icon={item.icon} label={item.label} size="sm" onClick={item.run} />
          ))}
        </div>

        <div className="ml-auto flex shrink-0 items-center gap-0.5">
          {(
            [
              ['edit', PenLine, t('editor.view.edit')],
              ['split', SquareCode, t('editor.view.split')],
              ['preview', Eye, t('editor.view.preview')],
            ] as const
          ).map(([mode, icon, label]) => (
            <IconButton
              key={mode}
              icon={icon}
              label={label}
              size="sm"
              active={view === mode}
              onClick={() => setView(mode)}
            />
          ))}
          <IconButton
            icon={Check}
            label={t('note.versions')}
            size="sm"
            onClick={() => onOpenVersions(note.id)}
          />
        </div>
      </header>

      <input
        value={title}
        onChange={(event) => {
          setTitle(event.target.value);
          markDirty();
        }}
        placeholder={t('editor.titlePlaceholder')}
        aria-label={t('editor.titlePlaceholder')}
        className="w-full shrink-0 border-none bg-transparent px-6 pt-5 pb-2 text-[1.3rem] font-semibold tracking-[-0.01em] text-foreground outline-none placeholder:text-muted-foreground/50"
      />

      <div
        className={cn(
          'grid min-h-0 flex-1',
          view === 'split' ? 'grid-cols-2 divide-x divide-border' : 'grid-cols-1',
        )}
      >
        {view !== 'preview' && (
          <textarea
            ref={bodyRef}
            value={body}
            onChange={(event) => {
              setBody(event.target.value);
              markDirty();
            }}
            onKeyDown={(event) => {
              if (!(event.ctrlKey || event.metaKey)) {
                return;
              }
              const key = event.key.toLowerCase();
              if (key === 'b') {
                event.preventDefault();
                applyWrap('**', '**');
              } else if (key === 'i') {
                event.preventDefault();
                applyWrap('*', '*');
              } else if (key === 's') {
                event.preventDefault();
                save.mutate({ title, body });
              }
            }}
            placeholder={t('editor.placeholder')}
            spellCheck={editorSettings.spellcheck}
            aria-label={t('editor.placeholder')}
            className={cn(
              'h-full resize-none bg-transparent px-6 py-3 text-foreground outline-none',
              'placeholder:text-muted-foreground/50',
              editorSettings.monospace ? 'font-mono' : 'font-sans',
              !editorSettings.wordWrap && 'whitespace-pre overflow-x-auto',
            )}
            style={{
              fontSize: `${editorSettings.fontSize}px`,
              lineHeight: 1.7,
              maxWidth: editorSettings.comfortableWidth && view !== 'split' ? '72ch' : undefined,
            }}
          />
        )}

        {view !== 'edit' && (
          <div
            className="markdown-body h-full overflow-y-auto px-6 py-3"
            style={{
              ['--editor-font-size' as string]: `${editorSettings.fontSize}px`,
              maxWidth: editorSettings.comfortableWidth && view !== 'split' ? '72ch' : undefined,
            }}
            // Превью собирается через DOMPurify (см. lib/markdown), поэтому
            // пользовательская разметка не может выполнить скрипт в окне.
            dangerouslySetInnerHTML={{ __html: preview }}
          />
        )}
      </div>

      <footer className="flex shrink-0 items-center gap-3 border-t border-border px-4 py-1.5 text-[0.72rem] text-muted-foreground">
        <span className={cn('flex items-center gap-1', isDirty && 'text-warning')}>
          {save.isPending ? t('editor.saving') : isDirty ? t('editor.unsaved') : t('editor.saved')}
        </span>
        <span className="tabular-nums">
          {lang === 'ru' ? pluralRu(words, 'слово', 'слова', 'слов') : t('editor.words', { count: words })}
        </span>
        <span className="tabular-nums">{t('editor.chars', { count: body.length })}</span>
        {savedAt && <span className="ml-auto tabular-nums">{formatFull(savedAt, lang)}</span>}
      </footer>
    </article>
  );
}
