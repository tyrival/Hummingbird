import '@mantine/core/styles.css';
import '@mantine/notifications/styles.css';
import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './app.css';
import { hummingbirdColorSchemeManager, hummingbirdTheme } from './theme';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MantineProvider
      colorSchemeManager={hummingbirdColorSchemeManager}
      defaultColorScheme="auto"
      theme={hummingbirdTheme}
    >
      <Notifications />
      <App />
    </MantineProvider>
  </StrictMode>,
);
