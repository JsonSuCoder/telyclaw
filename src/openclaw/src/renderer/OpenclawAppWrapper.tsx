import React, { useImperativeHandle } from 'react';
import { Provider } from 'react-redux';
import { store } from './store';
import App from './App';
import { TelegramDataProvider } from './contexts/TelegramDataContext';
import './index.css';

import type { ApiChat } from '../../../api/types/chats';
import type { ApiUser } from '../../../api/types/users';

export interface OpenclawAppHandle {
  updateTelegramData: (data: { chatList?: ApiChat[]; contactList?: ApiUser[] }) => void;
}

interface OpenclawAppWrapperProps {
  onClose: () => void;
}

const OpenclawAppWrapper = React.forwardRef<OpenclawAppHandle, OpenclawAppWrapperProps>(
  ({ onClose }, ref) => {
    const telegramDataRef = React.useRef<{ update: (data: { chatList?: ApiChat[]; contactList?: ApiUser[] }) => void }>(null);

    useImperativeHandle(ref, () => ({
      updateTelegramData: (data) => {
        telegramDataRef.current?.update(data);
      },
    }), []);

    return (
      <Provider store={store}>
        <TelegramDataProvider ref={telegramDataRef}>
          <App onClose={onClose} />
        </TelegramDataProvider>
      </Provider>
    );
  },
);

OpenclawAppWrapper.displayName = 'OpenclawAppWrapper';

export default OpenclawAppWrapper;
