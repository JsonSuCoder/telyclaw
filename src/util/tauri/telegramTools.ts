/**
 * Telegram Tools for OpenClaw
 *
 * Exposes Telegram methods and tools to the Tauri backend,
 * allowing OpenClaw AI agents to query Telegram chats, messages, and users.
 *
 * Architecture:
 * OpenClaw Agent → Tauri Backend → emit event → Frontend (this) → execute selectors → emit response
 */

import { getActions, getGlobal } from '../../global';
import {
  selectChat,
  selectChatFullInfo,
} from '../../global/selectors/chats';
import {
  selectUser,
  selectUserFullInfo,
} from '../../global/selectors/users';
import { selectThreadReadState } from '../../global/selectors/threads';
import { MAIN_THREAD_ID } from '../../api/types/messages';

// Query handlers
const queryHandlers: Record<string, (params: Record<string, unknown>) => unknown> = {
  // Get current user info
  'getCurrentUser': () => {
    const global = getGlobal();
    if (!global.currentUserId) return null;
    const user = selectUser(global, global.currentUserId);
    return user ? sanitizeUser(user) : null;
  },

  // Get user by ID (camelCase)
  'getUser': (params) => {
    const global = getGlobal();
    const userId = String(params.userId || params.user_id || '');
    if (!userId) return null;
    const user = selectUser(global, userId);
    return user ? sanitizeUser(user) : null;
  },

  // Get user by ID (snake_case alias)
  'get_user': (params) => {
    const global = getGlobal();
    const userId = String(params.user_id || params.userId || '');
    if (!userId) return null;
    const user = selectUser(global, userId);
    return user ? sanitizeUser(user) : null;
  },

  // Search users by name/username
  'search_user': (params) => {
    const global = getGlobal();
    const query = String(params.query || '').toLowerCase();
    const limit = Math.min(Number(params.limit) || 10, 20);

    if (!query) return [];

    const results: unknown[] = [];
    for (const user of Object.values(global.users.byId)) {
      const firstName = (user.firstName || '').toLowerCase();
      const lastName = (user.lastName || '').toLowerCase();
      const username = (user.usernames?.[0]?.username || '').toLowerCase();

      if (firstName.includes(query) || lastName.includes(query) || username.includes(query)) {
        results.push(sanitizeUser(user));
        if (results.length >= limit) break;
      }
    }
    return results;
  },

  // Get user full info
  'getUserFullInfo': (params) => {
    const global = getGlobal();
    const userId = String(params.userId || '');
    if (!userId) return null;
    const fullInfo = selectUserFullInfo(global, userId);
    return fullInfo ? sanitizeUserFullInfo(fullInfo) : null;
  },

  // Get chat by ID
  'getChat': (params) => {
    const global = getGlobal();
    const chatId = String(params.chatId || '');
    if (!chatId) return null;
    const chat = selectChat(global, chatId);
    return chat ? sanitizeChat(chat) : null;
  },

  // Get chat full info
  'getChatFullInfo': (params) => {
    const global = getGlobal();
    const chatId = String(params.chatId || '');
    if (!chatId) return null;
    const fullInfo = selectChatFullInfo(global, chatId);
    return fullInfo ? sanitizeChatFullInfo(fullInfo) : null;
  },

  // List all chats (alias: getChats) — compact, no page limit
  'getChats': (params) => {
    return queryHandlers.listChats(params);
  },

  // List all chats — compact, returns all chats from Redux store
  'listChats': (params) => {
    const global = getGlobal();
    const chatIds = global.chats.listIds?.active || [];
    const limit = Number(params.limit) || 0; // 0 = all
    const offset = Number(params.offset) || 0;

    const ids = limit > 0 ? chatIds.slice(offset, offset + limit) : chatIds.slice(offset);

    return ids.map((chatId) => {
      const chat = global.chats.byId[chatId];
      if (!chat) return null;
      const readState = selectThreadReadState(global, chatId, MAIN_THREAD_ID);
      return {
        id: chat.id,
        title: chat.title,
        type: chat.type,
        username: chat.usernames?.[0]?.username,
        unreadCount: readState?.unreadCount || 0,
      };
    }).filter(Boolean);
  },

  // Search chats by title
  'searchChats': (params) => {
    const global = getGlobal();
    const query = String(params.query || '').toLowerCase();
    const limit = Number(params.limit) || 20;

    if (!query) return [];

    const results: unknown[] = [];
    for (const chat of Object.values(global.chats.byId)) {
      if (chat.title?.toLowerCase().includes(query)) {
        results.push(sanitizeChat(chat));
        if (results.length >= limit) break;
      }
    }
    return results;
  },

  // Get messages from a chat
  'getMessages': (params) => {
    const global = getGlobal();
    const chatId = String(params.chatId || '');
    const limit = Math.min(Number(params.limit) || 20, 50);
    const offsetId = params.offsetId ? Number(params.offsetId) : undefined;

    if (!chatId) return [];

    const chatMessages = global.messages.byChatId[chatId];
    if (!chatMessages?.byId) return [];

    let messageIds = Object.keys(chatMessages.byId).map(Number).sort((a, b) => b - a);

    if (offsetId) {
      const offsetIndex = messageIds.indexOf(offsetId);
      if (offsetIndex !== -1) {
        messageIds = messageIds.slice(offsetIndex + 1);
      }
    }

    return messageIds.slice(0, limit).map(msgId => {
      const msg = chatMessages.byId[msgId];
      return msg ? sanitizeMessage(msg) : null;
    }).filter(Boolean);
  },

  // Get a specific message
  'getMessage': (params) => {
    const global = getGlobal();
    const chatId = String(params.chatId || '');
    const messageId = Number(params.messageId);

    if (!chatId || !messageId) return null;

    const chatMessages = global.messages.byChatId[chatId];
    const msg = chatMessages?.byId?.[messageId];
    return msg ? sanitizeMessage(msg) : null;
  },

  // Search messages
  'searchMessages': (params) => {
    const global = getGlobal();
    const chatId = params.chatId ? String(params.chatId) : undefined;
    const query = String(params.query || '').toLowerCase();
    const limit = Math.min(Number(params.limit) || 10, 20);

    if (!query) return [];

    const results: unknown[] = [];
    const chatIds = chatId ? [chatId] : Object.keys(global.messages.byChatId);

    for (const cid of chatIds) {
      const chatMessages = global.messages.byChatId[cid];
      if (!chatMessages?.byId) continue;

      for (const msg of Object.values(chatMessages.byId)) {
        if (msg.content?.text?.text?.toLowerCase().includes(query)) {
          results.push(sanitizeMessage(msg));
          if (results.length >= limit) break;
        }
      }
      if (results.length >= limit) break;
    }

    return results;
  },

  // Get unread counts
  'getUnreadCounts': () => {
    const global = getGlobal();
    const chats = global.chats.byId;

    let totalUnread = 0;
    let totalMuted = 0;
    const byChat: Record<string, number> = {};

    for (const [chatId, chat] of Object.entries(chats)) {
      const readState = selectThreadReadState(global, chatId, MAIN_THREAD_ID);
      const unread = readState?.unreadCount || 0;
      if (unread > 0) {
        byChat[chatId] = unread;
        totalUnread += unread;
        if ((chat as any).isMuted) totalMuted += unread;
      }
    }

    return { totalUnread, totalMuted, byChat };
  },

  // List contacts
  'listContacts': (params) => {
    const global = getGlobal();
    const limit = Number(params.limit) || 50;

    const contactIds = global.contactList?.userIds || [];
    return contactIds.slice(0, limit).map(userId => {
      const user = global.users.byId[userId];
      return user ? sanitizeUser(user) : null;
    }).filter(Boolean);
  },

  // Search users by name/username
  'searchUsers': (params) => {
    const global = getGlobal();
    const query = String(params.query || '').toLowerCase();
    const limit = Math.min(Number(params.limit) || 10, 20);

    if (!query) return [];

    const results: unknown[] = [];
    for (const user of Object.values(global.users.byId)) {
      const firstName = (user.firstName || '').toLowerCase();
      const lastName = (user.lastName || '').toLowerCase();
      const username = (user.usernames?.[0]?.username || '').toLowerCase();
      const fullName = `${firstName} ${lastName}`.trim();

      if (firstName.includes(query) || lastName.includes(query) ||
          username.includes(query) || fullName.includes(query)) {
        results.push(sanitizeUser(user));
        if (results.length >= limit) break;
      }
    }
    return results;
  },

  // Send a text message to a chat
  'sendMessage': (params) => {
    const chatId = String(params.chatId || params.chat_id || '');
    const text = String(params.text || '');

    if (!chatId) return { success: false, error: 'chatId is required' };
    if (!text) return { success: false, error: 'text is required' };

    const global = getGlobal();
    const chat = selectChat(global, chatId);
    if (!chat) return { success: false, error: `Chat not found: ${chatId}` };

    getActions().sendMessage({
      messageList: {
        chatId,
        threadId: MAIN_THREAD_ID,
        type: 'thread',
      },
      text,
    });

    return { success: true, data: { chatId, text } };
  },

  // Execute a tool call (for Claude AI)
  'executeTool': (params) => {
    const toolName = String(params.toolName || '');
    const input = (params.toolInput || params.input || {}) as Record<string, unknown>;

    // Map tool names to handlers
    const toolMapping: Record<string, { handler: string; mapParams: (i: Record<string, unknown>) => Record<string, unknown> }> = {
      'telegram_search_user': {
        handler: 'searchUsers',
        mapParams: (i) => ({ query: i.query, limit: i.limit }),
      },
      'telegram_get_user': {
        handler: 'getUser',
        mapParams: (i) => ({ userId: i.user_id }),
      },
      'telegram_get_messages': {
        handler: 'getMessages',
        mapParams: (i) => ({ chatId: i.chat_id, limit: i.limit, offsetId: i.offset_id }),
      },
      'telegram_search_messages': {
        handler: 'searchMessages',
        mapParams: (i) => ({ chatId: i.chat_id, query: i.query, limit: i.limit }),
      },
      'telegram_list_chats': {
        handler: 'listChats',
        mapParams: (i) => ({ limit: i.limit, offset: i.offset }),
      },
      'telegram_get_chat': {
        handler: 'getChat',
        mapParams: (i) => ({ chatId: i.chat_id }),
      },
      'telegram_send_message': {
        handler: 'sendMessage',
        mapParams: (i) => ({ chatId: i.chat_id, text: i.text }),
      },
    };

    const mapping = toolMapping[toolName];
    if (!mapping) {
      return { success: false, error: `Unknown tool: ${toolName}` };
    }

    const handler = queryHandlers[mapping.handler];
    if (!handler) {
      return { success: false, error: `Handler not found: ${mapping.handler}` };
    }

    try {
      const mappedParams = mapping.mapParams(input);
      const result = handler(mappedParams);
      return { success: true, data: result };
    } catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Unknown error' };
    }
  },

  // Get tool definitions for Claude AI
  'getToolDefinitions': () => {
    return [
      {
        name: 'telegram_search_user',
        description: 'Search Telegram users by name or username. Use this when the user asks about a Telegram contact.',
        input_schema: {
          type: 'object',
          properties: {
            query: { type: 'string', description: 'Name or username to search for' },
            limit: { type: 'number', description: 'Max results (default 20)' },
          },
          required: ['query'],
        },
      },
      {
        name: 'telegram_get_user',
        description: 'Get detailed info about a Telegram user by their ID.',
        input_schema: {
          type: 'object',
          properties: {
            user_id: { type: 'string', description: 'The user ID' },
          },
          required: ['user_id'],
        },
      },
      {
        name: 'telegram_get_messages',
        description: 'Get recent messages from a Telegram chat.',
        input_schema: {
          type: 'object',
          properties: {
            chat_id: { type: 'string', description: 'The chat ID' },
            limit: { type: 'number', description: 'Max messages (default 50)' },
            offset_id: { type: 'number', description: 'Start from this message ID' },
          },
          required: ['chat_id'],
        },
      },
      {
        name: 'telegram_search_messages',
        description: 'Search messages by text content.',
        input_schema: {
          type: 'object',
          properties: {
            query: { type: 'string', description: 'Text to search for' },
            chat_id: { type: 'string', description: 'Optional: limit to this chat' },
            limit: { type: 'number', description: 'Max results (default 20)' },
          },
          required: ['query'],
        },
      },
      {
        name: 'telegram_list_chats',
        description: 'List all Telegram chats/conversations. Returns compact data (id, title, type, username, unreadCount) for every chat. Omit limit to get all chats.',
        input_schema: {
          type: 'object',
          properties: {
            limit: { type: 'number', description: 'Max chats to return (0 or omit = all)' },
            offset: { type: 'number', description: 'Skip first N chats' },
          },
        },
      },
      {
        name: 'telegram_get_chat',
        description: 'Get info about a specific Telegram chat.',
        input_schema: {
          type: 'object',
          properties: {
            chat_id: { type: 'string', description: 'The chat ID' },
          },
          required: ['chat_id'],
        },
      },
      {
        name: 'telegram_send_message',
        description: 'Send a text message to a Telegram chat. Use this when the user asks to send a message to someone.',
        input_schema: {
          type: 'object',
          properties: {
            chat_id: { type: 'string', description: 'The chat ID to send the message to' },
            text: { type: 'string', description: 'The message text to send' },
          },
          required: ['chat_id', 'text'],
        },
      },
    ];
  },
};

