import React, { createContext, useCallback, useContext, useState } from 'react';

import type { ApiChat } from '../../../../api/types/chats';
import type { ApiUser } from '../../../../api/types/users';

interface TelegramDataContextValue {
  chatList: ApiChat[];
  contactList: ApiUser[];
}

interface TelegramDataHandle {
  update: (data: Partial<TelegramDataContextValue>) => void;
}

const TelegramDataContext = createContext<TelegramDataContextValue>({
  chatList: [],
  contactList: [],
});

export const useTelegramData = () => useContext(TelegramDataContext);

interface TelegramDataProviderProps {
  children: React.ReactNode;
}

export const TelegramDataProvider = React.forwardRef<TelegramDataHandle, TelegramDataProviderProps>(
  ({ children }, ref) => {
    const [data, setData] = useState<TelegramDataContextValue>({
      chatList: [],
      contactList: [],
    });

    const update = useCallback((partial: Partial<TelegramDataContextValue>) => {
      setData((prev) => ({ ...prev, ...partial }));
    }, []);

    React.useImperativeHandle(ref, () => ({ update }), [update]);

    return (
      <TelegramDataContext.Provider value={data}>
        {children}
      </TelegramDataContext.Provider>
    );
  },
);

TelegramDataProvider.displayName = 'TelegramDataProvider';
