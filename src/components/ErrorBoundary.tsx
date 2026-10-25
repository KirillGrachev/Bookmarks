import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle } from 'lucide-react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Граница ошибок окна.
 *
 * Без неё любое исключение в рендере (например, повреждённая заметка с
 * неожиданной разметкой) оставляет пользователя с белым окном и единственным
 * выходом — убить процесс. Здесь показывается сообщение и кнопка перезагрузки,
 * а данные при этом не теряются: они в SQLite, а не в состоянии React.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[bookmarks] render error', error, info.componentStack);
  }

  render(): ReactNode {
    if (!this.state.error) {
      return this.props.children;
    }
    return (
      <div className="grid h-full w-full place-items-center bg-background px-8 text-center">
        <div className="max-w-md">
          <AlertTriangle className="mx-auto mb-3 size-8 text-warning" aria-hidden />
          <h1 className="text-[0.95rem] font-semibold text-foreground">Интерфейс упал</h1>
          <p className="mt-2 text-[0.82rem] leading-relaxed text-muted-foreground">
            Заметки целы — они хранятся в базе, а не в окне. Перезагрузите интерфейс; если ошибка повторяется,
            сообщите текст ниже.
          </p>
          <pre className="mt-3 max-h-40 overflow-auto rounded-lg border border-border bg-muted p-3 text-left text-[0.72rem] text-muted-foreground">
            {this.state.error.message}
          </pre>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="mt-4 cursor-pointer rounded-lg bg-primary px-4 py-2 text-[0.83rem] font-medium text-primary-foreground transition-colors hover:bg-primary-hover"
          >
            Перезагрузить окно
          </button>
        </div>
      </div>
    );
  }
}
