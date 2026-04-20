import { invoke } from '@tauri-apps/api/core';
import { listen, emit } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-shell';

// Re-implementing the Electron API interface using Tauri's invoke/listen
// This satisfies the requirement of "directly using Tauri instead of bridging"
// by providing a structured way to call Tauri commands that matches the existing code structure.

export const tauriApi: any = {
  platform: navigator.userAgent.includes('Mac') ? 'darwin' : 'win32',
  arch: 'x64',
  store: {
    get: (key: string) => invoke('store_get', { key }),
    set: (key: string, value: any) => invoke('store_set', { key, value }),
    remove: (key: string) => invoke('store_remove', { key }),
  },
  skills: {
    list: () => invoke('skills_list'),
    setEnabled: (options: { id: string; enabled: boolean }) => invoke('skills_set_enabled', options),
    delete: (id: string) => invoke('skills_delete', { id }),
    download: (source: string) => invoke('skills_download', { source }),
    upgrade: (skillId: string, downloadUrl: string) => invoke('skills_upgrade', { skillId, downloadUrl }),
    confirmInstall: (pendingId: string, action: string) => invoke('skills_confirm_install', { pendingId, action }),
    getRoot: () => invoke('skills_get_root'),
    autoRoutingPrompt: () => invoke('skills_auto_routing_prompt'),
    getConfig: (skillId: string) => invoke('skills_get_config', { skillId }),
    setConfig: (skillId: string, config: Record<string, string>) => invoke('skills_set_config', { skillId, config }),
    testEmailConnectivity: (skillId: string, config: Record<string, string>) => invoke('skills_test_email_connectivity', { skillId, config }),
    onChanged: (callback: () => void) => {
      const unlisten = listen('skills_changed', () => callback());
      return () => unlisten.then(u => u());
    },
  },
  mcp: {
    list: () => invoke('mcp_list'),
    create: (data: any) => invoke('mcp_create', { data }),
    update: (id: string, data: any) => invoke('mcp_update', { id, data }),
    delete: (id: string) => invoke('mcp_delete', { id }),
    setEnabled: (options: { id: string; enabled: boolean }) => invoke('mcp_set_enabled', options),
    fetchMarketplace: () => invoke('mcp_fetch_marketplace'),
    refreshBridge: () => invoke('mcp_refresh_bridge'),
    onBridgeSyncStart: (callback: () => void) => {
      const unlisten = listen('mcp_bridge_sync_start', () => callback());
      return () => unlisten.then(u => u());
    },
    onBridgeSyncDone: (callback: (data: { tools: number; error?: string }) => void) => {
      const unlisten = listen('mcp_bridge_sync_done', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
  },
  telegram: {
    queryResponse: (queryId: number, result: any) => invoke('telegram_query_response', { queryId, result }),
  },
  agents: {
    list: () => invoke('agents_list'),
    get: (id: string) => invoke('agents_get', { id }),
    create: (request: any) => invoke('agents_create', { request }),
    update: (id: string, updates: any) => invoke('agents_update', { id, updates }),
    delete: (id: string) => invoke('agents_delete', { id }),
    presets: () => invoke('agents_presets'),
    addPreset: (presetId: string) => invoke('agents_add_preset', { presetId }),
  },
  api: {
    fetch: (options: any) => invoke('api_fetch', options),
    stream: (options: any) => invoke('api_stream', options),
    cancelStream: (requestId: string) => invoke('api_cancel_stream', { requestId }),
    onStreamData: (requestId: string, callback: (chunk: string) => void) => {
      const unlisten = listen(`api_stream_data_${requestId}`, (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamDone: (requestId: string, callback: () => void) => {
      const unlisten = listen(`api_stream_done_${requestId}`, () => callback());
      return () => unlisten.then(u => u());
    },
    onStreamError: (requestId: string, callback: (error: string) => void) => {
      const unlisten = listen(`api_stream_error_${requestId}`, (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamAbort: (requestId: string, callback: () => void) => {
      const unlisten = listen(`api_stream_abort_${requestId}`, () => callback());
      return () => unlisten.then(u => u());
    },
  },
  getApiConfig: () => invoke('get_api_config'),
  checkApiConfig: (options?: any) => invoke('check_api_config', { options: options ?? {} }),
  saveApiConfig: (config: any) => invoke('save_api_config', { config }),
  generateSessionTitle: (userInput: string | null) => invoke('generate_session_title', { userInput }),
  getRecentCwds: (limit?: number) => invoke('get_recent_cwds', { limit }),
  openclaw: {
    engine: {
      getStatus: () => invoke('openclaw_engine_get_status'),
      install: () => invoke('openclaw_engine_install'),
      retryInstall: () => invoke('openclaw_engine_retry_install'),
      restartGateway: () => invoke('openclaw_engine_restart_gateway'),
      onProgress: (callback: (status: any) => void) => {
        const unlisten = listen('openclaw_engine_progress', (event: any) => callback(event.payload));
        return () => unlisten.then(u => u());
      },
    },
    sessionPolicy: {
      get: () => invoke('openclaw_session_policy_get'),
      set: (config: any) => invoke('openclaw_session_policy_set', { config }),
    },
  },
  ipcRenderer: {
    send: (channel: string, ...args: any[]) => emit(channel, args[0]),
    on: (channel: string, func: (...args: any[]) => void) => {
      const unlisten = listen(channel, (event: any) => func(null, event.payload));
      return () => unlisten.then(u => u());
    },
  },
  window: {
    minimize: () => invoke('window_minimize'),
    toggleMaximize: () => invoke('window_toggle_maximize'),
    close: () => invoke('window_close'),
    isMaximized: () => invoke('window_is_maximized'),
    showSystemMenu: (position: any) => invoke('window_show_system_menu', position),
    onStateChanged: (callback: (state: any) => void) => {
      const unlisten = listen('window_state_changed', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
  },
  shell: {
    openPath: (filePath: string) => invoke('shell_open_path', { filePath }),
    showItemInFolder: (filePath: string) => invoke('shell_show_item_in_folder', { filePath }),
    openExternal: (url: string) => open(url),
  },
  cowork: {
    startSession: (options: any) => invoke('cowork_start_session', options),
    continueSession: (options: any) => invoke('cowork_continue_session', options),
    stopSession: (sessionId: string) => invoke('cowork_stop_session', { sessionId }),
    deleteSession: (sessionId: string) => invoke('cowork_delete_session', { sessionId }),
    deleteSessions: (sessionIds: string[]) => invoke('cowork_delete_sessions', { sessionIds }),
    setSessionPinned: (options: any) => invoke('cowork_set_session_pinned', options),
    renameSession: (options: any) => invoke('cowork_rename_session', options),
    getSession: (sessionId: string) => invoke('cowork_get_session', { sessionId }),
    remoteManaged: (sessionId: string) => invoke('cowork_remote_managed', { sessionId }),
    listSessions: (agentId?: string) => invoke('cowork_list_sessions', { agentId }),
    exportResultImage: (options: any) => invoke('cowork_export_result_image', options),
    captureImageChunk: (options: any) => invoke('cowork_capture_image_chunk', options),
    saveResultImage: (options: any) => invoke('cowork_save_result_image', options),
    exportSessionText: (options: any) => invoke('cowork_export_session_text', options),
    respondToPermission: (options: any) => invoke('cowork_respond_to_permission', options),
    getConfig: () => invoke('cowork_get_config'),
    setConfig: (config: any) => invoke('cowork_set_config', { config }),
    listMemoryEntries: (input: any) => invoke('cowork_list_memory_entries', input),
    createMemoryEntry: (input: any) => invoke('cowork_create_memory_entry', input),
    updateMemoryEntry: (input: any) => invoke('cowork_update_memory_entry', input),
    deleteMemoryEntry: (input: { id: string }) => invoke('cowork_delete_memory_entry', input),
    getMemoryStats: () => invoke('cowork_get_memory_stats'),
    readBootstrapFile: (filename: string) => invoke('cowork_read_bootstrap_file', { filename }),
    writeBootstrapFile: (filename: string, content: string) => invoke('cowork_write_bootstrap_file', { filename, content }),
    onStreamMessage: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_message', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamMessageUpdate: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_message_update', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamPermission: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_permission', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamPermissionDismiss: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_permission_dismiss', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamComplete: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_complete', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onStreamError: (callback: (data: any) => void) => {
      const unlisten = listen('cowork_stream_error', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onSessionsChanged: (callback: () => void) => {
      const unlisten = listen('cowork_sessions_changed', () => callback());
      return () => unlisten.then(u => u());
    },
  },
  dialog: {
    selectDirectory: () => invoke('dialog_select_directory'),
    selectFile: (options?: any) => invoke('dialog_select_file', options),
    selectFiles: (options?: any) => invoke('dialog_select_files', options),
    saveInlineFile: (options: any) => invoke('dialog_save_inline_file', options),
    readFileAsDataUrl: (filePath: string) => invoke('dialog_read_file_as_data_url', { filePath }),
  },
  autoLaunch: {
    get: () => invoke('auto_launch_get'),
    set: (enabled: boolean) => invoke('auto_launch_set', { enabled }),
  },
  preventSleep: {
    get: () => invoke('prevent_sleep_get'),
    set: (enabled: boolean) => invoke('prevent_sleep_set', { enabled }),
  },
  appInfo: {
    getVersion: () => invoke('app_get_version'),
    getSystemLocale: () => invoke('app_get_system_locale'),
  },
  appUpdate: {
    download: (url: string) => invoke('app_update_download', { url }),
    cancelDownload: () => invoke('app_update_cancel_download'),
    install: (filePath: string) => invoke('app_update_install', { filePath }),
    onDownloadProgress: (callback: (data: any) => void) => {
      const unlisten = listen('app_update_download_progress', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
  },
  log: {
    getPath: () => invoke('log_get_path'),
    openFolder: () => invoke('log_open_folder'),
    exportZip: () => invoke('log_export_zip'),
  },
  im: {
    getConfig: () => invoke('im_get_config'),
    setConfig: (config: any, options?: any) => invoke('im_set_config', { config, options }),
    syncConfig: () => invoke('im_sync_config'),
    startGateway: (platform: string) => invoke('im_start_gateway', { platform }),
    stopGateway: (platform: string) => invoke('im_stop_gateway', { platform }),
    testGateway: (platform: string, configOverride?: any) => invoke('im_test_gateway', { platform, configOverride }),
    getStatus: () => invoke('im_get_status'),
    getLocalIp: () => invoke('im_get_local_ip'),
    getOpenClawConfigSchema: () => invoke('im_get_openclaw_config_schema'),
    weixinQrLoginStart: () => invoke('im_weixin_qr_login_start'),
    weixinQrLoginWait: (accountId?: string) => invoke('im_weixin_qr_login_wait', { accountId }),
    listPairingRequests: (platform: string) => invoke('im_list_pairing_requests', { platform }),
    approvePairingCode: (platform: string, code: string) => invoke('im_approve_pairing_code', { platform, code }),
    rejectPairingRequest: (platform: string, code: string) => invoke('im_reject_pairing_request', { platform, code }),
    addQQInstance: (name: string) => invoke('im_add_qq_instance', { name }),
    deleteQQInstance: (instanceId: string) => invoke('im_delete_qq_instance', { instanceId }),
    setQQInstanceConfig: (instanceId: string, config: any, options?: any) => invoke('im_set_qq_instance_config', { instanceId, config, options }),
    addFeishuInstance: (name: string) => invoke('im_add_feishu_instance', { name }),
    deleteFeishuInstance: (instanceId: string) => invoke('im_delete_feishu_instance', { instanceId }),
    setFeishuInstanceConfig: (instanceId: string, config: any, options?: any) => invoke('im_set_feishu_instance_config', { instanceId, config, options }),
    addDingTalkInstance: (name: string) => invoke('im_add_dingtalk_instance', { name }),
    deleteDingTalkInstance: (instanceId: string) => invoke('im_delete_dingtalk_instance', { instanceId }),
    setDingTalkInstanceConfig: (instanceId: string, config: any, options?: any) => invoke('im_set_dingtalk_instance_config', { instanceId, config, options }),
    onStatusChange: (callback: (status: any) => void) => {
      const unlisten = listen('im_status_change', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onMessageReceived: (callback: (message: any) => void) => {
      const unlisten = listen('im_message_received', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
  },
  scheduledTasks: {
    list: () => invoke('scheduled_tasks_list'),
    get: (id: string) => invoke('scheduled_tasks_get', { id }),
    create: (input: any) => invoke('scheduled_tasks_create', { input }),
    update: (id: string, input: any) => invoke('scheduled_tasks_update', { id, input }),
    delete: (id: string) => invoke('scheduled_tasks_delete', { id }),
    toggle: (id: string, enabled: boolean) => invoke('scheduled_tasks_toggle', { id, enabled }),
    runManually: (id: string) => invoke('scheduled_tasks_run_manually', { id }),
    stop: (id: string) => invoke('scheduled_tasks_stop', { id }),
    listRuns: (taskId: string, limit?: number, offset?: number) => invoke('scheduled_tasks_list_runs', { taskId, limit, offset }),
    countRuns: (taskId: string) => invoke('scheduled_tasks_count_runs', { taskId }),
    listAllRuns: (limit?: number, offset?: number) => invoke('scheduled_tasks_list_all_runs', { limit, offset }),
    resolveSession: (sessionKey: string) => invoke('scheduled_tasks_resolve_session', { sessionKey }),
    listChannels: () => invoke('scheduled_tasks_list_channels'),
    listChannelConversations: (channel: string, accountId?: string) => invoke('scheduled_tasks_list_channel_conversations', { channel, accountId }),
    onStatusUpdate: (callback: (data: any) => void) => {
      const unlisten = listen('scheduled_tasks_status_update', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onRunUpdate: (callback: (data: any) => void) => {
      const unlisten = listen('scheduled_tasks_run_update', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onRefresh: (callback: () => void) => {
      const unlisten = listen('scheduled_tasks_refresh', () => callback());
      return () => unlisten.then(u => u());
    },
  },
  permissions: {
    checkCalendar: () => invoke('permissions_check_calendar'),
    requestCalendar: () => invoke('permissions_request_calendar'),
  },
  auth: {
    login: (loginUrl?: string) => invoke('auth_login', { loginUrl }),
    exchange: (code: string) => invoke('auth_exchange', { code }),
    getUser: () => invoke('auth_get_user'),
    getQuota: () => invoke('auth_get_quota'),
    logout: () => invoke('auth_logout'),
    refreshToken: () => invoke('auth_refresh_token'),
    getAccessToken: () => invoke('auth_get_access_token'),
    getModels: () => invoke('auth_get_models'),
    getProfileSummary: () => invoke('auth_get_profile_summary'),
    onCallback: (callback: (data: any) => void) => {
      const unlisten = listen('auth_callback', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
    onQuotaChanged: (callback: () => void) => {
      const unlisten = listen('auth_quota_changed', () => callback());
      return () => unlisten.then(u => u());
    },
  },
  enterprise: {
    getConfig: () => invoke('enterprise_get_config'),
  },
  networkStatus: {
    send: (status: 'online' | 'offline') => emit('network_status', { status }),
  },
  feishu: {
    install: {
      qrcode: (isLark: boolean) => invoke('feishu_install_qrcode', { isLark }),
      poll: (deviceCode: string) => invoke('feishu_install_poll', { deviceCode }),
      verify: (appId: string, appSecret: string) => invoke('feishu_install_verify', { appId, appSecret }),
    },
  },
  githubCopilot: {
    requestDeviceCode: () => invoke('github_copilot_request_device_code'),
    pollForToken: (deviceCode: string, interval: number, expiresIn: number) => invoke('github_copilot_poll_for_token', { deviceCode, interval, expiresIn }),
    cancelPolling: () => invoke('github_copilot_cancel_polling'),
    signOut: () => invoke('github_copilot_sign_out'),
    refreshToken: () => invoke('github_copilot_refresh_token'),
    onTokenUpdated: (callback: (data: any) => void) => {
      const unlisten = listen('github_copilot_token_updated', (event: any) => callback(event.payload));
      return () => unlisten.then(u => u());
    },
  },
};
