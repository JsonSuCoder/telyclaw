import React from 'react';
import ReactDOM from 'react-dom/client';
import OpenclawAppWrapper from './OpenclawAppWrapper';

import type { OpenclawAppHandle } from './OpenclawAppWrapper';
import type { ApiChat } from '../../../api/types/chats';
import type { ApiUser } from '../../../api/types/users';

export interface OpenclawInstance {
  unmount: () => void;
  updateTelegramData: (data: { chatList?: ApiChat[]; contactList?: ApiUser[] }) => void;
}

export function mountOpenclaw(
  container: HTMLElement,
  props: { onClose: () => void },
): OpenclawInstance {
  const root = ReactDOM.createRoot(container);
  const appRef = React.createRef<OpenclawAppHandle>();

  root.render(
    <React.StrictMode>
      <OpenclawAppWrapper ref={appRef} onClose={props.onClose} />
    </React.StrictMode>
  );

  return {
    unmount: () => {
      setTimeout(() => {
        root.unmount();
      }, 0);
    },
    updateTelegramData: (data) => {
      appRef.current?.updateTelegramData(data);
    },
  };
}