// Sanitize functions to remove sensitive/internal data
function sanitizeUser(user: any): Record<string, unknown> {
  return {
    id: user.id,
    firstName: user.firstName,
    lastName: user.lastName,
    username: user.usernames?.[0]?.username,
    phoneNumber: user.phoneNumber ? '***' : undefined, // Hide phone for privacy
    isBot: user.isBot,
    isPremium: user.isPremium,
    isVerified: user.isVerified,
    type: user.type,
  };
}

function sanitizeUserFullInfo(info: any): Record<string, unknown> {
  return {
    bio: info.bio,
    commonChatsCount: info.commonChatsCount,
    isBlocked: info.isBlocked,
  };
}

function sanitizeChat(chat: any, includeLastMessage = false): Record<string, unknown> {
  const global = getGlobal();
  const readState = selectThreadReadState(global, chat.id, MAIN_THREAD_ID);
  const result: Record<string, unknown> = {
    id: chat.id,
    title: chat.title,
    type: chat.type,
    username: chat.usernames?.[0]?.username,
    membersCount: chat.membersCount,
    unreadCount: readState?.unreadCount || 0,
    isMuted: chat.isMuted,
    isVerified: chat.isVerified,
    isCreator: chat.isCreator,
  };
  if (includeLastMessage && chat.lastMessage) {
    result.lastMessage = sanitizeMessage(chat.lastMessage);
  }
  return result;
}

