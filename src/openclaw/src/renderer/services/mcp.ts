import { McpServerConfig, McpServerFormData, McpRegistryEntry, McpMarketplaceCategoryInfo, McpCategory, McpMarketplaceServer } from '../types/mcp';
import { tauriApi as api } from '../lib/tauri';

/**
 * Convert remote marketplace server data to McpRegistryEntry format.
 */
function convertMarketplaceToRegistry(
  servers: McpMarketplaceServer[],
): McpRegistryEntry[] {
  return servers.map((s) => ({
    id: s.id,
    name: s.name,
    descriptionKey: '',
    description_zh: s.description_zh,
    description_en: s.description_en,
    category: s.category as McpCategory,
    categoryKey: '',
    transportType: s.transportType as McpRegistryEntry['transportType'],
    command: s.command,
    defaultArgs: s.defaultArgs,
    requiredEnvKeys: s.requiredEnvKeys,
    optionalEnvKeys: s.optionalEnvKeys,
  }));
}

class McpService {
  private servers: McpServerConfig[] = [];
  private initialized = false;

  async init(): Promise<void> {
    if (this.initialized) return;
    await this.loadServers();
    this.initialized = true;
  }

  async loadServers(): Promise<McpServerConfig[]> {
    try {
      const result = await api.mcp.list() as any;
      if (result.success && result.servers) {
        this.servers = result.servers;
      } else {
        this.servers = [];
      }
      return this.servers;
    } catch (error) {
      console.error('Failed to load MCP servers:', error);
      this.servers = [];
      return this.servers;
    }
  }

  async createServer(data: McpServerFormData): Promise<{ success: boolean; servers?: McpServerConfig[]; error?: string }> {
    try {
      const result = await api.mcp.create(data) as any;
      if (result.success && result.servers) {
        this.servers = result.servers;
      }
      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to create MCP server';
      console.error('Failed to create MCP server:', error);
      return { success: false, error: message };
    }
  }

  async updateServer(id: string, data: Partial<McpServerFormData>): Promise<{ success: boolean; servers?: McpServerConfig[]; error?: string }> {
    try {
      const result = await api.mcp.update(id, data) as any;
      if (result.success && result.servers) {
        this.servers = result.servers;
      }
      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to update MCP server';
      console.error('Failed to update MCP server:', error);
      return { success: false, error: message };
    }
  }

  async deleteServer(id: string): Promise<{ success: boolean; servers?: McpServerConfig[]; error?: string }> {
    try {
      const result = await api.mcp.delete(id) as any;
      if (result.success && result.servers) {
        this.servers = result.servers;
      }
      return result;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to delete MCP server';
      console.error('Failed to delete MCP server:', error);
      return { success: false, error: message };
    }
  }

  async setServerEnabled(id: string, enabled: boolean): Promise<McpServerConfig[]> {
    try {
      const result = await api.mcp.setEnabled({ id, enabled }) as any;
      if (result.success && result.servers) {
        this.servers = result.servers;
        return this.servers;
      }
      throw new Error(result.error || 'Failed to update MCP server');
    } catch (error) {
      console.error('Failed to update MCP server:', error);
      throw error;
    }
  }

  getServers(): McpServerConfig[] {
    return this.servers;
  }

  getEnabledServers(): McpServerConfig[] {
    return this.servers.filter(s => s.enabled);
  }

  getServerById(id: string): McpServerConfig | undefined {
    return this.servers.find(s => s.id === id);
  }

  async fetchMarketplace(): Promise<{
    registry: McpRegistryEntry[];
    categories: McpMarketplaceCategoryInfo[];
  } | null> {
    try {
      const result = await api.mcp.fetchMarketplace() as any;
      if (result.success && result.data) {
        const registry = convertMarketplaceToRegistry(result.data.servers);
        return { registry, categories: result.data.categories };
      }
      return null;
    } catch (error) {
      console.error('Failed to fetch MCP marketplace:', error);
      return null;
    }
  }

  /**
   * Refresh the MCP Bridge: restarts MCP servers, re-discovers tools,
   * syncs openclaw.json, and restarts the gateway.
   * Returns the number of tools discovered.
   */
  async refreshBridge(): Promise<{ success: boolean; tools: number; error?: string }> {
    try {
      return await api.mcp.refreshBridge() as any;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to refresh MCP bridge';
      console.error('Failed to refresh MCP bridge:', error);
      return { success: false, tools: 0, error: message };
    }
  }

  onBridgeSyncStart(callback: () => void): () => void {
    return api.mcp.onBridgeSyncStart(callback);
  }

  onBridgeSyncDone(callback: (data: { tools: number; error?: string }) => void): () => void {
    return api.mcp.onBridgeSyncDone(callback);
  }
}

export const mcpService = new McpService();
