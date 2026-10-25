import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';

import App from './App';
import { isTauri, setBackend, TauriBackend } from '@/api/backend';
import { DemoBackend } from '@/api/demo';
import './index.css';

// Ядро выбирается до первого рендера: компоненты не должны проверять режим.
setBackend(isTauri() ? new TauriBackend() : DemoBackend.shared());

/**
 * Клиент данных настроен под локальное хранилище.
 *
 * `staleTime: Infinity` — данные не меняются сами по себе, только через наши
 * команды, поэтому кэш не «протухает» по таймеру, а сбрасывается явно после
 * мутаций. Повторный запрос при возврате в окно лишь дёргал бы базу впустую.
 */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      gcTime: 10 * 60 * 1000,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      retry: 1,
    },
    mutations: { retry: 0 },
  },
});

const container = document.getElementById('root');
if (!container) {
  throw new Error('root container not found');
}

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