function sanitizeChatFullInfo(info: any): Record<string, unknown> {
  return {
    about: info.about,
    membersCount: info.membersCount,
    onlineCount: info.onlineCount,
    linkedChatId: info.linkedChatId,
  };
}

const MAX_MESSAGE_TEXT_LENGTH = 500;

function sanitizeMessage(msg: any): Record<string, unknown> {
  let text = msg.content?.text?.text;
  if (typeof text === 'string' && text.length > MAX_MESSAGE_TEXT_LENGTH) {
    text = text.slice(0, MAX_MESSAGE_TEXT_LENGTH) + '…';
  }
  return {
    id: msg.id,
    chatId: msg.chatId,
    senderId: msg.senderId,
    date: msg.date,
    text,
    isOutgoing: msg.isOutgoing,
    isForwarded: !!msg.forwardInfo,
    replyToMessageId: msg.replyInfo?.replyToMsgId,
    hasMedia: !!(msg.content?.photo || msg.content?.video || msg.content?.document),
    mediaType: msg.content?.photo ? 'photo' : msg.content?.video ? 'video' : msg.content?.document ? 'document' : undefined,
  };
}

// Setup the bridge listener
let isSetup = false;

type TelegramQueryEvent = {
  queryId: number;
  queryType: string;
  params: Record<string, unknown>;
};

