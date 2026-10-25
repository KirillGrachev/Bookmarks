import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Database, HardDrive, Info, Lock, Palette, PenLine, RefreshCw, ShieldCheck, X } from 'lucide-react';
import { toast } from 'sonner';

import { getBackend, isTauri } from '@/api/backend';
import { queryKeys } from '@/api/keys';
import type { AccentColor, Language, Settings, Theme } from '@/api/contracts';
import { Button } from '@/components/ui/Button';
import { IconButton } from '@/components/ui/IconButton';
import { Switch } from '@/components/ui/Switch';
import { TextField } from '@/components/ui/TextField';
import { useI18n } from '@/hooks/useI18n';
import type { Translator } from '@/lib/i18n';
import { ACCENT_COLORS } from '@/api/contracts';
import { cn, formatBytes } from '@/lib/utils';
import { useAppStore } from '@/store/useAppStore';

type SectionId = 'interface' | 'editor' | 'vault' | 'sync' | 'data' | 'about';

/**
 * Настройки: отдельный экран поверх списка, а не модальное окно.
 *
 * Разделов шесть, в каждом по несколько экранов прокрутки — в модалке это
 * превращается в «окно в окне» с собственным скроллом, что на десктопе
 * ощущается тесно.
 */
export function SettingsPage() {
  const { t } = useI18n();
  const [section, setSection] = useState<SectionId>('interface');
  const setSettingsOpen = useAppStore((state) => state.setSettingsOpen);
  const settings = useAppStore((state) => state.settings);
  const patchInterface = useAppStore((state) => state.patchInterface);
  const patchEditor = useAppStore((state) => state.patchEditor);
  const setSettings = useAppStore((state) => state.setSettings);
  const saveSettings = useAppStore((state) => state.saveSettings);
  const resetSettings = useAppStore((state) => state.resetSettings);
  const refreshVault = useAppStore((state) => state.refreshVault);
  const vault = useAppStore((state) => state.vault);

  const sections: Array<{ id: SectionId; label: string; icon: typeof Palette }> = [
    { id: 'interface', label: t('settings.interface'), icon: Palette },
    { id: 'editor', label: t('settings.editor'), icon: PenLine },
    { id: 'vault', label: t('settings.vault'), icon: Lock },
    { id: 'sync', label: t('settings.sync'), icon: RefreshCw },
    { id: 'data', label: t('settings.data'), icon: Database },
    { id: 'about', label: t('settings.about'), icon: Info },
  ];

  /** Любое изменение сразу уходит в ядро: кнопка «Сохранить» в настройках — лишний шаг. */
  const persist = async (next: Settings) => {
    setSettings(next);
    await saveSettings();
  };

  return (
    <div className="absolute inset-0 z-[70] flex min-h-0 flex-col bg-background fade-in">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
        <h1 className="text-[0.9rem] font-semibold text-foreground">{t('settings.title')}</h1>
        <IconButton
          icon={X}
          label={t('common.close')}
          className="ml-auto"
          onClick={() => setSettingsOpen(false)}
        />
      </header>

      <div className="flex min-h-0 flex-1">
        <nav
          className="w-56 shrink-0 overflow-y-auto border-r border-border px-2 py-3"
          aria-label={t('settings.title')}
        >
          <ul className="space-y-0.5">
            {sections.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  onClick={() => setSection(item.id)}
                  aria-current={section === item.id}
                  className={cn(
                    'flex w-full cursor-pointer items-center gap-2 rounded-lg px-3 py-2 text-left text-[0.83rem] transition-colors',
                    section === item.id
                      ? 'bg-primary/10 text-primary'
                      : 'text-muted-foreground hover:bg-surface-hover hover:text-foreground',
                  )}
                >
                  <item.icon className="size-4" aria-hidden />
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <div className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
          {section === 'interface' && (
            <InterfaceSection
              settings={settings}
              t={t}
              onPatch={async (patch) => {
                patchInterface(patch);
                await persist({ ...settings, interface: { ...settings.interface, ...patch } });
              }}
            />
          )}

          {section === 'editor' && (
            <EditorSection
              settings={settings}
              t={t}
              onPatch={async (patch) => {
                patchEditor(patch);
                await persist({ ...settings, editor: { ...settings.editor, ...patch } });
              }}
            />
          )}

          {section === 'vault' && <VaultSection vault={vault} onRefresh={refreshVault} t={t} />}

          {section === 'sync' && (
            <SyncSection
              settings={settings}
              onChange={async (patch) => persist({ ...settings, sync: { ...settings.sync, ...patch } })}
              t={t}
            />
          )}

          {section === 'data' && <DataSection t={t} />}

          {section === 'about' && <AboutSection t={t} />}

          <footer className="mt-8 border-t border-border pt-4">
            <Button
              variant="ghost"
              size="sm"
              onClick={async () => {
                await resetSettings();
                toast.success(t('settings.saved'));
              }}
            >
              {t('settings.reset')}
            </Button>
          </footer>
        </div>
      </div>
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-border/60 py-3 last:border-none">
      <div className="min-w-0">
        <p className="text-[0.84rem] text-foreground">{label}</p>
        {hint && <p className="mt-0.5 text-[0.76rem] leading-relaxed text-muted-foreground">{hint}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function InterfaceSection({
  settings,
  t,
  onPatch,
}: {
  settings: Settings;
  t: Translator;
  onPatch: (patch: Partial<Settings['interface']>) => Promise<void>;
}) {
  const current = settings.interface;
  return (
    <section>
      <h2 className="mb-3 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        {t('settings.interface')}
      </h2>

      <Row label={t('settings.theme')}>
        <div className="flex overflow-hidden rounded-lg border border-border">
          {(['light', 'dark', 'system'] as Theme[]).map((theme) => (
            <button
              key={theme}
              type="button"
              onClick={() => void onPatch({ theme })}
              className={cn(
                'cursor-pointer px-3 py-1.5 text-[0.78rem] transition-colors',
                current.theme === theme
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-surface-hover',
              )}
            >
              {t(`settings.theme.${theme}`)}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t('settings.accent')}>
        <div className="flex gap-1.5">
          {ACCENT_COLORS.map((accent) => (
            <button
              key={accent}
              type="button"
              onClick={() => void onPatch({ accent: accent as AccentColor })}
              aria-label={accent}
              aria-pressed={current.accent === accent}
              className={cn(
                'size-6 cursor-pointer rounded-full border-2 transition-all',
                `dot-${accent}`,
                current.accent === accent ? 'border-foreground' : 'border-transparent hover:scale-110',
              )}
              style={{ backgroundColor: 'var(--dot)' }}
            />
          ))}
        </div>
      </Row>

      <Row label={t('settings.language')}>
        <div className="flex overflow-hidden rounded-lg border border-border">
          {(['ru', 'en'] as Language[]).map((language) => (
            <button
              key={language}
              type="button"
              onClick={() => void onPatch({ language })}
              className={cn(
                'cursor-pointer px-3 py-1.5 text-[0.78rem] uppercase transition-colors',
                current.language === language
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-surface-hover',
              )}
            >
              {language}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t('settings.animations')}>
        <Switch
          checked={current.animations}
          onChange={(animations) => void onPatch({ animations })}
          ariaLabel={t('settings.animations')}
        />
      </Row>

      <Row label={t('settings.compactList')}>
        <Switch
          checked={current.compactList}
          onChange={(compactList) => void onPatch({ compactList })}
          ariaLabel={t('settings.compactList')}
        />
      </Row>

      <Row label={t('settings.showSidebar')}>
        <Switch
          checked={current.showSidebar}
          onChange={(showSidebar) => void onPatch({ showSidebar })}
          ariaLabel={t('settings.showSidebar')}
        />
      </Row>

      <Row label={t('settings.showExcerpts')}>
        <Switch
          checked={current.showExcerpts}
          onChange={(showExcerpts) => void onPatch({ showExcerpts })}
          ariaLabel={t('settings.showExcerpts')}
        />
      </Row>
    </section>
  );
}

function EditorSection({
  settings,
  t,
  onPatch,
}: {
  settings: Settings;
  t: Translator;
  onPatch: (patch: Partial<Settings['editor']>) => Promise<void>;
}) {
  const current = settings.editor;
  return (
    <section>
      <h2 className="mb-3 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        {t('settings.editor')}
      </h2>

      <Row label={t('settings.fontSize')} hint={`${current.fontSize}px`}>
        <input
          type="range"
          min={11}
          max={28}
          step={1}
          value={current.fontSize}
          onChange={(event) => void onPatch({ fontSize: Number(event.target.value) })}
          aria-label={t('settings.fontSize')}
          className="w-44 cursor-pointer accent-[var(--primary)]"
        />
      </Row>

      <Row label={t('settings.defaultView')}>
        <div className="flex overflow-hidden rounded-lg border border-border">
          {(['edit', 'split', 'preview'] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => void onPatch({ defaultView: mode })}
              className={cn(
                'cursor-pointer px-3 py-1.5 text-[0.78rem] transition-colors',
                current.defaultView === mode
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:bg-surface-hover',
              )}
            >
              {t(`editor.view.${mode}`)}
            </button>
          ))}
        </div>
      </Row>

      <Row label={t('settings.monospace')}>
        <Switch
          checked={current.monospace}
          onChange={(monospace) => void onPatch({ monospace })}
          ariaLabel={t('settings.monospace')}
        />
      </Row>
      <Row label={t('settings.comfortableWidth')}>
        <Switch
          checked={current.comfortableWidth}
          onChange={(comfortableWidth) => void onPatch({ comfortableWidth })}
          ariaLabel={t('settings.comfortableWidth')}
        />
      </Row>
      <Row label={t('settings.wordWrap')}>
        <Switch
          checked={current.wordWrap}
          onChange={(wordWrap) => void onPatch({ wordWrap })}
          ariaLabel={t('settings.wordWrap')}
        />
      </Row>
      <Row label={t('settings.spellcheck')}>
        <Switch
          checked={current.spellcheck}
          onChange={(spellcheck) => void onPatch({ spellcheck })}
          ariaLabel={t('settings.spellcheck')}
        />
      </Row>
      <Row label={t('settings.autosave')}>
        <Switch
          checked={current.autosave}
          onChange={(autosave) => void onPatch({ autosave })}
          ariaLabel={t('settings.autosave')}
        />
      </Row>
      <Row label={t('settings.autosaveDelay')}>
        <input
          type="number"
          min={200}
          max={10000}
          step={100}
          value={current.autosaveDelayMs}
          onChange={(event) => void onPatch({ autosaveDelayMs: Number(event.target.value) })}
          aria-label={t('settings.autosaveDelay')}
          className="w-24 rounded-lg border border-input bg-surface px-2 py-1 text-[0.8rem] outline-none focus:border-primary"
        />
      </Row>
    </section>
  );
}

function VaultSection({
  vault,
  onRefresh,
  t,
}: {
  vault: { initialized: boolean; unlocked: boolean };
  onRefresh: () => Promise<void>;
  t: Translator;
}) {
  const [password, setPassword] = useState('');
  const [repeat, setRepeat] = useState('');
  const [error, setError] = useState<string | null>(null);

  const run = useMutation({
    mutationFn: async () => {
      if (!vault.initialized) {
        if (password !== repeat) {
          throw new Error('Пароли не совпадают');
        }
        await getBackend().vaultInitialize(password);
      } else if (!vault.unlocked) {
        await getBackend().vaultUnlock(password);
      } else {
        await getBackend().vaultLock();
      }
    },
    onSuccess: async () => {
      setPassword('');
      setRepeat('');
      setError(null);
      await onRefresh();
      toast.success(vault.unlocked ? t('vault.locked') : t('vault.unlocked'));
    },
    onError: (err: Error) => {
      setError(err.message);
      void onRefresh();
    },
  });

  return (
    <section className="max-w-xl">
      <h2 className="mb-3 flex items-center gap-2 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        <ShieldCheck className="size-4" aria-hidden />
        {t('settings.vault')}
      </h2>

      <p className="mb-4 rounded-lg border border-border bg-muted/40 p-3 text-[0.78rem] leading-relaxed text-muted-foreground">
        {t('vault.hint')}
      </p>

      <div className="mb-4 flex items-center gap-2 text-[0.82rem]">
        <span
          className={cn(
            'size-2 rounded-full',
            !vault.initialized ? 'bg-muted-foreground/40' : vault.unlocked ? 'bg-success' : 'bg-warning',
          )}
          aria-hidden
        />
        {!vault.initialized ? t('vault.create') : vault.unlocked ? t('vault.unlocked') : t('vault.locked')}
      </div>

      {!isTauri() && (
        <p className="mb-3 rounded-lg border border-warning/30 bg-warning/10 p-3 text-[0.78rem] text-warning">
          {t('about.demo')}
        </p>
      )}

      <div className="space-y-3">
        {(!vault.unlocked || !vault.initialized) && (
          <>
            <TextField
              type="password"
              label={t('vault.password')}
              value={password}
              error={error}
              autoComplete="new-password"
              onChange={(event) => {
                setPassword(event.target.value);
                setError(null);
              }}
            />
            {!vault.initialized && (
              <TextField
                type="password"
                label={t('vault.passwordRepeat')}
                value={repeat}
                autoComplete="new-password"
                onChange={(event) => setRepeat(event.target.value)}
              />
            )}
          </>
        )}
        <Button
          onClick={() => run.mutate()}
          disabled={run.isPending || (!vault.unlocked && password.length < 8)}
        >
          {!vault.initialized ? t('vault.create') : vault.unlocked ? t('vault.lock') : t('vault.unlock')}
        </Button>
      </div>
    </section>
  );
}

function SyncSection({
  settings,
  onChange,
  t,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings['sync']>) => Promise<void>;
  t: Translator;
}) {
  const sync = settings.sync;
  const probe = useMutation({
    mutationFn: () => getBackend().syncProbe(),
    onSuccess: () => toast.success(t('sync.probeOk')),
    onError: (error: Error) => toast.error(error.message),
  });

  return (
    <section className="max-w-xl">
      <h2 className="mb-3 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        {t('settings.sync')}
      </h2>
      <p className="mb-4 text-[0.78rem] leading-relaxed text-muted-foreground">{t('sync.hint')}</p>

      <Row label={t('sync.remote')}>
        <select
          value={sync.remote}
          onChange={(event) => void onChange({ remote: event.target.value as Settings['sync']['remote'] })}
          aria-label={t('sync.remote')}
          className="cursor-pointer rounded-lg border border-input bg-surface px-2 py-1.5 text-[0.8rem] outline-none focus:border-primary"
        >
          <option value="none">{t('sync.remote.none')}</option>
          <option value="directory">{t('sync.remote.directory')}</option>
          <option value="web_dav">{t('sync.remote.webDav')}</option>
        </select>
      </Row>

      {sync.remote !== 'none' && (
        <>
          <div className="py-3">
            <TextField
              label={t('sync.location')}
              value={sync.location}
              onChange={(event) => void onChange({ location: event.target.value })}
            />
          </div>
          <Row label={t('sync.autoSync')}>
            <Switch
              checked={sync.autoSync}
              onChange={(autoSync) => void onChange({ autoSync })}
              ariaLabel={t('sync.autoSync')}
            />
          </Row>
          <Row label={t('sync.interval')}>
            <input
              type="number"
              min={30}
              max={86400}
              value={sync.intervalSecs}
              onChange={(event) => void onChange({ intervalSecs: Number(event.target.value) })}
              aria-label={t('sync.interval')}
              className="w-24 rounded-lg border border-input bg-surface px-2 py-1 text-[0.8rem] outline-none focus:border-primary"
            />
          </Row>
          <Row label={t('sync.encryptRemote')} hint="AES-256-GCM, ключ — из пароля сейфа">
            <Switch
              checked={sync.encryptRemote}
              onChange={(encryptRemote) => void onChange({ encryptRemote })}
              ariaLabel={t('sync.encryptRemote')}
            />
          </Row>
          <div className="flex gap-2 py-3">
            <Button size="sm" variant="secondary" onClick={() => probe.mutate()} disabled={probe.isPending}>
              {t('sync.probe')}
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

function DataSection({ t }: { t: Translator }) {
  const queryClient = useQueryClient();
  const stats = useQuery({ queryKey: queryKeys.storageStats(), queryFn: () => getBackend().storageStats() });

  const integrity = useMutation({
    mutationFn: () => getBackend().verifyIntegrity(),
    onSuccess: (problems) => {
      if (problems.length === 0) {
        toast.success(t('data.integrityOk'));
      } else {
        toast.error(t('data.integrityBad' as never, { count: problems.length }));
      }
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const rebuild = useMutation({
    mutationFn: () => getBackend().rebuildSearchIndex(),
    onSuccess: (count) => {
      void queryClient.invalidateQueries({ queryKey: ['notes'] });
      toast.success(t('data.rebuilt' as never, { count }));
    },
    onError: (error: Error) => toast.error(error.message),
  });

  const data = stats.data;

  return (
    <section className="max-w-xl">
      <h2 className="mb-3 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        {t('settings.data')}
      </h2>

      <dl className="mb-5 grid grid-cols-2 gap-x-6 gap-y-2 text-[0.82rem]">
        {(
          [
            ['data.notes', data?.notes],
            ['data.versions', data?.versions],
            ['data.folders', data?.folders],
            ['data.tags', data?.tags],
            ['data.archived', data?.archivedNotes],
            ['data.trashed', data?.notesInTrash],
            ['data.encrypted', data?.encryptedNotes],
          ] as const
        ).map(([key, value]) => (
          <div key={key} className="flex items-baseline justify-between border-b border-border/50 py-1.5">
            <dt className="text-muted-foreground">{t(key)}</dt>
            <dd className="font-medium tabular-nums text-foreground">{value ?? '—'}</dd>
          </div>
        ))}
        <div className="flex items-baseline justify-between border-b border-border/50 py-1.5">
          <dt className="text-muted-foreground">{t('data.dbSize')}</dt>
          <dd className="font-medium tabular-nums text-foreground">
            {data ? formatBytes(data.databaseBytes) : '—'}
          </dd>
        </div>
      </dl>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={() => integrity.mutate()}
          disabled={integrity.isPending}
        >
          <ShieldCheck className="size-4" aria-hidden />
          {t('data.integrity')}
        </Button>
        <Button size="sm" variant="secondary" onClick={() => rebuild.mutate()} disabled={rebuild.isPending}>
          <HardDrive className="size-4" aria-hidden />
          {t('data.rebuildIndex')}
        </Button>
      </div>
    </section>
  );
}

function AboutSection({ t }: { t: Translator }) {
  const info = useQuery({ queryKey: ['app-info'], queryFn: () => getBackend().appInfo() });

  return (
    <section className="max-w-xl">
      <h2 className="mb-3 text-[0.8rem] font-semibold tracking-wide text-muted-foreground uppercase">
        {t('settings.about')}
      </h2>
      <p className="mb-4 text-[1.1rem] font-semibold text-foreground">Bookmarks</p>
      <p className="mb-5 text-[0.82rem] text-muted-foreground">{t('app.tagline')}</p>

      <dl className="space-y-1.5 text-[0.82rem]">
        {(
          [
            ['about.version', info.data?.version],
            ['about.platform', info.data && `${info.data.platform} · ${info.data.arch}`],
            ['about.dataDir', info.data?.dataDir],
            ['about.database', info.data?.databasePath],
          ] as const
        ).map(([key, value]) => (
          <div key={key} className="flex gap-3">
            <dt className="w-40 shrink-0 text-muted-foreground">{t(key)}</dt>
            <dd className="min-w-0 break-all text-foreground">{value ?? '—'}</dd>
          </div>
        ))}
      </dl>

      <p className="mt-5 text-[0.78rem] text-muted-foreground">
        {t('about.license')} ·{' '}
        <a
          className="text-primary underline-offset-2 hover:underline"
          href="https://github.com/KirillGrachev/Bookmarks"
          target="_blank"
          rel="noreferrer noopener"
        >
          {t('about.repo')}
        </a>
      </p>
    </section>
  );
}