export default async function setupTelegramTools() {
  if (isSetup) return;
  isSetup = true;

  try {
    const { listen } = await import('@tauri-apps/api/event');
    const { invoke } = await import('@tauri-apps/api/core');

    // Listen for query requests from Tauri backend (event name: telegram-query)
    await listen<TelegramQueryEvent>('telegram-query', async (event) => {
      const { queryId, queryType, params } = event.payload;

      let result: { success: boolean; data?: unknown; error?: string };

      try {
        const handler = queryHandlers[queryType];
        if (!handler) {
          result = {
            success: false,
            error: `Unknown query type: ${queryType}`,
          };
        } else {
          const data = handler(params);
          result = {
            success: true,
            data,
          };
        }
      } catch (error) {
        result = {
          success: false,
          error: error instanceof Error ? error.message : 'Unknown error',
        };
      }

      // Send response back via Tauri command
      try {
        await invoke('telegram_query_response', {
          queryId,
          result,
        });
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error('[TelegramTools] Failed to send response:', err);
      }
    });

    // eslint-disable-next-line no-console
    console.log('[TelegramTools] Initialized successfully');
  } catch (error) {
    // eslint-disable-next-line no-console
    console.error('[TelegramTools] Failed to initialize:', error);
  }
}

// Export query types for documentation
export const SUPPORTED_QUERIES = Object.keys(queryHandlers);
